# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

gurl is a colorful CLI wrapper around `curl` written in Rust. It shells out to `curl` via `std::process::Command` — it is not an HTTP client itself, and any curl flag can be passed through after `--`.

## Build & Development Commands

```bash
cargo build                  # Debug build
cargo build --release        # Release build
cargo install --path .       # Install locally
cargo test                   # Unit tests + end-to-end tests (needs curl on PATH)
cargo test --lib             # Unit tests only
cargo test --test cli        # End-to-end tests only
cargo fmt --all -- --check   # Check formatting
cargo clippy --all-targets --all-features -- -D warnings  # Lint
```

## Pre-commit Hooks

Pre-commit hooks run `cargo fmt` and `cargo clippy` before each commit. Set up with:
```bash
pre-commit install
pre-commit run --all-files   # Manual run
```

## Architecture

A library crate (`src/lib.rs`) with a thin binary (`src/main.rs`): parse args → load request file → `request::resolve` → print banner → `exec::run` → exit with curl's exit code.

| Module | Responsibility |
| --- | --- |
| `args.rs` | clap `Args` definition only |
| `request.rs` | `RequestFile` / `HeadersFormat` serde types, `load_request_file`, and `resolve(args, file) -> CurlRequest` — a pure function holding all CLI-vs-file merge rules (CLI wins; file headers precede CLI headers; JSON bodies get a `Content-Type` unless one is set) |
| `url.rs` | `expand_url` (`:3000/x` → `http://localhost:3000/x`, bare host → `https://`) |
| `curl.rs` | `CurlRequest`, `Body` (`Raw` → `--data-raw`, `File` → `--data-binary @path`) and `build_curl_args` (pure, order matters: extra args go last before the URL so the user can override anything). `-X` is only passed when curl wouldn't infer the method; `HEAD` becomes `-I`. `display_command` is the shell-quoted command minus gurl's own plumbing; always pass it `req.masked()` unless `--show-secrets` |
| `exec.rs` | Spawns curl, plumbs its streams, returns the exit code. Never calls `process::exit` |
| `format.rs` | Banner, status footer, response-header coloring, JSON pretty-printer |

### Stream contract (the invariant to preserve)

- **stdout carries only the response.** Banner, status footer and errors all go to stderr, so `gurl url | jq` and `gurl url > file` just work. `-s/--silent` suppresses gurl's decorations entirely.
- curl is always run with `-sS` and a `-w` format that starts with `%{stderr}`: response metadata (status, time, size, content type) arrives on curl's **stderr**, on a line starting with `__GURL_META__`. `exec::forward_stderr` forwards curl's stderr live (so `-v` works) and holds back only that line.
- `exec::output_mode` picks how stdout is wired: `Raw` **inherits** our stdout (streaming, binary-safe — never decode or buffer on this path); `Pretty` pipes and buffers bytes, splits off `-i` header blocks, pretty-prints the body if it parses as JSON, and otherwise writes the bytes through untouched.

## Testing

- Unit tests live next to the code. Keep logic in pure functions (`resolve`, `build_curl_args`, `parse_meta_line`, `forward_stderr`, `split_headers`) and assert exact values — e.g. whole argument vectors, not `contains`.
- `tests/cli.rs` runs the real binary against a small `TcpListener` server defined in the same file. Add a route there rather than calling the network. Under `assert_cmd` stdout is not a TTY, so output is uncolored; the helper also sets `NO_COLOR`.

Rust edition 2024 is used, which enables `let` chains (`if let Some(..) = x && cond`).

## CI

GitHub Actions (`.github/workflows/ci.yml`) runs tests, fmt check, clippy, and cross-platform builds (Linux, macOS, Windows) on push/PR to main.
