/// Marks the line on curl's stderr that carries response metadata for gurl.
pub const META_SENTINEL: &str = "__GURL_META__";

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// Sent exactly as given (`--data-raw`): no `@file` expansion
    Raw(String),
    /// Contents of a file, or stdin for `-`, sent byte for byte (`--data-binary @path`)
    File(String),
    /// A multipart/form-data upload
    Multipart(Vec<Part>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    /// `--form-string name=value`: the value is literal, even with a leading `@` or `<`
    Text { name: String, value: String },
    /// `-F name=@path`: a file upload
    File { name: String, path: String },
}

/// Everything that determines the curl invocation.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CurlRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<String>,
    pub body: Option<Body>,
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
/// for itself (`-sSN`, the metadata `-w`).
pub fn display_curl_args(req: &CurlRequest) -> Vec<String> {
    build(req, false)
}

const MASK: &str = "***";

impl CurlRequest {
    /// A copy that is safe to show on screen: credentials replaced by `***`.
    pub fn masked(&self) -> CurlRequest {
        let mut masked = self.clone();
        for header in &mut masked.headers {
            if let Some((name, _)) = header.split_once(':')
                && ["authorization", "proxy-authorization"]
                    .contains(&name.trim().to_lowercase().as_str())
            {
                *header = format!("{name}: {MASK}");
            }
        }
        if let Some(user) = &mut masked.user
            && let Some((name, _)) = user.split_once(':')
        {
            *user = format!("{name}:{MASK}");
        }
        masked
    }
}

/// Quote `arg` for a POSIX shell, only when needed.
pub fn shell_quote(arg: &str) -> String {
    let is_safe = |c: char| c.is_ascii_alphanumeric() || "_-./:=@%+,".contains(c);
    if !arg.is_empty() && arg.chars().all(is_safe) {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// The equivalent curl command line, ready to paste into a shell.
pub fn display_command(req: &CurlRequest) -> String {
    std::iter::once("curl".to_string())
        .chain(display_curl_args(req).iter().map(|arg| shell_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn build(req: &CurlRequest, plumbing: bool) -> Vec<String> {
    let mut args = Vec::new();

    // Let curl infer the method where it can: a forced -X is also applied to
    // every request of a redirect chain followed with -L.
    match (req.method.as_str(), req.body.is_some()) {
        ("GET", false) | ("POST", true) => {}
        // -X HEAD would make curl wait for a body that never comes
        ("HEAD", false) => args.push("-I".into()),
        _ => {
            args.push("-X".into());
            args.push(req.method.clone());
        }
    }

    for header in &req.headers {
        args.push("-H".into());
        args.push(header.clone());
    }

    match &req.body {
        Some(Body::Raw(data)) => {
            args.push("--data-raw".into());
            args.push(data.clone());
        }
        Some(Body::File(path)) => {
            args.push("--data-binary".into());
            args.push(format!("@{path}"));
        }
        Some(Body::Multipart(parts)) => {
            for part in parts {
                match part {
                    Part::Text { name, value } => {
                        args.push("--form-string".into());
                        args.push(format!("{name}={value}"));
                    }
                    Part::File { name, path } => {
                        args.push("-F".into());
                        args.push(format!("{name}=@{path}"));
                    }
                }
            }
        }
        None => {}
    }

    if req.verbose {
        args.push("-v".into());
    }
    if req.include {
        args.push("-i".into());
    }

    // -sS: no progress meter (it would interleave with our output), but keep
    // errors. -N: curl's stdout is never a terminal here (it is a pipe to
    // gurl, or gurl's own redirected stdout), so without it curl holds the
    // response back in a buffer instead of streaming it.
    if plumbing {
        args.push("-sSN".into());
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
            vec!["-sSN", "-w", &meta, "https://example.com"]
        );
    }

    #[test]
    fn build_curl_args_post_with_body_and_headers() {
        let req = CurlRequest {
            method: "POST".into(),
            url: "https://example.com/api".into(),
            headers: vec!["Content-Type: application/json".into(), "X-A: 1".into()],
            body: Some(Body::Raw(r#"{"key":"value"}"#.into())),
            ..Default::default()
        };
        let meta = meta_write_out();
        assert_eq!(
            strs(&build_curl_args(&req)),
            vec![
                "-H",
                "Content-Type: application/json",
                "-H",
                "X-A: 1",
                "--data-raw",
                r#"{"key":"value"}"#,
                "-sSN",
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
                "-v",
                "-i",
                "-sSN",
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

    fn method_args(method: &str, body: Option<Body>) -> Vec<String> {
        let req = CurlRequest {
            method: method.into(),
            url: "u".into(),
            body,
            ..Default::default()
        };
        display_curl_args(&req)
            .into_iter()
            .filter(|a| a != "u" && a != "--data-raw" && a != "x")
            .collect()
    }

    #[test]
    fn method_is_only_forced_when_curl_would_pick_another() {
        let body = || Some(Body::Raw("x".into()));
        assert!(method_args("GET", None).is_empty());
        assert!(method_args("POST", body()).is_empty());
        assert_eq!(method_args("HEAD", None), vec!["-I"]);
        assert_eq!(method_args("POST", None), vec!["-X", "POST"]);
        assert_eq!(method_args("GET", body()), vec!["-X", "GET"]);
        assert_eq!(method_args("PUT", body()), vec!["-X", "PUT"]);
        assert_eq!(method_args("DELETE", None), vec!["-X", "DELETE"]);
        assert_eq!(method_args("HEAD", body()), vec!["-X", "HEAD"]);
    }

    #[test]
    fn file_body_uses_data_binary() {
        let req = CurlRequest {
            method: "POST".into(),
            url: "u".into(),
            body: Some(Body::File("big.json".into())),
            ..Default::default()
        };
        assert_eq!(
            strs(&display_curl_args(&req)),
            vec!["--data-binary", "@big.json", "u"]
        );
    }

    #[test]
    fn multipart_body_uses_form_flags() {
        let req = CurlRequest {
            method: "POST".into(),
            url: "u".into(),
            body: Some(Body::Multipart(vec![
                Part::Text {
                    name: "title".into(),
                    value: "@not-a-file".into(),
                },
                Part::File {
                    name: "doc".into(),
                    path: "a b.pdf".into(),
                },
            ])),
            ..Default::default()
        };
        assert_eq!(
            strs(&display_curl_args(&req)),
            vec![
                "--form-string",
                "title=@not-a-file",
                "-F",
                "doc=@a b.pdf",
                "u"
            ]
        );
    }

    #[test]
    fn shell_quote_leaves_plain_words_alone() {
        assert_eq!(shell_quote("-X"), "-X");
        assert_eq!(
            shell_quote("https://example.com/a?b=1"),
            "'https://example.com/a?b=1'"
        );
        assert_eq!(
            shell_quote("https://example.com/a"),
            "https://example.com/a"
        );
        assert_eq!(shell_quote("@file.json"), "@file.json");
    }

    #[test]
    fn shell_quote_wraps_special_characters() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("X-A: 1"), "'X-A: 1'");
        assert_eq!(shell_quote(r#"{"a":"b"}"#), r#"'{"a":"b"}'"#);
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
    }

    #[test]
    fn display_command_is_copy_pasteable() {
        let req = CurlRequest {
            method: "PUT".into(),
            url: "https://example.com/users/1".into(),
            headers: vec!["Content-Type: application/json".into()],
            body: Some(Body::Raw(r#"{"name":"O'Brien"}"#.into())),
            ..Default::default()
        };
        assert_eq!(
            display_command(&req),
            r#"curl -X PUT -H 'Content-Type: application/json' --data-raw '{"name":"O'\''Brien"}' https://example.com/users/1"#
        );
    }

    #[test]
    fn masked_hides_credentials_only() {
        let req = CurlRequest {
            headers: vec![
                "Authorization: Bearer secret".into(),
                "proxy-authorization: Basic abc".into(),
                "X-Authorization-Hint: visible".into(),
            ],
            user: Some("admin:hunter2".into()),
            ..Default::default()
        };
        let masked = req.masked();
        assert_eq!(
            masked.headers,
            vec![
                "Authorization: ***",
                "proxy-authorization: ***",
                "X-Authorization-Hint: visible"
            ]
        );
        assert_eq!(masked.user.as_deref(), Some("admin:***"));

        // A user without a password makes curl prompt for it: nothing to hide
        let prompt = CurlRequest {
            user: Some("admin".into()),
            ..Default::default()
        };
        assert_eq!(prompt.masked().user.as_deref(), Some("admin"));
    }

    #[test]
    fn meta_write_out_targets_stderr_on_its_own_line() {
        let w = meta_write_out();
        assert!(w.starts_with("%{stderr}\n__GURL_META__ %{http_code} "));
        assert!(w.ends_with(" %{content_type}\n"));
    }
}
