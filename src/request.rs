use crate::args::Args;
use crate::curl::{Body, CurlRequest};
use crate::items::{self, Item};
use crate::url::{append_query, expand_url};
use crate::vars::Vars;
use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

/// Request configuration loaded from a file
#[derive(Debug, Deserialize, Default, Clone)]
pub struct RequestFile {
    pub url: Option<String>,
    pub method: Option<String>,
    #[serde(default)]
    pub headers: HeadersFormat,
    pub body: Option<Value>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(untagged)]
pub enum HeadersFormat {
    #[default]
    None,
    /// Array of "Key: Value" strings
    Array(Vec<String>),
    /// Object { "Key": "Value" }, sent in the order written. A `null` value
    /// becomes `Key:`, which tells curl to drop one of its default headers.
    Object(Map<String, Value>),
}

impl HeadersFormat {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            HeadersFormat::None => vec![],
            HeadersFormat::Array(arr) => arr.clone(),
            HeadersFormat::Object(obj) => obj
                .iter()
                .map(|(name, value)| match value {
                    Value::Null => format!("{name}:"),
                    Value::String(s) => format!("{name}: {s}"),
                    other => format!("{name}: {other}"),
                })
                .collect(),
        }
    }
}

impl RequestFile {
    /// Replace `{{NAME}}` variables in every string: url, method, header
    /// names and values, and body strings (object keys included). Variables
    /// are substituted into parsed JSON, so a value containing quotes can't
    /// break the body's structure.
    pub fn substitute(&self, vars: &Vars) -> anyhow::Result<RequestFile> {
        let sub_opt = |s: &Option<String>| s.as_deref().map(|s| vars.substitute(s)).transpose();
        Ok(RequestFile {
            url: sub_opt(&self.url)?,
            method: sub_opt(&self.method)?,
            headers: match &self.headers {
                HeadersFormat::None => HeadersFormat::None,
                HeadersFormat::Array(arr) => HeadersFormat::Array(
                    arr.iter()
                        .map(|h| vars.substitute(h))
                        .collect::<anyhow::Result<_>>()?,
                ),
                HeadersFormat::Object(obj) => {
                    match substitute_value(&Value::Object(obj.clone()), vars)? {
                        Value::Object(obj) => HeadersFormat::Object(obj),
                        _ => unreachable!("an object stays an object"),
                    }
                }
            },
            body: self
                .body
                .as_ref()
                .map(|b| substitute_value(b, vars))
                .transpose()?,
        })
    }
}

fn substitute_value(value: &Value, vars: &Vars) -> anyhow::Result<Value> {
    Ok(match value {
        Value::String(s) => Value::String(vars.substitute(s)?),
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| substitute_value(v, vars))
                .collect::<anyhow::Result<_>>()?,
        ),
        Value::Object(obj) => Value::Object(
            obj.iter()
                .map(|(k, v)| Ok((vars.substitute(k)?, substitute_value(v, vars)?)))
                .collect::<anyhow::Result<_>>()?,
        ),
        other => other.clone(),
    })
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

fn has_header(headers: &[String], wanted: &str) -> bool {
    headers.iter().any(|h| {
        h.split_once(':')
            .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case(wanted))
    })
}

pub fn has_content_type_header(headers: &[String]) -> bool {
    has_header(headers, "content-type")
}

const METHODS: [&str; 9] = [
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT",
];

/// The positional `[METHOD] URL [ITEM]...` words, taken apart.
#[derive(Debug, Default, PartialEq)]
pub struct Targets {
    pub method: Option<String>,
    pub url: Option<String>,
    pub items: Vec<Item>,
}

/// Split the positional words. `has_default_url` is true when a request file
/// supplies the URL, in which case the URL may be left out.
pub fn split_targets(words: &[String], has_default_url: bool) -> anyhow::Result<Targets> {
    let mut targets = Targets::default();
    let mut rest = words;

    // A method is recognised only where a URL can still follow (or isn't
    // needed), so `gurl get` still requests the host `get`
    if let Some(first) = rest.first() {
        let upper = first.to_uppercase();
        let is_method = METHODS.contains(&upper.as_str())
            && (*first == upper || *first == first.to_lowercase())
            && (rest.len() > 1 || has_default_url);
        if is_method {
            targets.method = Some(upper);
            rest = &rest[1..];
        }
    }

    if let Some(first) = rest.first() {
        let is_item =
            has_default_url && !looks_like_host_port(first) && items::parse_item(first)?.is_some();
        if !is_item {
            targets.url = Some(first.clone());
            rest = &rest[1..];
        }
    }

    for word in rest {
        match items::parse_item(word)? {
            Some(item) => targets.items.push(item),
            None => bail!(
                "`{word}` is not a request item. Expected Header:value, name=value, \
                 name:=json, name==value, or name@file (with --form)"
            ),
        }
    }
    Ok(targets)
}

/// `localhost:3000` or `api:8080/x`: a host and port, not a header.
/// `X-Count:3` is a header: hosts are written in lowercase, header names
/// usually aren't.
fn looks_like_host_port(word: &str) -> bool {
    word.split_once(':').is_some_and(|(host, rest)| {
        let port: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        !host.chars().any(|c| c.is_ascii_uppercase())
            && !port.is_empty()
            && matches!(rest[port.len()..].chars().next(), None | Some('/' | '?'))
    })
}

fn parse_query_flags(pairs: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    pairs
        .iter()
        .map(|pair| {
            let (name, value) = pair
                .split_once('=')
                .with_context(|| format!("--query `{pair}`: expected NAME=VALUE"))?;
            Ok((name.to_string(), value.to_string()))
        })
        .collect()
}

/// Merge CLI arguments with an optional request file into the request to send.
/// CLI values win over file values. Headers go in this order: file,
/// `--bearer`, `-H`, header items.
pub fn resolve(args: &Args, file: Option<&RequestFile>) -> anyhow::Result<CurlRequest> {
    let file_url = file.and_then(|r| r.url.clone());
    let targets = split_targets(&args.targets, file_url.is_some())?;
    let parts = items::build(&targets.items, args.form)?;

    let url = targets
        .url
        .or(file_url)
        .ok_or_else(|| anyhow::anyhow!("URL is required (provide as argument or in --file)"))?;
    let mut query = parse_query_flags(&args.query)?;
    query.extend(parts.query);
    let url = append_query(&expand_url(&url), &query);

    let mut headers: Vec<String> = file.map(|r| r.headers.to_vec()).unwrap_or_default();
    if let Some(token) = &args.bearer {
        headers.push(format!("Authorization: Bearer {token}"));
    }
    headers.extend(args.headers.iter().cloned());
    headers.extend(parts.headers);

    // Only an explicit `-d @path` reads a file, as with curl. A body from a
    // request file is always sent as written, even if it starts with '@'.
    let cli_body = args.data.as_ref().map(|data| match data.strip_prefix('@') {
        Some(path) => Body::File(path.to_string()),
        None => Body::Raw(data.clone()),
    });
    let body = match (cli_body, parts.body) {
        (Some(_), Some(_)) => bail!("Use either -d or body fields (name=value), not both"),
        (Some(body), None) | (None, Some(body)) => Some(body),
        (None, None) => file.and_then(|r| r.body.as_ref()).map(|b| {
            Body::Raw(
                b.as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| serde_json::to_string(b).unwrap_or_default()),
            )
        }),
    };

    let method = match (targets.method, &args.method) {
        (Some(word), Some(flag)) if !word.eq_ignore_ascii_case(flag) => {
            bail!("Method given twice: `{word}` and `-m {flag}`")
        }
        (Some(word), _) => Some(word),
        (None, flag) => flag.clone(),
    };
    // Like curl: sending data means POST unless told otherwise
    let method = method
        .or_else(|| file.and_then(|r| r.method.clone()))
        .unwrap_or_else(|| if body.is_some() { "POST" } else { "GET" }.to_string())
        .to_uppercase();

    // Auto-detect JSON and add Content-Type if missing
    let is_json = match &body {
        Some(Body::Raw(data)) => args.json || parts.json || looks_like_json(data),
        Some(Body::File(path)) => args.json || path.to_lowercase().ends_with(".json"),
        Some(Body::Multipart(_)) | None => false,
    };
    if is_json && !has_content_type_header(&headers) {
        headers.push("Content-Type: application/json".to_string());
    }
    if (args.json || parts.json) && !has_header(&headers, "accept") {
        headers.push("Accept: application/json, */*;q=0.5".to_string());
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
        crate::args::Cli::parse_from(std::iter::once("gurl").chain(argv.iter().copied())).args
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
        assert_eq!(req.body, Some(Body::Raw(r#"{"k":"v"}"#.into())));
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
        assert_eq!(req.body, Some(Body::Raw("from cli".into())));
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
        assert_eq!(req.body, Some(Body::Raw("plain text".into())));
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
    fn headers_object_keeps_order_and_accepts_scalars() {
        let hf: HeadersFormat = serde_json::from_str(
            r#"{"Z-First": "1", "A-Second": 2, "M-Third": true, "Accept": null}"#,
        )
        .unwrap();
        assert_eq!(
            hf.to_vec(),
            vec!["Z-First: 1", "A-Second: 2", "M-Third: true", "Accept:"]
        );
    }

    #[test]
    fn content_type_match_is_on_header_name_only() {
        assert!(!has_content_type_header(&[
            "Content-Type-Options: nosniff".to_string()
        ]));
        assert!(has_content_type_header(&[
            "  Content-Type : text/plain".to_string()
        ]));
    }

    #[test]
    fn resolve_data_implies_post() {
        let req = resolve(&args(&["-d", "x=1", "https://x.test"]), None).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.body, Some(Body::Raw("x=1".into())));
        assert!(req.headers.is_empty());
    }

    #[test]
    fn resolve_at_prefix_on_cli_reads_a_file() {
        let req = resolve(&args(&["-d", "@payload.json", "https://x.test"]), None).unwrap();
        assert_eq!(req.body, Some(Body::File("payload.json".into())));
        assert_eq!(req.headers, vec!["Content-Type: application/json"]);

        let req = resolve(&args(&["-d", "@-", "https://x.test"]), None).unwrap();
        assert_eq!(req.body, Some(Body::File("-".into())));
        assert!(req.headers.is_empty());
    }

    #[test]
    fn resolve_at_prefix_in_file_body_is_literal() {
        let rf = file(r#"{"url": "https://x.test", "body": "@/etc/passwd"}"#);
        let req = resolve(&args(&[]), Some(&rf)).unwrap();
        assert_eq!(req.body, Some(Body::Raw("@/etc/passwd".into())));
    }

    fn words(w: &[&str]) -> Vec<String> {
        w.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_targets_url_only() {
        let t = split_targets(&words(&["example.com"]), false).unwrap();
        assert_eq!(
            t,
            Targets {
                method: None,
                url: Some("example.com".into()),
                items: vec![]
            }
        );
    }

    #[test]
    fn split_targets_method_url_items() {
        let t = split_targets(&words(&["post", ":3000/u", "a=1", "X:y"]), false).unwrap();
        assert_eq!(t.method.as_deref(), Some("POST"));
        assert_eq!(t.url.as_deref(), Some(":3000/u"));
        assert_eq!(t.items.len(), 2);
    }

    #[test]
    fn split_targets_a_lone_method_word_is_a_host() {
        let t = split_targets(&words(&["get"]), false).unwrap();
        assert_eq!(t.method, None);
        assert_eq!(t.url.as_deref(), Some("get"));
    }

    #[test]
    fn split_targets_mixed_case_is_not_a_method() {
        let t = split_targets(&words(&["Delete", "x"]), false);
        assert!(t.is_err(), "`x` should be rejected as an item");
    }

    #[test]
    fn split_targets_with_a_file_url_can_skip_the_url() {
        let t = split_targets(&words(&["DELETE"]), true).unwrap();
        assert_eq!(t.method.as_deref(), Some("DELETE"));
        assert_eq!(t.url, None);

        let t = split_targets(&words(&["name=Jo", "X-A:1"]), true).unwrap();
        assert_eq!(t.url, None);
        assert_eq!(t.items.len(), 2);
    }

    #[test]
    fn split_targets_host_port_is_a_url_even_with_a_file() {
        for url in ["localhost:3000", "api:8080/v1", "db:5432?x=1"] {
            let t = split_targets(&words(&[url]), true).unwrap();
            assert_eq!(t.url.as_deref(), Some(url));
        }
        // ...but a capitalized header with a numeric value is an item
        let t = split_targets(&words(&["X-Count:3"]), true).unwrap();
        assert_eq!(t.url, None);
        assert_eq!(t.items.len(), 1);
    }

    #[test]
    fn split_targets_rejects_stray_words() {
        let err = split_targets(&words(&["example.com", "oops"]), false).unwrap_err();
        assert!(
            err.to_string().contains("`oops` is not a request item"),
            "{err}"
        );
    }

    #[test]
    fn resolve_items_build_a_json_post() {
        let req = resolve(
            &args(&[":3000/users", "name=Jo", "age:=30", "X-Key:abc", "v==2"]),
            None,
        )
        .unwrap();
        assert_eq!(
            req,
            CurlRequest {
                method: "POST".into(),
                url: "http://localhost:3000/users?v=2".into(),
                headers: vec![
                    "X-Key: abc".into(),
                    "Content-Type: application/json".into(),
                    "Accept: application/json, */*;q=0.5".into(),
                ],
                body: Some(Body::Raw(r#"{"name":"Jo","age":30}"#.into())),
                ..Default::default()
            }
        );
    }

    #[test]
    fn resolve_method_word_overrides_default() {
        let req = resolve(&args(&["PUT", ":3000/u/1", "name=Jo"]), None).unwrap();
        assert_eq!(req.method, "PUT");
    }

    #[test]
    fn resolve_method_given_twice_must_agree() {
        assert!(resolve(&args(&["-m", "put", "PUT", "x.test"]), None).is_ok());
        let err = resolve(&args(&["-m", "POST", "PUT", "x.test"]), None).unwrap_err();
        assert!(err.to_string().contains("Method given twice"), "{err}");
    }

    #[test]
    fn resolve_data_and_body_items_conflict() {
        let err = resolve(&args(&["-d", "x", "x.test", "a=1"]), None).unwrap_err();
        assert!(err.to_string().contains("not both"), "{err}");
        // Header and query items are fine alongside -d
        assert!(resolve(&args(&["-d", "x", "x.test", "A:1", "b==2"]), None).is_ok());
    }

    #[test]
    fn resolve_query_flags_and_items_are_encoded_in_order() {
        let req = resolve(
            &args(&["-q", "q=rust lang", "x.test/s?a=1#top", "page==2"]),
            None,
        )
        .unwrap();
        assert_eq!(req.url, "https://x.test/s?a=1&q=rust%20lang&page=2#top");
    }

    #[test]
    fn resolve_bearer_goes_after_file_headers_before_cli_headers() {
        let rf = file(r#"{"url": "https://x.test", "headers": ["X-File: 1"]}"#);
        let req = resolve(
            &args(&["--bearer", "tok", "-H", "X-Cli: 2", "X-Item:3"]),
            Some(&rf),
        )
        .unwrap();
        assert_eq!(
            req.headers,
            vec![
                "X-File: 1",
                "Authorization: Bearer tok",
                "X-Cli: 2",
                "X-Item: 3"
            ]
        );
    }

    #[test]
    fn resolve_json_flag_forces_json_headers() {
        let req = resolve(
            &args(&["--json", "-d", "not obviously json", "x.test"]),
            None,
        )
        .unwrap();
        assert_eq!(
            req.headers,
            vec![
                "Content-Type: application/json",
                "Accept: application/json, */*;q=0.5"
            ]
        );

        // Without a body only Accept is added
        let req = resolve(&args(&["--json", "x.test"]), None).unwrap();
        assert_eq!(req.headers, vec!["Accept: application/json, */*;q=0.5"]);
        assert_eq!(req.method, "GET");
    }

    #[test]
    fn resolve_json_flag_keeps_explicit_headers() {
        let req = resolve(
            &args(&[
                "--json",
                "-H",
                "accept: application/vnd.x+json",
                "x.test",
                "a=1",
            ]),
            None,
        )
        .unwrap();
        assert_eq!(
            req.headers,
            vec![
                "accept: application/vnd.x+json",
                "Content-Type: application/json"
            ]
        );
    }

    #[test]
    fn resolve_form_items() {
        let req = resolve(&args(&["--form", "x.test", "name=Jo Bloggs"]), None).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.body, Some(Body::Raw("name=Jo%20Bloggs".into())));
        assert!(req.headers.is_empty(), "curl sets the form Content-Type");
    }

    #[test]
    fn resolve_items_add_to_a_request_file() {
        let rf = file(r#"{"method": "PATCH", "url": "https://x.test/u/1", "headers": ["X-A: 1"]}"#);
        let req = resolve(&args(&["name=Jo", "v==2"]), Some(&rf)).unwrap();
        assert_eq!(req.method, "PATCH");
        assert_eq!(req.url, "https://x.test/u/1?v=2");
        assert_eq!(req.body, Some(Body::Raw(r#"{"name":"Jo"}"#.into())));
    }

    #[test]
    fn substitute_replaces_variables_everywhere_in_a_request_file() {
        let vars = Vars {
            cli: [
                ("HOST", "api.test"),
                ("TOKEN", "t0k"),
                ("NAME", r#"O"Brien"#),
                ("VERB", "put"),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
            ..Default::default()
        };
        let rf = file(
            r#"{
                "method": "{{VERB}}",
                "url": "https://{{HOST}}/users",
                "headers": {"Authorization": "Bearer {{TOKEN}}", "X-Count": 3},
                "body": {"name": "{{NAME}}", "tags": ["{{HOST}}"], "n": 1}
            }"#,
        )
        .substitute(&vars)
        .unwrap();

        assert_eq!(rf.method.as_deref(), Some("put"));
        assert_eq!(rf.url.as_deref(), Some("https://api.test/users"));
        assert_eq!(
            rf.headers.to_vec(),
            vec!["Authorization: Bearer t0k", "X-Count: 3"]
        );
        // The quote in NAME is escaped, not spliced into the JSON text
        assert_eq!(
            serde_json::to_string(&rf.body.unwrap()).unwrap(),
            r#"{"name":"O\"Brien","tags":["api.test"],"n":1}"#
        );
    }

    #[test]
    fn substitute_reports_undefined_variables() {
        let rf =
            file(r#"{"url": "https://x.test", "headers": ["Authorization: Bearer {{TOKEN}}"]}"#);
        let err = rf.substitute(&Vars::default()).unwrap_err();
        assert!(err.to_string().contains("`TOKEN`"), "{err}");
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
