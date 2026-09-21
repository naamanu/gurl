//! Collections: several named requests in one file, run with `gurl run`.
//!
//! ```json
//! {
//!   "vars": { "base": "http://localhost:3000" },
//!   "defaults": { "headers": { "Authorization": "Bearer {{TOKEN}}" } },
//!   "requests": {
//!     "health": { "url": "{{base}}/health" },
//!     "create": { "method": "POST", "url": "{{base}}/users", "body": { "name": "Jo" } }
//!   }
//! }
//! ```

use crate::request::{HeadersFormat, RequestFile};
use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    /// Lowest-precedence values for `{{NAME}}` variables
    #[serde(default)]
    pub vars: HashMap<String, String>,
    #[serde(default)]
    pub defaults: Defaults,
    /// In file order. Parsed one at a time, so a mistake in one request
    /// doesn't stop the others from running.
    pub requests: Map<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// Sent before each request's own headers
    #[serde(default)]
    pub headers: HeadersFormat,
}

/// One line of `gurl run <file>`
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub name: String,
    pub method: String,
    pub url: String,
    pub description: Option<String>,
}

pub fn load_collection(path: &Path) -> anyhow::Result<Collection> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read collection: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse collection: {}", path.display()))
}

impl Collection {
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.requests.keys().map(String::as_str)
    }

    /// The named request, with the collection's default headers in front
    /// of its own. Variables are left for the caller to substitute.
    pub fn request(&self, name: &str) -> anyhow::Result<RequestFile> {
        let Some(raw) = self.requests.get(name) else {
            let names: Vec<_> = self.names().collect();
            if names.is_empty() {
                bail!("No request named `{name}`: the collection has no requests");
            }
            bail!("No request named `{name}`. Available: {}", names.join(", "));
        };
        let mut request: RequestFile = serde_json::from_value(raw.clone())
            .with_context(|| format!("Request `{name}` is not a valid request"))?;
        request.headers = merge_headers(&self.defaults.headers, &request.headers);
        Ok(request)
    }

    /// Every request, for listing. A request that doesn't parse is still
    /// listed, marked as invalid.
    pub fn summaries(&self) -> Vec<Summary> {
        self.requests
            .iter()
            .map(
                |(name, raw)| match serde_json::from_value::<RequestFile>(raw.clone()) {
                    Ok(request) => Summary {
                        name: name.clone(),
                        method: request.method.unwrap_or_else(|| {
                            if request.body.is_some() {
                                "POST"
                            } else {
                                "GET"
                            }
                            .to_string()
                        }),
                        url: request.url.unwrap_or_default(),
                        description: request.description,
                    },
                    Err(e) => Summary {
                        name: name.clone(),
                        method: "?".into(),
                        url: String::new(),
                        description: Some(format!("invalid: {e}")),
                    },
                },
            )
            .collect()
    }
}

fn merge_headers(defaults: &HeadersFormat, own: &HeadersFormat) -> HeadersFormat {
    match (defaults, own) {
        (HeadersFormat::None, _) => own.clone(),
        (_, HeadersFormat::None) => defaults.clone(),
        // Both objects: a request can override a default header by name,
        // or drop it with null
        (HeadersFormat::Object(d), HeadersFormat::Object(o)) => {
            let mut merged = Map::new();
            for (name, value) in d {
                if !o.keys().any(|k| k.eq_ignore_ascii_case(name)) {
                    merged.insert(name.clone(), value.clone());
                }
            }
            merged.extend(o.clone());
            HeadersFormat::Object(merged)
        }
        _ => {
            let mut merged = defaults.to_vec();
            merged.extend(own.to_vec());
            HeadersFormat::Array(merged)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(json: &str) -> Collection {
        serde_json::from_str(json).unwrap()
    }

    const SAMPLE: &str = r#"{
        "vars": {"base": "http://localhost:3000"},
        "defaults": {"headers": {"Authorization": "Bearer {{TOKEN}}", "X-Client": "gurl"}},
        "requests": {
            "health": {"url": "{{base}}/health", "description": "Is it up?"},
            "create": {
                "method": "POST",
                "url": "{{base}}/users",
                "headers": {"X-Client": "other", "Authorization": null},
                "body": {"name": "Jo"}
            },
            "upload": {"url": "{{base}}/files", "body": "raw", "headers": ["X-Kind: raw"]},
            "broken": {"url": 42}
        }
    }"#;

    #[test]
    fn lists_requests_in_file_order() {
        let c = collection(SAMPLE);
        assert_eq!(
            c.names().collect::<Vec<_>>(),
            vec!["health", "create", "upload", "broken"]
        );
        let summaries = c.summaries();
        assert_eq!(
            summaries[0],
            Summary {
                name: "health".into(),
                method: "GET".into(),
                url: "{{base}}/health".into(),
                description: Some("Is it up?".into()),
            }
        );
        assert_eq!(summaries[2].method, "POST", "a body implies POST");
        assert_eq!(summaries[3].method, "?");
        assert!(
            summaries[3]
                .description
                .as_deref()
                .unwrap()
                .starts_with("invalid:")
        );
    }

    #[test]
    fn default_headers_come_first() {
        let request = collection(SAMPLE).request("health").unwrap();
        assert_eq!(
            request.headers.to_vec(),
            vec!["Authorization: Bearer {{TOKEN}}", "X-Client: gurl"]
        );
        assert_eq!(request.url.as_deref(), Some("{{base}}/health"));
    }

    #[test]
    fn a_request_can_override_or_drop_default_headers() {
        let request = collection(SAMPLE).request("create").unwrap();
        assert_eq!(
            request.headers.to_vec(),
            vec!["X-Client: other", "Authorization:"]
        );
    }

    #[test]
    fn array_headers_are_appended_to_defaults() {
        let request = collection(SAMPLE).request("upload").unwrap();
        assert_eq!(
            request.headers.to_vec(),
            vec![
                "Authorization: Bearer {{TOKEN}}",
                "X-Client: gurl",
                "X-Kind: raw"
            ]
        );
    }

    #[test]
    fn unknown_request_lists_the_available_ones() {
        let err = collection(SAMPLE).request("nope").unwrap_err();
        assert_eq!(
            err.to_string(),
            "No request named `nope`. Available: health, create, upload, broken"
        );
    }

    #[test]
    fn invalid_request_is_reported_by_name() {
        let err = collection(SAMPLE).request("broken").unwrap_err();
        assert!(err.to_string().contains("`broken`"), "{err}");
    }

    #[test]
    fn unknown_top_level_keys_are_rejected() {
        let err = serde_json::from_str::<Collection>(r#"{"requests": {}, "var": {}}"#).unwrap_err();
        assert!(err.to_string().contains("unknown field `var`"), "{err}");
    }

    #[test]
    fn minimal_collection() {
        let c = collection(r#"{"requests": {"a": {"url": "x.test"}}}"#);
        assert!(c.vars.is_empty());
        assert!(c.request("a").unwrap().headers.to_vec().is_empty());
    }
}
