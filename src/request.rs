use crate::args::Args;
use crate::curl::CurlRequest;
use crate::url::expand_url;
use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Request configuration loaded from a file
#[derive(Debug, Deserialize, Default)]
pub struct RequestFile {
    pub url: Option<String>,
    pub method: Option<String>,
    #[serde(default)]
    pub headers: HeadersFormat,
    pub body: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
pub enum HeadersFormat {
    #[default]
    None,
    /// Array of "Key: Value" strings
    Array(Vec<String>),
    /// Object { "Key": "Value" }
    Object(HashMap<String, String>),
}

impl HeadersFormat {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            HeadersFormat::None => vec![],
            HeadersFormat::Array(arr) => arr.clone(),
            HeadersFormat::Object(obj) => obj.iter().map(|(k, v)| format!("{k}: {v}")).collect(),
        }
    }
}

pub fn load_request_file(path: &Path) -> anyhow::Result<RequestFile> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read request file: {}", path.display()))?;

    let request: RequestFile = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse request file: {}", path.display()))?;

    Ok(request)
}

pub fn looks_like_json(data: &str) -> bool {
    let trimmed = data.trim();
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

pub fn has_content_type_header(headers: &[String]) -> bool {
    headers
        .iter()
        .any(|h| h.to_lowercase().starts_with("content-type"))
}

/// Merge CLI arguments with an optional request file into the request to send.
/// CLI values win over file values; file headers come before CLI headers.
pub fn resolve(args: &Args, file: Option<&RequestFile>) -> anyhow::Result<CurlRequest> {
    let url = args
        .url
        .clone()
        .or_else(|| file.and_then(|r| r.url.clone()))
        .ok_or_else(|| anyhow::anyhow!("URL is required (provide as argument or in --file)"))?;
    let url = expand_url(&url);

    let method = args
        .method
        .clone()
        .or_else(|| file.and_then(|r| r.method.clone()))
        .unwrap_or_else(|| "GET".to_string())
        .to_uppercase();

    let mut headers: Vec<String> = file.map(|r| r.headers.to_vec()).unwrap_or_default();
    headers.extend(args.headers.iter().cloned());

    let body: Option<String> = args.data.clone().or_else(|| {
        file.and_then(|r| r.body.as_ref()).map(|b| {
            b.as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(b).unwrap_or_default())
        })
    });

    // Auto-detect JSON and add Content-Type if missing
    if let Some(ref data) = body
        && looks_like_json(data)
        && !has_content_type_header(&headers)
    {
        headers.push("Content-Type: application/json".to_string());
    }

    Ok(CurlRequest {
        method,
        url,
        headers,
        body,
        verbose: args.verbose,
        include: args.include,
        location: args.location,
        timeout: args.timeout,
        output: args.output.clone(),
        user: args.user.clone(),
        extra_args: args.extra_args.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Write;

    fn args(argv: &[&str]) -> Args {
        Args::parse_from(std::iter::once("gurl").chain(argv.iter().copied()))
    }

    fn file(json: &str) -> RequestFile {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn headers_format_deserialize_array() {
        let json = r#"["Content-Type: application/json", "Accept: text/html"]"#;
        let hf: HeadersFormat = serde_json::from_str(json).unwrap();
        assert_eq!(
            hf.to_vec(),
            vec!["Content-Type: application/json", "Accept: text/html"]
        );
    }

    #[test]
    fn headers_format_deserialize_object() {
        let json = r#"{"Content-Type": "application/json"}"#;
        let hf: HeadersFormat = serde_json::from_str(json).unwrap();
        assert_eq!(hf.to_vec(), vec!["Content-Type: application/json"]);
    }

    #[test]
    fn headers_format_none_to_vec() {
        assert!(HeadersFormat::None.to_vec().is_empty());
    }

    #[test]
    fn request_file_deserialize_full() {
        let rf = file(
            r#"{
            "method": "POST",
            "url": "https://example.com",
            "headers": {"Authorization": "Bearer token"},
            "body": {"key": "value"}
        }"#,
        );
        assert_eq!(rf.method.as_deref(), Some("POST"));
        assert_eq!(rf.url.as_deref(), Some("https://example.com"));
        assert_eq!(rf.headers.to_vec(), vec!["Authorization: Bearer token"]);
        assert_eq!(rf.body, Some(serde_json::json!({"key": "value"})));
    }

    #[test]
    fn request_file_deserialize_minimal() {
        let rf = file("{}");
        assert!(rf.method.is_none());
        assert!(rf.url.is_none());
        assert!(rf.body.is_none());
        assert!(rf.headers.to_vec().is_empty());
    }

    #[test]
    fn load_request_file_missing() {
        let err = load_request_file(Path::new("/nonexistent/file.json")).unwrap_err();
        assert!(err.to_string().contains("Failed to read request file"));
    }

    #[test]
    fn load_request_file_invalid_json() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"not json").unwrap();
        let err = load_request_file(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("Failed to parse request file"));
    }

    #[test]
    fn load_request_file_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/create_post.json");
        let rf = load_request_file(&path).unwrap();
        assert_eq!(rf.method.as_deref(), Some("POST"));
        assert_eq!(
            rf.url.as_deref(),
            Some("https://jsonplaceholder.typicode.com/posts")
        );
        assert!(rf.body.is_some());
    }

    #[test]
    fn looks_like_json_accepts_objects_and_arrays() {
        assert!(looks_like_json(r#"{"key": "value"}"#));
        assert!(looks_like_json("[1, 2, 3]"));
        assert!(looks_like_json("  { \"key\": 1 }  "));
        assert!(looks_like_json("\n[]\n"));
    }

    #[test]
    fn looks_like_json_rejects_non_json() {
        assert!(!looks_like_json(""));
        assert!(!looks_like_json("hello world"));
        assert!(!looks_like_json("{incomplete"));
        assert!(!looks_like_json("[incomplete"));
    }

    #[test]
    fn has_content_type_header_is_case_insensitive() {
        assert!(has_content_type_header(&[
            "Content-Type: application/json".to_string()
        ]));
        assert!(has_content_type_header(&[
            "content-type: text/plain".to_string()
        ]));
    }

    #[test]
    fn has_content_type_header_absent() {
        assert!(!has_content_type_header(&[
            "Authorization: Bearer token".to_string()
        ]));
        assert!(!has_content_type_header(&[]));
    }

    #[test]
    fn resolve_requires_url() {
        let err = resolve(&args(&[]), None).unwrap_err();
        assert!(err.to_string().contains("URL is required"));
    }

    #[test]
    fn resolve_defaults_to_get_and_expands_url() {
        let req = resolve(&args(&[":3000/health"]), None).unwrap();
        assert_eq!(
            req,
            CurlRequest {
                method: "GET".into(),
                url: "http://localhost:3000/health".into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn resolve_uses_file_values() {
        let rf = file(
            r#"{"method": "post", "url": "example.com/api",
                "headers": ["X-A: 1"], "body": {"k": "v"}}"#,
        );
        let req = resolve(&args(&[]), Some(&rf)).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.url, "https://example.com/api");
        assert_eq!(req.body.as_deref(), Some(r#"{"k":"v"}"#));
        assert_eq!(
            req.headers,
            vec!["X-A: 1", "Content-Type: application/json"]
        );
    }

    #[test]
    fn resolve_cli_overrides_file() {
        let rf = file(r#"{"method": "POST", "url": "https://file.test", "body": "from file"}"#);
        let req = resolve(
            &args(&["-m", "put", "-d", "from cli", "https://cli.test"]),
            Some(&rf),
        )
        .unwrap();
        assert_eq!(req.method, "PUT");
        assert_eq!(req.url, "https://cli.test");
        assert_eq!(req.body.as_deref(), Some("from cli"));
    }

    #[test]
    fn resolve_file_headers_precede_cli_headers() {
        let rf = file(r#"{"url": "https://x.test", "headers": ["X-File: 1"]}"#);
        let req = resolve(&args(&["-H", "X-Cli: 2"]), Some(&rf)).unwrap();
        assert_eq!(req.headers, vec!["X-File: 1", "X-Cli: 2"]);
    }

    #[test]
    fn resolve_string_body_is_sent_verbatim() {
        let rf = file(r#"{"url": "https://x.test", "body": "plain text"}"#);
        let req = resolve(&args(&[]), Some(&rf)).unwrap();
        assert_eq!(req.body.as_deref(), Some("plain text"));
        assert!(req.headers.is_empty());
    }

    #[test]
    fn resolve_keeps_explicit_content_type() {
        let req = resolve(
            &args(&[
                "-H",
                "content-type: application/vnd.api+json",
                "-d",
                "{}",
                "https://x.test",
            ]),
            None,
        )
        .unwrap();
        assert_eq!(req.headers, vec!["content-type: application/vnd.api+json"]);
    }

    #[test]
    fn resolve_copies_curl_flags() {
        let req = resolve(
            &args(&[
                "-v",
                "-i",
                "-L",
                "-t",
                "30",
                "-o",
                "out.json",
                "-u",
                "admin:pass",
                "https://x.test",
                "--",
                "-k",
            ]),
            None,
        )
        .unwrap();
        assert_eq!(
            req,
            CurlRequest {
                method: "GET".into(),
                url: "https://x.test".into(),
                verbose: true,
                include: true,
                location: true,
                timeout: Some(30),
                output: Some("out.json".into()),
                user: Some("admin:pass".into()),
                extra_args: vec!["-k".into()],
                ..Default::default()
            }
        );
    }
}
