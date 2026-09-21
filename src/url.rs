pub fn expand_url(url: &str) -> String {
    if url.starts_with(':') {
        format!("http://localhost{url}")
    } else if !url.starts_with("http://") && !url.starts_with("https://") {
        format!("https://{url}")
    } else {
        url.to_string()
    }
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
}
