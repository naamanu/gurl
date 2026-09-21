use crate::curl::{self, CurlRequest, META_SENTINEL};
use crate::format;
use colored::*;
use serde_json::Value;
use std::io::{self, BufRead, BufReader, BufWriter, IsTerminal, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Instant;

/// What the user asked for with `--pretty` / `--raw`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PrettyChoice {
    /// Pretty on a terminal, raw when piped
    #[default]
    Auto,
    Always,
    Never,
}

/// How gurl presents the response (as opposed to what curl is asked to do).
#[derive(Debug, Default, Clone, Copy)]
pub struct RunOptions {
    pub pretty: PrettyChoice,
    pub silent: bool,
    pub show_secrets: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// curl writes straight to our stdout: streaming and binary-safe
    Raw,
    /// The response passes through gurl so JSON can be formatted
    /// (see `relay_response`)
    Pretty,
}

pub fn output_mode(pretty: PrettyChoice, has_output_file: bool, stdout_is_tty: bool) -> OutputMode {
    let wanted = match pretty {
        PrettyChoice::Always => true,
        PrettyChoice::Never => false,
        PrettyChoice::Auto => stdout_is_tty,
    };
    if wanted && !has_output_file {
        OutputMode::Pretty
    } else {
        OutputMode::Raw
    }
}

/// What happened, for the caller to turn into an exit code.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// curl's exit code
    pub exit_code: i32,
    pub meta: Meta,
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

/// Relay the response from `r` to `w`, pretty-printing it if it is JSON.
///
/// Only something that starts like JSON has to be held back until the end;
/// anything else (HTML, text, event streams, binary) is passed through as it
/// arrives. With `-i` the header block(s) come first, so that is buffered.
fn relay_response<R: Read, W: Write>(mut r: R, w: &mut W, has_headers: bool) -> io::Result<()> {
    let mut buf = Vec::new();
    if has_headers {
        r.read_to_end(&mut buf)?;
        return write_response(w, &buf, true);
    }

    let mut chunk = [0u8; 8192];
    loop {
        let n = r.read(&mut chunk)?;
        if n == 0 {
            // Empty or whitespace only
            w.write_all(&buf)?;
            return w.flush();
        }
        buf.extend_from_slice(&chunk[..n]);

        let Some(&first) = buf.iter().find(|b| !b.is_ascii_whitespace()) else {
            continue;
        };
        if first == b'{' || first == b'[' {
            r.read_to_end(&mut buf)?;
            return write_response(w, &buf, false);
        }

        w.write_all(&buf)?;
        w.flush()?;
        loop {
            let n = r.read(&mut chunk)?;
            if n == 0 {
                return Ok(());
            }
            w.write_all(&chunk[..n])?;
            w.flush()?;
        }
    }
}

fn print_response<R: Read>(r: R, has_headers: bool) -> io::Result<()> {
    let mut stdout = BufWriter::new(io::stdout().lock());
    match relay_response(r, &mut stdout, has_headers) {
        // The reader went away (`gurl ... | head`): not an error
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

/// Run curl for `req`, printing the response and gurl's status footer.
pub fn run(req: &CurlRequest, opts: &RunOptions) -> anyhow::Result<Outcome> {
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

    let mode = output_mode(
        opts.pretty,
        req.output.is_some(),
        io::stdout().is_terminal(),
    );

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

    if let Some(stdout) = child.stdout.take() {
        print_response(stdout, req.include)?;
    }

    let status = child.wait()?;
    let meta = forwarder.join().unwrap_or_default();
    let elapsed = start.elapsed();

    let exit_code = status.code().unwrap_or(1);
    if !opts.silent {
        if status.success() {
            format::print_success_footer(&meta, elapsed);
        } else {
            format::print_failure_footer(&meta, exit_code, elapsed);
        }
    }

    Ok(Outcome { exit_code, meta })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_mode_follows_the_terminal_by_default() {
        use PrettyChoice::*;
        assert_eq!(output_mode(Auto, false, true), OutputMode::Pretty);
        assert_eq!(output_mode(Auto, false, false), OutputMode::Raw);
        assert_eq!(output_mode(Always, false, false), OutputMode::Pretty);
        assert_eq!(output_mode(Never, false, true), OutputMode::Raw);
    }

    #[test]
    fn output_mode_is_raw_when_writing_to_a_file() {
        use PrettyChoice::*;
        for choice in [Auto, Always, Never] {
            assert_eq!(output_mode(choice, true, true), OutputMode::Raw);
        }
    }

    /// A reader that hands out its data in the given pieces, like a pipe
    struct Chunked(std::collections::VecDeque<Vec<u8>>);

    impl Read for Chunked {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let Some(mut piece) = self.0.pop_front() else {
                return Ok(0);
            };
            let n = piece.len().min(out.len());
            out[..n].copy_from_slice(&piece[..n]);
            if n < piece.len() {
                self.0.push_front(piece.split_off(n));
            }
            Ok(n)
        }
    }

    fn relay(pieces: &[&str], has_headers: bool) -> String {
        colored::control::set_override(false);
        let reader = Chunked(pieces.iter().map(|p| p.as_bytes().to_vec()).collect());
        let mut out = Vec::new();
        relay_response(reader, &mut out, has_headers).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn relay_pretty_prints_json_split_across_reads() {
        assert_eq!(
            relay(&["  ", "\n{\"a\"", ":1}"], false),
            "{\n  \"a\": 1\n}\n"
        );
    }

    #[test]
    fn relay_passes_invalid_json_through_unchanged() {
        assert_eq!(
            relay(&["{\"a\":1}\n", "{\"a\":2}\n"], false),
            "{\"a\":1}\n{\"a\":2}\n"
        );
    }

    #[test]
    fn relay_passes_non_json_through() {
        assert_eq!(
            relay(&["data: 1\n\n", "data: 2\n\n"], false),
            "data: 1\n\ndata: 2\n\n"
        );
        assert_eq!(relay(&["", ""], false), "");
        assert_eq!(relay(&["\n", " "], false), "\n ");
    }

    /// Records each write, to observe what is passed on before EOF
    #[derive(Default)]
    struct Recorder(Vec<Vec<u8>>);

    impl Write for Recorder {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.push(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn relay_streams_non_json_as_it_arrives() {
        let reader = Chunked(
            ["<html>", "<body>", "</html>"]
                .iter()
                .map(|p| p.as_bytes().to_vec())
                .collect(),
        );
        let mut out = Recorder::default();
        relay_response(reader, &mut out, false).unwrap();
        assert_eq!(
            out.0,
            vec![b"<html>".to_vec(), b"<body>".to_vec(), b"</html>".to_vec()]
        );
    }

    #[test]
    fn relay_with_headers_formats_the_body() {
        assert_eq!(
            relay(&["HTTP/1.1 200 OK\r\nX: y\r\n\r\n", "[1]"], true),
            "HTTP/1.1 200 OK\nX: y\n\n[\n  1\n]\n"
        );
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
