use crate::format;
use colored::*;
use serde_json::Value;
use std::io;
use std::process::Command;
use std::time::Instant;

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
    pub pretty: bool,
    pub extra_args: Vec<String>,
}

pub fn build_curl_args(req: &CurlRequest) -> Vec<String> {
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

    // Always suppress progress meter since we capture output
    args.push("-s".into());

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

    for arg in &req.extra_args {
        args.push(arg.clone());
    }

    // Write format to extract HTTP status code
    args.push("-w".into());
    args.push("\n%{http_code}".into());

    args.push(req.url.clone());

    args
}

pub fn execute_request(req: &CurlRequest) -> anyhow::Result<()> {
    let curl_args = build_curl_args(req);

    if req.verbose {
        eprintln!(
            "{} curl {}",
            "Command:".dimmed(),
            curl_args.join(" ").dimmed()
        );
        eprintln!();
    }

    let start = Instant::now();

    let mut cmd = Command::new("curl");
    for arg in &curl_args {
        cmd.arg(arg);
    }

    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            anyhow::bail!("curl is not installed or not in PATH. Please install curl to use gurl.");
        }
        Err(e) => return Err(e.into()),
    };

    let elapsed = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse HTTP status code from the last line (added by -w "\n%{http_code}")
    let (body, status_code) = parse_status_code(&stdout);

    if output.status.success() {
        if req.output.is_none() {
            if req.pretty {
                if let Ok(json) = serde_json::from_str::<Value>(body) {
                    format::print_json_colored(&json, 0);
                    println!();
                } else {
                    print!("{body}");
                }
            } else {
                print!("{body}");
            }
        }

        println!();
        if let Some(code) = status_code {
            format::print_status_code(code);
        }
        println!(
            "{} {} in {:.2?}",
            "✓".green().bold(),
            "Response received".green(),
            elapsed
        );
    } else {
        // Try to pretty-print error response body if present
        if !body.is_empty() {
            if req.pretty {
                if let Ok(json) = serde_json::from_str::<Value>(body) {
                    format::print_json_colored(&json, 0);
                    println!();
                } else {
                    eprint!("{body}");
                }
            } else {
                eprint!("{body}");
            }
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            eprint!("{stderr}");
        }
        println!();
        if let Some(code) = status_code {
            format::print_status_code(code);
        }
        format::print_error(output.status.code());
        eprintln!("{} in {:.2?}", "Failed".red(), elapsed);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

fn parse_status_code(stdout: &str) -> (&str, Option<u16>) {
    if let Some(pos) = stdout.rfind('\n') {
        let potential_code = &stdout[pos + 1..];
        if let Ok(code) = potential_code.trim().parse::<u16>()
            && (100..=599).contains(&code)
        {
            return (&stdout[..pos], Some(code));
        }
    }
    (stdout, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_curl_args_basic_get() {
        let req = CurlRequest {
            method: "GET".into(),
            url: "https://example.com".into(),
            headers: vec![],
            body: None,
            verbose: false,
            include: false,
            location: false,
            timeout: None,
            output: None,
            user: None,
            pretty: false,
            extra_args: vec![],
        };
        let args = build_curl_args(&req);
        assert!(args.contains(&"-X".to_string()));
        assert!(args.contains(&"GET".to_string()));
        assert!(args.contains(&"-s".to_string()));
        assert!(args.contains(&"-w".to_string()));
        assert_eq!(args.last().unwrap(), "https://example.com");
    }

    #[test]
    fn build_curl_args_post_with_body_and_headers() {
        let req = CurlRequest {
            method: "POST".into(),
            url: "https://example.com/api".into(),
            headers: vec!["Content-Type: application/json".into()],
            body: Some(r#"{"key":"value"}"#.into()),
            verbose: false,
            include: false,
            location: false,
            timeout: None,
            output: None,
            user: None,
            pretty: true,
            extra_args: vec![],
        };
        let args = build_curl_args(&req);
        assert!(args.contains(&"-d".to_string()));
        assert!(args.contains(&r#"{"key":"value"}"#.to_string()));
        assert!(args.contains(&"-H".to_string()));
        assert!(args.contains(&"Content-Type: application/json".to_string()));
    }

    #[test]
    fn build_curl_args_with_all_flags() {
        let req = CurlRequest {
            method: "GET".into(),
            url: "https://example.com".into(),
            headers: vec![],
            body: None,
            verbose: true,
            include: true,
            location: true,
            timeout: Some(30),
            output: Some("out.json".into()),
            user: Some("admin:pass".into()),
            pretty: false,
            extra_args: vec!["--compressed".into()],
        };
        let args = build_curl_args(&req);
        assert!(args.contains(&"-v".to_string()));
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"-L".to_string()));
        assert!(args.contains(&"--max-time".to_string()));
        assert!(args.contains(&"30".to_string()));
        assert!(args.contains(&"-o".to_string()));
        assert!(args.contains(&"out.json".to_string()));
        assert!(args.contains(&"-u".to_string()));
        assert!(args.contains(&"admin:pass".to_string()));
        assert!(args.contains(&"--compressed".to_string()));
    }

    #[test]
    fn build_curl_args_extra_args_before_url() {
        let req = CurlRequest {
            method: "GET".into(),
            url: "https://example.com".into(),
            headers: vec![],
            body: None,
            verbose: false,
            include: false,
            location: false,
            timeout: None,
            output: None,
            user: None,
            pretty: false,
            extra_args: vec!["-k".into(), "--compressed".into()],
        };
        let args = build_curl_args(&req);
        let url_pos = args
            .iter()
            .position(|a| a == "https://example.com")
            .unwrap();
        let k_pos = args.iter().position(|a| a == "-k").unwrap();
        assert!(k_pos < url_pos);
    }

    #[test]
    fn parse_status_code_200() {
        let (body, code) = parse_status_code("response body\n200");
        assert_eq!(body, "response body");
        assert_eq!(code, Some(200));
    }

    #[test]
    fn parse_status_code_404() {
        let (body, code) = parse_status_code("{\"error\":\"not found\"}\n404");
        assert_eq!(body, "{\"error\":\"not found\"}");
        assert_eq!(code, Some(404));
    }

    #[test]
    fn parse_status_code_no_code() {
        let (body, code) = parse_status_code("just some text");
        assert_eq!(body, "just some text");
        assert_eq!(code, None);
    }

    #[test]
    fn parse_status_code_empty_body() {
        let (body, code) = parse_status_code("\n200");
        assert_eq!(body, "");
        assert_eq!(code, Some(200));
    }

    #[test]
    fn parse_status_code_multiline_body() {
        let (body, code) = parse_status_code("line1\nline2\nline3\n201");
        assert_eq!(body, "line1\nline2\nline3");
        assert_eq!(code, Some(201));
    }
}
