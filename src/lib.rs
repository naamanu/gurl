pub mod args;
pub mod curl;
pub mod format;

pub fn expand_url(url: &str) -> String {
    if url.starts_with(':') {
        format!("http://localhost{url}")
    } else if !url.starts_with("http://") && !url.starts_with("https://") {
        format!("https://{url}")
    } else {
        url.to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_url_localhost_shorthand() {
        assert_eq!(expand_url(":8080/health"), "http://localhost:8080/health");
        assert_eq!(expand_url(":3000"), "http://localhost:3000");
    }

    #[test]
    fn expand_url_bare_domain() {
        assert_eq!(expand_url("example.com"), "https://example.com");
        assert_eq!(
            expand_url("api.example.com/v1"),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn expand_url_existing_scheme() {
        assert_eq!(expand_url("http://example.com"), "http://example.com");
        assert_eq!(expand_url("https://example.com"), "https://example.com");
    }

    #[test]
    fn looks_like_json_object() {
        assert!(looks_like_json(r#"{"key": "value"}"#));
    }

    #[test]
    fn looks_like_json_array() {
        assert!(looks_like_json(r#"[1, 2, 3]"#));
    }

    #[test]
    fn looks_like_json_with_whitespace() {
        assert!(looks_like_json("  { \"key\": 1 }  "));
        assert!(looks_like_json("\n[]\n"));
    }

    #[test]
    fn looks_like_json_empty_string() {
        assert!(!looks_like_json(""));
    }

    #[test]
    fn looks_like_json_non_json() {
        assert!(!looks_like_json("hello world"));
        assert!(!looks_like_json("{incomplete"));
        assert!(!looks_like_json("[incomplete"));
    }

    #[test]
    fn has_content_type_header_present() {
        let headers = vec!["Content-Type: application/json".to_string()];
        assert!(has_content_type_header(&headers));
    }

    #[test]
    fn has_content_type_header_case_insensitive() {
        let headers = vec!["content-type: text/plain".to_string()];
        assert!(has_content_type_header(&headers));
    }

    #[test]
    fn has_content_type_header_absent() {
        let headers = vec!["Authorization: Bearer token".to_string()];
        assert!(!has_content_type_header(&headers));
    }

    #[test]
    fn has_content_type_header_empty() {
        assert!(!has_content_type_header(&[]));
    }
}
