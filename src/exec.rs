use crate::curl::{self, CurlRequest, META_SENTINEL};
use crate::format;
use colored::*;
use serde_json::Value;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Instant;

/// How gurl presents the response (as opposed to what curl is asked to do).
#[derive(Debug, Default, Clone, Copy)]
pub struct RunOptions {
    pub pretty: bool,
    pub silent: bool,
    pub show_secrets: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// curl writes straight to our stdout: streaming and binary-safe
    Raw,
    /// The response is buffered so it can be formatted before printing
    Pretty,
}

pub fn output_mode(pretty: bool, has_output_file: bool) -> OutputMode {
    if pretty && !has_output_file {
        OutputMode::Pretty
    } else {
        OutputMode::Raw
    }
}

/// Response metadata reported by curl through `curl::meta_write_out`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Meta {
    pub status: Option<u16>,
    pub time_total: Option<f64>,
    pub size_download: Option<u64>,
    pub content_type: Option<String>,
}

pub fn parse_meta_line(line: &str) -> Option<Meta> {
    let rest = line.strip_prefix(META_SENTINEL)?.strip_prefix(' ')?;
    let mut fields = rest.trim_end().splitn(4, ' ');

    // curl reports 000 when no response was received
    let status = fields
        .next()?
        .parse::<u16>()
        .ok()
        .filter(|code| (100..=599).contains(code));
    let time_total = fields.next().and_then(|f| f.parse().ok());
    let size_download = fields.next().and_then(|f| f.parse().ok());
    let content_type = fields
        .next()
        .map(str::trim)
        .filter(|ct| !ct.is_empty())
        .map(str::to_string);

    Some(Meta {
        status,
        time_total,
        size_download,
        content_type,
    })
}

/// Copy curl's stderr to `out` line by line as it arrives, holding back the
/// metadata line (and the blank line that introduces it). With several
/// transfers (e.g. extra URLs passed through `--`) the last one wins.
fn forward_stderr<R: BufRead, W: Write>(mut reader: R, mut out: W) -> Meta {
    let mut meta = Meta::default();
    let mut pending_blank: Option<Vec<u8>> = None;
    let mut line = Vec::new();

    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        if line.starts_with(META_SENTINEL.as_bytes())
            && let Some(parsed) = parse_meta_line(&String::from_utf8_lossy(&line))
        {
            meta = parsed;
            pending_blank = None;
            continue;
        }

        if let Some(blank) = pending_blank.take() {
            let _ = out.write_all(&blank);
        }
        if line == b"\n" || line == b"\r\n" {
            pending_blank = Some(line.clone());
        } else {
            let _ = out.write_all(&line);
            let _ = out.flush();
        }
    }

    if let Some(blank) = pending_blank {
        let _ = out.write_all(&blank);
    }
    let _ = out.flush();
    meta
}

/// Split the header block(s) curl's `-i` puts in front of the body. There can
/// be several: redirects followed with `-L`, `100 Continue`, proxy responses.
fn split_headers(bytes: &[u8]) -> (&[u8], &[u8]) {
    let mut pos = 0;
    while bytes[pos..].starts_with(b"HTTP/") {
        match header_block_len(&bytes[pos..]) {
            Some(len) => pos += len,
            None => break,
        }
    }
    bytes.split_at(pos)
}

fn header_block_len(bytes: &[u8]) -> Option<usize> {
    let find = |needle: &[u8]| {
        bytes
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|i| i + needle.len())
    };
    match (find(b"\r\n\r\n"), find(b"\n\n")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn write_response(w: &mut impl Write, response: &[u8], has_headers: bool) -> io::Result<()> {
    let (head, body) = if has_headers {
        split_headers(response)
    } else {
        (&[][..], response)
    };

    if !head.is_empty() {
        format::write_response_headers(w, &String::from_utf8_lossy(head))?;
    }

    if let Ok(json) = serde_json::from_slice::<Value>(body) {
        format::write_json(w, &json, 0)?;
        writeln!(w)?;
    } else {
        w.write_all(body)?;
    }
    w.flush()
}

fn print_response(response: &[u8], has_headers: bool) -> io::Result<()> {
    let mut stdout = BufWriter::new(io::stdout().lock());
    match write_response(&mut stdout, response, has_headers) {
        // The reader went away (`gurl ... | head`): not an error
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

/// Run curl for `req` and return the exit code gurl should finish with.
pub fn run(req: &CurlRequest, opts: &RunOptions) -> anyhow::Result<i32> {
    if req.verbose && !opts.silent {
        let shown = if opts.show_secrets {
            req.clone()
        } else {
            req.masked()
        };
        eprintln!(
            "{} {}",
            "Command:".dimmed(),
            curl::display_command(&shown).dimmed()
        );
        eprintln!();
    }

    let mode = output_mode(opts.pretty, req.output.is_some());

    let mut cmd = Command::new("curl");
    cmd.args(curl::build_curl_args(req))
        .stdin(Stdio::inherit())
        .stderr(Stdio::piped())
        .stdout(match mode {
            OutputMode::Raw => Stdio::inherit(),
            OutputMode::Pretty => Stdio::piped(),
        });

    let start = Instant::now();

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            anyhow::bail!("curl is not installed or not in PATH. Please install curl to use gurl.");
        }
        Err(e) => return Err(e.into()),
    };

    // Drain stderr on its own thread so neither pipe can fill up and stall curl
    let stderr = child.stderr.take().expect("stderr is piped");
    let forwarder = thread::spawn(move || forward_stderr(BufReader::new(stderr), io::stderr()));

    let mut response = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        stdout.read_to_end(&mut response)?;
    }

    let status = child.wait()?;
    let meta = forwarder.join().unwrap_or_default();
    let elapsed = start.elapsed();

    if mode == OutputMode::Pretty {
        print_response(&response, req.include)?;
    }

    let exit_code = status.code().unwrap_or(1);
    if !opts.silent {
        if status.success() {
            format::print_success_footer(meta.status, elapsed);
        } else {
            format::print_failure_footer(meta.status, exit_code, elapsed);
        }
    }

    Ok(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_mode_is_pretty_only_when_asked_and_printing() {
        assert_eq!(output_mode(false, false), OutputMode::Raw);
        assert_eq!(output_mode(true, false), OutputMode::Pretty);
        assert_eq!(output_mode(true, true), OutputMode::Raw);
        assert_eq!(output_mode(false, true), OutputMode::Raw);
    }

    #[test]
    fn parse_meta_line_full() {
        let meta =
            parse_meta_line("__GURL_META__ 201 0.142310 1234 application/json; charset=utf-8\n")
                .unwrap();
        assert_eq!(
            meta,
            Meta {
                status: Some(201),
                time_total: Some(0.14231),
                size_download: Some(1234),
                content_type: Some("application/json; charset=utf-8".into()),
            }
        );
    }

    #[test]
    fn parse_meta_line_without_response() {
        let meta = parse_meta_line("__GURL_META__ 000 0.001200 0 \n").unwrap();
        assert_eq!(
            meta,
            Meta {
                status: None,
                time_total: Some(0.0012),
                size_download: Some(0),
                content_type: None,
            }
        );
    }

    #[test]
    fn parse_meta_line_rejects_other_lines() {
        assert_eq!(parse_meta_line("* Connected to example.com"), None);
        assert_eq!(parse_meta_line("__GURL_META__"), None);
        assert_eq!(parse_meta_line(""), None);
    }

    fn forward(input: &str) -> (String, Meta) {
        let mut out = Vec::new();
        let meta = forward_stderr(input.as_bytes(), &mut out);
        (String::from_utf8(out).unwrap(), meta)
    }

    #[test]
    fn forward_stderr_hides_only_the_meta_line() {
        let (out, meta) = forward("\n__GURL_META__ 200 0.1 12 text/plain\n");
        assert_eq!(out, "");
        assert_eq!(meta.status, Some(200));
    }

    #[test]
    fn forward_stderr_keeps_verbose_trace() {
        let (out, meta) = forward(
            "* Connected\n> GET / HTTP/1.1\n>\n< HTTP/1.1 404 Not Found\n\n__GURL_META__ 404 0.2 9 application/json\n",
        );
        assert_eq!(
            out,
            "* Connected\n> GET / HTTP/1.1\n>\n< HTTP/1.1 404 Not Found\n"
        );
        assert_eq!(meta.status, Some(404));
        assert_eq!(meta.content_type.as_deref(), Some("application/json"));
    }

    #[test]
    fn forward_stderr_keeps_unrelated_blank_lines_and_errors() {
        let (out, meta) = forward(
            "line one\n\nline two\n\n__GURL_META__ 000 0.0 0 \ncurl: (7) Failed to connect\n",
        );
        assert_eq!(out, "line one\n\nline two\ncurl: (7) Failed to connect\n");
        assert_eq!(meta.status, None);
    }

    #[test]
    fn forward_stderr_last_transfer_wins() {
        let (out, meta) =
            forward("\n__GURL_META__ 301 0.1 0 \n\n__GURL_META__ 200 0.2 5 text/html\n");
        assert_eq!(out, "");
        assert_eq!(meta.status, Some(200));
    }

    #[test]
    fn forward_stderr_without_meta() {
        let (out, meta) = forward("curl: (6) Could not resolve host: nope\n");
        assert_eq!(out, "curl: (6) Could not resolve host: nope\n");
        assert_eq!(meta, Meta::default());
    }

    #[test]
    fn split_headers_single_block() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        let (head, body) = split_headers(raw);
        assert_eq!(
            head,
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n"
        );
        assert_eq!(body, b"{\"a\":1}");
    }

    #[test]
    fn split_headers_redirect_chain() {
        let raw = b"HTTP/1.1 301 Moved\r\nLocation: /b\r\n\r\nHTTP/2 200 \r\nX: y\r\n\r\nbody";
        let (head, body) = split_headers(raw);
        assert!(head.ends_with(b"X: y\r\n\r\n"));
        assert_eq!(body, b"body");
    }

    #[test]
    fn split_headers_empty_body() {
        let raw = b"HTTP/1.1 204 No Content\r\n\r\n";
        let (head, body) = split_headers(raw);
        assert_eq!(head, raw);
        assert_eq!(body, b"");
    }

    #[test]
    fn split_headers_without_headers() {
        let (head, body) = split_headers(b"{\"a\":1}");
        assert_eq!(head, b"");
        assert_eq!(body, b"{\"a\":1}");
    }

    #[test]
    fn split_headers_unterminated_block_is_left_in_body() {
        let raw = b"HTTP/1.1 200 OK\r\nX: y";
        let (head, body) = split_headers(raw);
        assert_eq!(head, b"");
        assert_eq!(body, raw);
    }
}
