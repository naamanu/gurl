use anyhow::Context;
use clap::Parser;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "A simple curl wrapper for easier terminal usage",
    long_about = None,
    after_help = "\
Examples:
  gurl https://api.example.com/users          GET request (default)
  gurl :8080/health                            Localhost shorthand
  gurl -m POST -d '{\"name\":\"foo\"}' :3000/api  POST with JSON body
  gurl -f request.json                         Load request from file
  gurl --pretty https://api.example.com/data   Pretty-print JSON response
  gurl -H 'Authorization: Bearer tok' api.com  Custom header
  gurl -L -t 30 https://example.com            Follow redirects, 30s timeout
  gurl https://example.com -- -k --compressed  Pass extra args to curl"
)]
pub struct Args {
    /// The URL to request (use :port/path as shorthand for localhost)
    /// Can be omitted if using --file with url field
    pub url: Option<String>,

    /// Request method (GET, POST, PUT, DELETE, PATCH, etc.)
    #[arg(short = 'm', long)]
    pub method: Option<String>,

    /// Data payload (auto-detects JSON and sets Content-Type)
    #[arg(short, long)]
    pub data: Option<String>,

    /// Headers to include (can be used multiple times)
    #[arg(short = 'H', long)]
    pub headers: Vec<String>,

    /// Load request from JSON file (headers, body, method, url)
    #[arg(short, long)]
    pub file: Option<PathBuf>,

    /// Include HTTP headers in the output
    #[arg(short = 'i', long)]
    pub include: bool,

    /// Verbose output (shows full curl command)
    #[arg(short, long)]
    pub verbose: bool,

    /// Silent mode (suppress progress meter)
    #[arg(short, long)]
    pub silent: bool,

    /// Follow redirects
    #[arg(short = 'L', long)]
    pub location: bool,

    /// Request timeout in seconds
    #[arg(short = 't', long)]
    pub timeout: Option<u64>,

    /// Save response to file
    #[arg(short, long)]
    pub output: Option<String>,

    /// HTTP basic auth (user:password)
    #[arg(short, long)]
    pub user: Option<String>,

    /// Pretty print JSON output with syntax highlighting
    #[arg(short, long)]
    pub pretty: bool,

    /// Extra arguments passed directly to curl
    #[arg(last = true)]
    pub extra_args: Vec<String>,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_format_deserialize_array() {
        let json = r#"["Content-Type: application/json", "Accept: text/html"]"#;
        let hf: HeadersFormat = serde_json::from_str(json).unwrap();
        let vec = hf.to_vec();
        assert_eq!(vec.len(), 2);
        assert!(vec.contains(&"Content-Type: application/json".to_string()));
        assert!(vec.contains(&"Accept: text/html".to_string()));
    }

    #[test]
    fn headers_format_deserialize_object() {
        let json = r#"{"Content-Type": "application/json"}"#;
        let hf: HeadersFormat = serde_json::from_str(json).unwrap();
        let vec = hf.to_vec();
        assert_eq!(vec.len(), 1);
        assert_eq!(vec[0], "Content-Type: application/json");
    }

    #[test]
    fn headers_format_none_to_vec() {
        let hf = HeadersFormat::None;
        assert!(hf.to_vec().is_empty());
    }

    #[test]
    fn request_file_deserialize_full() {
        let json = r#"{
            "method": "POST",
            "url": "https://example.com",
            "headers": {"Authorization": "Bearer token"},
            "body": {"key": "value"}
        }"#;
        let rf: RequestFile = serde_json::from_str(json).unwrap();
        assert_eq!(rf.method.as_deref(), Some("POST"));
        assert_eq!(rf.url.as_deref(), Some("https://example.com"));
        assert!(rf.body.is_some());
    }

    #[test]
    fn request_file_deserialize_minimal() {
        let json = r#"{}"#;
        let rf: RequestFile = serde_json::from_str(json).unwrap();
        assert!(rf.method.is_none());
        assert!(rf.url.is_none());
        assert!(rf.body.is_none());
    }

    #[test]
    fn request_file_string_body() {
        let json = r#"{"body": "plain text"}"#;
        let rf: RequestFile = serde_json::from_str(json).unwrap();
        assert_eq!(rf.body.unwrap().as_str().unwrap(), "plain text");
    }

    #[test]
    fn request_file_object_body() {
        let json = r#"{"body": {"title": "hello"}}"#;
        let rf: RequestFile = serde_json::from_str(json).unwrap();
        assert!(rf.body.unwrap().is_object());
    }

    #[test]
    fn load_request_file_missing() {
        let result = load_request_file(Path::new("/nonexistent/file.json"));
        assert!(result.is_err());
    }

    #[test]
    fn load_request_file_invalid_json() {
        let dir = std::env::temp_dir();
        let path = dir.join("gurl_test_invalid.json");
        std::fs::write(&path, "not json").unwrap();
        let result = load_request_file(&path);
        assert!(result.is_err());
        std::fs::remove_file(&path).ok();
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
}
