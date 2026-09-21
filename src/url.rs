/// Complete a URL typed in shorthand:
/// - `:3000/api` → `http://localhost:3000/api`
/// - local hosts (`localhost`, `127.0.0.1`, `[::1]`, `*.localhost`) → `http://`
/// - any other bare host → `https://`
/// - anything that already has a scheme (`http://`, `ws://`, `file://`, …) is kept
pub fn expand_url(url: &str) -> String {
    if url.starts_with(':') {
        format!("http://localhost{url}")
    } else if has_scheme(url) {
        url.to_string()
    } else if is_local_host(host_of(url)) {
        format!("http://{url}")
    } else {
        format!("https://{url}")
    }
}

/// RFC 3986: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` followed by `://`
fn has_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}

/// The host part of a scheme-less URL, without port or userinfo
fn host_of(url: &str) -> &str {
    let authority = url.split(['/', '?', '#']).next().unwrap_or(url);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if host_port.starts_with('[') {
        // IPv6 literal: the port, if any, comes after the closing bracket
        return host_port
            .split_once(']')
            .map_or(host_port, |(host, _)| &host_port[..host.len() + 1]);
    }
    host_port.split(':').next().unwrap_or(host_port)
}

fn is_local_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "[::1]"
        || host.split('.').next() == Some("127") && host.parse::<std::net::Ipv4Addr>().is_ok()
}

/// Percent-encode everything but RFC 3986 unreserved characters.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Append encoded `name=value` pairs to the URL's query string, keeping any
/// query it already has and any `#fragment` at the end.
pub fn append_query(url: &str, pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return url.to_string();
    }
    let (base, fragment) = match url.find('#') {
        Some(pos) => url.split_at(pos),
        None => (url, ""),
    };
    let encoded = pairs
        .iter()
        .map(|(name, value)| format!("{}={}", percent_encode(name), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    let sep = if !base.contains('?') {
        "?"
    } else if base.ends_with('?') || base.ends_with('&') {
        ""
    } else {
        "&"
    };
    format!("{base}{sep}{encoded}{fragment}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
        p.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn percent_encode_keeps_only_unreserved() {
        assert_eq!(percent_encode("abc-._~XYZ09"), "abc-._~XYZ09");
        assert_eq!(percent_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(percent_encode(""), "");
    }

    #[test]
    fn append_query_to_bare_url() {
        assert_eq!(
            append_query(
                "https://x.test/s",
                &pairs(&[("q", "rust lang"), ("page", "2")])
            ),
            "https://x.test/s?q=rust%20lang&page=2"
        );
    }

    #[test]
    fn append_query_keeps_existing_query_and_fragment() {
        assert_eq!(
            append_query("https://x.test/s?a=1#top", &pairs(&[("b", "2")])),
            "https://x.test/s?a=1&b=2#top"
        );
        assert_eq!(
            append_query("https://x.test/s?", &pairs(&[("b", "2")])),
            "https://x.test/s?b=2"
        );
    }

    #[test]
    fn append_query_with_nothing_is_a_no_op() {
        assert_eq!(
            append_query("https://x.test/?a=1", &[]),
            "https://x.test/?a=1"
        );
    }

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
        assert_eq!(
            expand_url("user:pw@example.com:8443/x"),
            "https://user:pw@example.com:8443/x"
        );
    }

    #[test]
    fn expand_url_local_hosts_use_http() {
        assert_eq!(
            expand_url("localhost:3000/api"),
            "http://localhost:3000/api"
        );
        assert_eq!(expand_url("LOCALHOST"), "http://LOCALHOST");
        assert_eq!(expand_url("127.0.0.1:8080"), "http://127.0.0.1:8080");
        assert_eq!(expand_url("127.1.2.3/x"), "http://127.1.2.3/x");
        assert_eq!(expand_url("[::1]:8080/x"), "http://[::1]:8080/x");
        assert_eq!(expand_url("app.localhost/x"), "http://app.localhost/x");
    }

    #[test]
    fn expand_url_lookalikes_are_not_local() {
        assert_eq!(
            expand_url("localhost.example.com"),
            "https://localhost.example.com"
        );
        assert_eq!(expand_url("127.example.com"), "https://127.example.com");
        assert_eq!(expand_url("mylocalhost"), "https://mylocalhost");
    }

    #[test]
    fn expand_url_existing_scheme() {
        assert_eq!(expand_url("http://example.com"), "http://example.com");
        assert_eq!(expand_url("https://example.com"), "https://example.com");
        assert_eq!(expand_url("HTTPS://example.com"), "HTTPS://example.com");
        assert_eq!(expand_url("ws://example.com/sock"), "ws://example.com/sock");
        assert_eq!(expand_url("file:///etc/hosts"), "file:///etc/hosts");
        assert_eq!(expand_url("svn+ssh://host/repo"), "svn+ssh://host/repo");
    }

    #[test]
    fn expand_url_scheme_like_text_in_path_is_not_a_scheme() {
        assert_eq!(
            expand_url("example.com/redirect?to=http://x"),
            "https://example.com/redirect?to=http://x"
        );
    }
}
