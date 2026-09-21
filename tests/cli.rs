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
    let route = path.split('?').next().unwrap_or(path);
    let (status, content_type, extra, body): (&str, &str, &str, Vec<u8>) = match route {
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
        "/echo-path" => ("200 OK", "text/plain", "", path.as_bytes().to_vec()),
        "/echo-headers" => ("200 OK", "text/plain", "", head.as_bytes().to_vec()),
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
        .stderr(predicate::str::is_match(r"✓ HTTP 200 OK · [0-9.]+m?s · 15 B\n$").unwrap())
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
        .stderr(predicate::str::contains("✗ HTTP 404 Not Found"));
}

#[test]
fn fail_exits_22_on_http_errors_and_still_prints_the_body() {
    let server = TestServer::start();
    gurl()
        .args(["--fail", &server.url("/missing")])
        .assert()
        .code(22)
        .stdout(NOT_FOUND_BODY);
}

#[test]
fn fail_exits_zero_on_success() {
    let server = TestServer::start();
    gurl()
        .args(["--fail", "-s", &server.url("/json")])
        .assert()
        .success();
}

#[test]
fn raw_is_accepted_and_leaves_json_alone() {
    let server = TestServer::start();
    gurl()
        .args(["--raw", &server.url("/json")])
        .assert()
        .success()
        .stdout(JSON_BODY);
}

#[test]
fn completions_are_generated() {
    for shell in ["bash", "zsh", "fish"] {
        gurl()
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::contains("gurl"))
            .stdout(predicate::str::contains("dry-run"));
    }
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

#[test]
fn head_returns_headers_without_waiting_for_a_body() {
    let server = TestServer::start();
    gurl()
        .args(["-m", "HEAD", &server.url("/json")])
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .success()
        .stdout(predicate::str::starts_with("HTTP/1.1 200 OK"));
}

#[test]
fn at_prefix_sends_file_contents_byte_for_byte() {
    let server = TestServer::start();
    let mut body = tempfile::NamedTempFile::new().unwrap();
    body.write_all(b"line1\nline2\n").unwrap();

    gurl()
        .arg("-d")
        .arg(format!("@{}", body.path().display()))
        .arg(server.url("/echo"))
        .assert()
        .success()
        .stdout("line1\nline2\n");
}

#[test]
fn at_prefix_reads_stdin() {
    let server = TestServer::start();
    gurl()
        .args(["-d", "@-", &server.url("/echo")])
        .write_stdin("from stdin")
        .assert()
        .success()
        .stdout("from stdin");
}

#[test]
fn string_body_in_request_file_is_never_read_as_a_file() {
    let server = TestServer::start();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(
        file,
        r#"{{"url": "{}", "body": "@/etc/hosts"}}"#,
        server.url("/echo")
    )
    .unwrap();

    gurl()
        .arg("-f")
        .arg(file.path())
        .assert()
        .success()
        .stdout("@/etc/hosts");
}

#[test]
fn pretty_keeps_the_servers_key_order() {
    let server = TestServer::start();
    gurl()
        .args(["--pretty", "-d", r#"{"z":1,"a":2}"#, &server.url("/echo")])
        .assert()
        .success()
        .stdout("{\n  \"z\": 1,\n  \"a\": 2\n}\n");
}

#[test]
fn dry_run_prints_a_quoted_command_and_sends_nothing() {
    gurl()
        .args([
            "--dry-run",
            "-m",
            "PUT",
            "-H",
            "Authorization: Bearer s3cret",
            "-d",
            r#"{"name":"O'Brien"}"#,
            ":1/users/1",
        ])
        .assert()
        .success()
        .stdout(
            r#"curl -X PUT -H 'Authorization: ***' -H 'Content-Type: application/json' --data-raw '{"name":"O'\''Brien"}' http://localhost:1/users/1
"#,
        )
        .stderr("");
}

#[test]
fn show_secrets_reveals_credentials() {
    gurl()
        .args(["--dry-run", "--show-secrets", "-u", "me:pw", ":1/"])
        .assert()
        .success()
        .stdout("curl -u me:pw http://localhost:1/\n");
}

#[test]
fn verbose_masks_credentials_in_gurls_own_output() {
    let server = TestServer::start();
    gurl()
        .args([
            "-v",
            "-H",
            "Authorization: Bearer s3cret",
            &server.url("/json"),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Command: curl -H 'Authorization: ***'",
        ))
        .stderr(predicate::str::contains("    Authorization: ***"));
}

#[test]
fn closed_stdout_is_not_an_error() {
    use std::process::{Command as StdCommand, Stdio};

    let server = TestServer::start();
    let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("gurl"))
        .args(["--pretty", "-s", &server.url("/json")])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(!stderr.contains("Broken pipe"), "{stderr}");
}

#[test]
fn items_send_a_json_body_headers_and_query() {
    let server = TestServer::start();
    gurl()
        .args([
            "-s",
            &server.url("/echo"),
            "name=Jo",
            "age:=30",
            "tags:=[\"a\"]",
        ])
        .assert()
        .success()
        .stdout(r#"{"name":"Jo","age":30,"tags":["a"]}"#);

    gurl()
        .args([
            "-s",
            &server.url("/echo-headers"),
            "X-Api-Key:abc",
            "name=Jo",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("POST /echo-headers "))
        .stdout(predicate::str::contains("\r\nX-Api-Key: abc\r\n"))
        .stdout(predicate::str::contains(
            "\r\nContent-Type: application/json\r\n",
        ))
        .stdout(predicate::str::contains(
            "\r\nAccept: application/json, */*;q=0.5\r\n",
        ));

    gurl()
        .args([
            "-s",
            "-q",
            "sort=-created at",
            &server.url("/echo-path?a=1"),
            "page==2",
        ])
        .assert()
        .success()
        .stdout("/echo-path?a=1&sort=-created%20at&page=2");
}

#[test]
fn method_word_comes_before_the_url() {
    let server = TestServer::start();
    gurl()
        .args(["-s", "DELETE", &server.url("/echo-headers")])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("DELETE /echo-headers "));
}

#[test]
fn form_items_are_urlencoded() {
    let server = TestServer::start();
    gurl()
        .args([
            "-s",
            "--form",
            &server.url("/echo"),
            "name=Jo Bloggs",
            "x=a&b",
        ])
        .assert()
        .success()
        .stdout("name=Jo%20Bloggs&x=a%26b");
}

#[test]
fn form_with_a_file_is_a_multipart_upload() {
    let server = TestServer::start();
    let mut upload = tempfile::NamedTempFile::new().unwrap();
    upload.write_all(b"file contents here").unwrap();

    gurl()
        .args(["-s", "--form", &server.url("/echo"), "title=Hi"])
        .arg(format!("doc@{}", upload.path().display()))
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Content-Disposition: form-data; name=\"title\"\r\n\r\nHi\r\n",
        ))
        .stdout(predicate::str::contains("name=\"doc\"; filename="))
        .stdout(predicate::str::contains("file contents here"));
}

#[test]
fn stray_word_is_a_clear_error() {
    gurl()
        .args(["--dry-run", "example.com", "oops"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("`oops` is not a request item"));
}

#[test]
fn bearer_is_sent_and_masked() {
    let server = TestServer::start();
    gurl()
        .args(["-s", "--bearer", "s3cret", &server.url("/echo-headers")])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\r\nAuthorization: Bearer s3cret\r\n",
        ));

    gurl()
        .args(["--dry-run", "--bearer", "s3cret", ":1/"])
        .assert()
        .success()
        .stdout("curl -H 'Authorization: ***' http://localhost:1/\n");
}

fn request_file(contents: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    file
}

#[test]
fn request_file_variables_come_from_var_env_and_env_file() {
    let server = TestServer::start();
    let file = request_file(&format!(
        r#"{{"url": "{}", "body": {{"a": "{{{{A}}}}", "b": "{{{{B}}}}", "c": "{{{{C}}}}"}}}}"#,
        server.url("/echo")
    ));
    let env_file = request_file("A=from-env-file\nB=from-env-file\nC=from-env-file\n");

    gurl()
        .args(["-s", "--var", "A=from-var", "--env-file"])
        .arg(env_file.path())
        .arg("-f")
        .arg(file.path())
        .env("A", "from-env")
        .env("B", "from-env")
        .env_remove("C")
        .assert()
        .success()
        .stdout(r#"{"a":"from-var","b":"from-env","c":"from-env-file"}"#);
}

#[test]
fn undefined_variable_fails_before_sending() {
    let file = request_file(
        r#"{"url": "http://localhost:1/", "headers": ["Authorization: Bearer {{GURL_TEST_UNSET}}"]}"#,
    );
    gurl()
        .arg("-f")
        .arg(file.path())
        .env_remove("GURL_TEST_UNSET")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Undefined variable `GURL_TEST_UNSET`",
        ))
        .stderr(predicate::str::contains("In request file"));
}

#[test]
fn items_extend_a_request_file() {
    let server = TestServer::start();
    let file = request_file(&format!(
        r#"{{"method": "PUT", "url": "{}"}}"#,
        server.url("/echo-headers")
    ));
    gurl()
        .args(["-s", "-f"])
        .arg(file.path())
        .args(["X-Extra:1", "name=Jo"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("PUT /echo-headers "))
        .stdout(predicate::str::contains("\r\nX-Extra: 1\r\n"));
}

fn collection_file(server: &TestServer) -> tempfile::NamedTempFile {
    request_file(&format!(
        r#"{{
            "vars": {{"base": "{}", "who": "default"}},
            "defaults": {{"headers": {{"X-Default": "1"}}}},
            "requests": {{
                "hello": {{"description": "Say hi", "url": "{{{{base}}}}/echo-headers"}},
                "create": {{"method": "PUT", "url": "{{{{base}}}}/echo", "body": {{"who": "{{{{who}}}}"}}}},
                "needs-token": {{"url": "{{{{base}}}}/echo", "headers": ["Authorization: Bearer {{{{GURL_TEST_TOKEN}}}}"]}}
            }}
        }}"#,
        server.url("")
    ))
}

#[test]
fn run_lists_requests() {
    let server = TestServer::start();
    let coll = collection_file(&server);
    gurl()
        .arg("run")
        .arg(coll.path())
        .assert()
        .success()
        .stdout(concat!(
            "hello        GET  {{base}}/echo-headers  Say hi\n",
            "create       PUT  {{base}}/echo\n",
            "needs-token  GET  {{base}}/echo\n",
        ));
}

#[test]
fn run_sends_the_named_request_with_default_headers() {
    let server = TestServer::start();
    let coll = collection_file(&server);
    gurl()
        .arg("run")
        .arg(coll.path())
        .args(["hello", "X-Extra:2"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("GET /echo-headers "))
        .stdout(predicate::str::contains("\r\nX-Default: 1\r\n"))
        .stdout(predicate::str::contains("\r\nX-Extra: 2\r\n"))
        .stderr(predicate::str::contains("› hello"));
}

#[test]
fn run_collection_vars_are_the_lowest_precedence() {
    let server = TestServer::start();
    let coll = collection_file(&server);
    gurl()
        .arg("run")
        .arg(coll.path())
        .args(["create", "-s"])
        .assert()
        .success()
        .stdout(r#"{"who":"default"}"#);

    gurl()
        .arg("run")
        .arg(coll.path())
        .args(["create", "-s", "--var", "who=cli"])
        .assert()
        .success()
        .stdout(r#"{"who":"cli"}"#);
}

#[test]
fn run_items_replace_the_body() {
    let server = TestServer::start();
    let coll = collection_file(&server);
    gurl()
        .arg("run")
        .arg(coll.path())
        .args(["create", "-s", "who=items"])
        .assert()
        .success()
        .stdout(r#"{"who":"items"}"#);
}

#[test]
fn run_unknown_request_lists_names() {
    let server = TestServer::start();
    let coll = collection_file(&server);
    gurl()
        .arg("run")
        .arg(coll.path())
        .arg("nope")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "No request named `nope`. Available: hello, create, needs-token",
        ));
}

#[test]
fn run_undefined_variable_names_the_request() {
    let server = TestServer::start();
    let coll = collection_file(&server);
    gurl()
        .arg("run")
        .arg(coll.path())
        .arg("needs-token")
        .env_remove("GURL_TEST_TOKEN")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "In request `needs-token` of collection",
        ))
        .stderr(predicate::str::contains(
            "Undefined variable `GURL_TEST_TOKEN`",
        ));
}

#[test]
fn run_rejects_file_flag() {
    gurl()
        .args(["run", "x.json", "a", "-f", "y.json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--file can't be used with `gurl run`",
        ));
}

#[test]
fn example_collection_works_in_dry_run() {
    gurl()
        .args(["run", "examples/collection.json", "get-post", "--dry-run", "--var", "ID=3"])
        .env("TOKEN", "t")
        .assert()
        .success()
        .stdout(
            "curl -H 'Authorization: ***' -H 'X-Client: gurl' https://jsonplaceholder.typicode.com/posts/3\n",
        );
}

#[test]
fn options_before_a_subcommand_get_a_hint() {
    gurl()
        .args(["-s", "run", "api.json", "login"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Put options after the subcommand"));
}
