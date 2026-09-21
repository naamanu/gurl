/// Marks the line on curl's stderr that carries response metadata for gurl.
pub const META_SENTINEL: &str = "__GURL_META__";

/// Everything that determines the curl invocation.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CurlRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<String>,
    pub body: Option<String>,
    pub verbose: bool,
    pub include: bool,
    pub location: bool,
    pub timeout: Option<u64>,
    pub output: Option<String>,
    pub user: Option<String>,
    pub extra_args: Vec<String>,
}

/// curl `--write-out` format that reports response metadata on stderr, on a
/// line of its own, so stdout carries nothing but the response.
/// Content type goes last because it may contain spaces.
pub fn meta_write_out() -> String {
    format!(
        "%{{stderr}}\n{META_SENTINEL} %{{http_code}} %{{time_total}} %{{size_download}} %{{content_type}}\n"
    )
}

/// The arguments curl is actually run with.
pub fn build_curl_args(req: &CurlRequest) -> Vec<String> {
    build(req, true)
}

/// The arguments as the user would type them: without the plumbing gurl adds
/// for itself (`-sS`, the metadata `-w`).
pub fn display_curl_args(req: &CurlRequest) -> Vec<String> {
    build(req, false)
}

fn build(req: &CurlRequest, plumbing: bool) -> Vec<String> {
    let mut args = Vec::new();

    args.push("-X".into());
    args.push(req.method.clone());

    for header in &req.headers {
        args.push("-H".into());
        args.push(header.clone());
    }

    if let Some(ref data) = req.body {
        args.push("-d".into());
        args.push(data.clone());
    }

    if req.verbose {
        args.push("-v".into());
    }
    if req.include {
        args.push("-i".into());
    }

    // No progress meter (it would interleave with our output), but keep errors
    if plumbing {
        args.push("-sS".into());
    }

    if req.location {
        args.push("-L".into());
    }

    if let Some(timeout) = req.timeout {
        args.push("--max-time".into());
        args.push(timeout.to_string());
    }

    if let Some(ref output) = req.output {
        args.push("-o".into());
        args.push(output.clone());
    }

    if let Some(ref user) = req.user {
        args.push("-u".into());
        args.push(user.clone());
    }

    // Before the extra args, so a user-supplied -w wins over ours
    if plumbing {
        args.push("-w".into());
        args.push(meta_write_out());
    }

    args.extend(req.extra_args.iter().cloned());

    args.push(req.url.clone());

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(args: &[String]) -> Vec<&str> {
        args.iter().map(String::as_str).collect()
    }

    #[test]
    fn build_curl_args_basic_get() {
        let req = CurlRequest {
            method: "GET".into(),
            url: "https://example.com".into(),
            ..Default::default()
        };
        let meta = meta_write_out();
        assert_eq!(
            strs(&build_curl_args(&req)),
            vec!["-X", "GET", "-sS", "-w", &meta, "https://example.com"]
        );
    }

    #[test]
    fn build_curl_args_post_with_body_and_headers() {
        let req = CurlRequest {
            method: "POST".into(),
            url: "https://example.com/api".into(),
            headers: vec!["Content-Type: application/json".into(), "X-A: 1".into()],
            body: Some(r#"{"key":"value"}"#.into()),
            ..Default::default()
        };
        let meta = meta_write_out();
        assert_eq!(
            strs(&build_curl_args(&req)),
            vec![
                "-X",
                "POST",
                "-H",
                "Content-Type: application/json",
                "-H",
                "X-A: 1",
                "-d",
                r#"{"key":"value"}"#,
                "-sS",
                "-w",
                &meta,
                "https://example.com/api"
            ]
        );
    }

    #[test]
    fn build_curl_args_with_all_flags() {
        let req = CurlRequest {
            method: "GET".into(),
            url: "https://example.com".into(),
            verbose: true,
            include: true,
            location: true,
            timeout: Some(30),
            output: Some("out.json".into()),
            user: Some("admin:pass".into()),
            extra_args: vec!["-k".into(), "--compressed".into()],
            ..Default::default()
        };
        let meta = meta_write_out();
        assert_eq!(
            strs(&build_curl_args(&req)),
            vec![
                "-X",
                "GET",
                "-v",
                "-i",
                "-sS",
                "-L",
                "--max-time",
                "30",
                "-o",
                "out.json",
                "-u",
                "admin:pass",
                "-w",
                &meta,
                "-k",
                "--compressed",
                "https://example.com"
            ]
        );
    }

    #[test]
    fn user_write_out_comes_after_ours() {
        let req = CurlRequest {
            method: "GET".into(),
            url: "https://example.com".into(),
            extra_args: vec!["-w".into(), "%{http_code}".into()],
            ..Default::default()
        };
        let args = build_curl_args(&req);
        let positions: Vec<_> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-w")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), 2);
        assert_eq!(args[positions[0] + 1], meta_write_out());
        assert_eq!(args[positions[1] + 1], "%{http_code}");
    }

    #[test]
    fn display_args_omit_plumbing() {
        let req = CurlRequest {
            method: "DELETE".into(),
            url: "https://example.com/1".into(),
            location: true,
            extra_args: vec!["-k".into()],
            ..Default::default()
        };
        assert_eq!(
            strs(&display_curl_args(&req)),
            vec!["-X", "DELETE", "-L", "-k", "https://example.com/1"]
        );
    }

    #[test]
    fn meta_write_out_targets_stderr_on_its_own_line() {
        let w = meta_write_out();
        assert!(w.starts_with("%{stderr}\n__GURL_META__ %{http_code} "));
        assert!(w.ends_with(" %{content_type}\n"));
    }
}
