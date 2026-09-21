//! End-to-end tests: the real `gurl` binary, the real `curl`, and a tiny local
//! HTTP server. assert_cmd captures stdout/stderr, so neither is a terminal.

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

const JSON_BODY: &str = r#"{"a":"x","b":1}"#;
const PRETTY_JSON_BODY: &str = "{\n  \"a\": \"x\",\n  \"b\": 1\n}\n";
const NOT_FOUND_BODY: &str = r#"{"error":"not found"}"#;
/// Not valid UTF-8, and contains NUL and newlines
const BINARY_BODY: &[u8] = &[0x89, b'P', b'N', b'G', 0x00, 0xff, 0xfe, b'\n', 0x80, b'\n'];

struct TestServer {
    addr: String,
}

impl TestServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                thread::spawn(move || handle(stream));
            }
        });
        TestServer { addr }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
}

fn handle(mut stream: TcpStream) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let content_length = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let request_body = buf[header_end..].to_vec();

    let path = head.split_whitespace().nth(1).unwrap_or("/");
    let (status, content_type, extra, body): (&str, &str, &str, Vec<u8>) = match path {
        "/json" => ("200 OK", "application/json", "", JSON_BODY.into()),
        "/missing" => (
            "404 Not Found",
            "application/json",
            "",
            NOT_FOUND_BODY.into(),
        ),
        "/binary" => ("200 OK", "application/octet-stream", "", BINARY_BODY.into()),
        "/redirect" => ("302 Found", "text/plain", "Location: /json\r\n", vec![]),
        "/echo" => ("200 OK", "text/plain", "", request_body),
        _ => ("500 Internal Server Error", "text/plain", "", vec![]),
    };

    let response_head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response_head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn gurl() -> Command {
    let mut cmd = Command::cargo_bin("gurl").unwrap();
    cmd.env("NO_COLOR", "1").env_remove("CLICOLOR_FORCE");
    cmd
}

fn no_meta_leak() -> impl Predicate<str> {
    predicate::str::contains("__GURL_META__").not()
}

#[test]
fn stdout_is_exactly_the_response_body() {
    let server = TestServer::start();
    gurl()
        .arg(server.url("/json"))
        .assert()
        .success()
        .stdout(JSON_BODY)
        .stderr(predicate::str::contains("GET"))
        .stderr(predicate::str::contains("200"))
        .stderr(predicate::str::contains("Response received"))
        .stderr(no_meta_leak());
}

#[test]
fn binary_response_round_trips_byte_for_byte() {
    let server = TestServer::start();
    gurl()
        .arg(server.url("/binary"))
        .assert()
        .success()
        .stdout(BINARY_BODY);
}

#[test]
fn pretty_formats_json() {
    let server = TestServer::start();
    gurl()
        .args(["--pretty", &server.url("/json")])
        .assert()
        .success()
        .stdout(PRETTY_JSON_BODY);
}

#[test]
fn pretty_leaves_non_json_untouched() {
    let server = TestServer::start();
    gurl()
        .args(["--pretty", &server.url("/binary")])
        .assert()
        .success()
        .stdout(BINARY_BODY);
}

#[test]
fn pretty_with_include_shows_headers_then_formatted_body() {
    let server = TestServer::start();
    gurl()
        .args(["--pretty", "-i", &server.url("/json")])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("HTTP/1.1 200 OK\n"))
        .stdout(predicate::str::contains("Content-Type: application/json\n"))
        .stdout(predicate::str::ends_with(PRETTY_JSON_BODY));
}

#[test]
fn error_status_is_reported_on_stderr() {
    let server = TestServer::start();
    gurl()
        .arg(server.url("/missing"))
        .assert()
        .success()
        .stdout(NOT_FOUND_BODY)
        .stderr(predicate::str::contains("404"))
        .stderr(predicate::str::contains("Error response received"));
}

#[test]
fn verbose_trace_reaches_stderr() {
    let server = TestServer::start();
    gurl()
        .args(["-v", &server.url("/json")])
        .assert()
        .success()
        .stdout(JSON_BODY)
        .stderr(predicate::str::contains("Command:"))
        .stderr(predicate::str::contains("> GET /json"))
        .stderr(predicate::str::contains("< HTTP/1.1 200 OK"))
        .stderr(no_meta_leak());
}

#[test]
fn verbose_trace_survives_pretty_mode() {
    let server = TestServer::start();
    gurl()
        .args(["-v", "--pretty", &server.url("/json")])
        .assert()
        .success()
        .stdout(PRETTY_JSON_BODY)
        .stderr(predicate::str::contains("< HTTP/1.1 200 OK"));
}

#[test]
fn silent_prints_only_the_response() {
    let server = TestServer::start();
    gurl()
        .args(["-s", &server.url("/json")])
        .assert()
        .success()
        .stdout(JSON_BODY)
        .stderr("");
}

#[test]
fn follows_redirects_and_reports_final_status() {
    let server = TestServer::start();
    gurl()
        .args(["-L", &server.url("/redirect")])
        .assert()
        .success()
        .stdout(JSON_BODY)
        .stderr(predicate::str::contains("200"));
}

#[test]
fn sends_body_from_the_command_line() {
    let server = TestServer::start();
    gurl()
        .args(["-m", "POST", "-d", r#"{"name":"Jo"}"#, &server.url("/echo")])
        .assert()
        .success()
        .stdout(r#"{"name":"Jo"}"#);
}

#[test]
fn sends_request_from_file() {
    let server = TestServer::start();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(
        file,
        r#"{{"method": "POST", "url": "{}", "body": {{"k": "v"}}}}"#,
        server.url("/echo")
    )
    .unwrap();

    gurl()
        .arg("--file")
        .arg(file.path())
        .assert()
        .success()
        .stdout(r#"{"k":"v"}"#)
        .stderr(predicate::str::contains("Loading from"));
}

#[test]
fn output_file_receives_the_body() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("body.json");

    gurl()
        .arg("-o")
        .arg(&out)
        .arg(server.url("/json"))
        .assert()
        .success()
        .stdout("");

    assert_eq!(std::fs::read_to_string(out).unwrap(), JSON_BODY);
}

#[test]
fn connection_failure_propagates_curls_exit_code() {
    // Grab a free port, then close it again so nothing is listening
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    gurl()
        .arg(format!("http://127.0.0.1:{port}/"))
        .assert()
        .code(7)
        .stdout("")
        .stderr(predicate::str::contains("curl: (7)"))
        .stderr(predicate::str::contains("Failed"))
        .stderr(predicate::str::contains("curl exit code 7"))
        .stderr(no_meta_leak());
}

#[test]
fn missing_url_is_a_usage_error() {
    gurl()
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL is required"));
}
