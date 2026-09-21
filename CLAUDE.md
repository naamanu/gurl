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

A library crate (`src/lib.rs`) with a thin binary (`src/main.rs`): parse args → load request file and substitute variables → `request::resolve` → print banner → `exec::run` → exit code.

| Module | Responsibility |
| --- | --- |
| `args.rs` | clap definitions only: `Cli { command: Option<Command>, args: Args }` with `args_conflicts_with_subcommands`, so `gurl <url>` and `gurl completions zsh` coexist |
| `request.rs` | `RequestFile` / `HeadersFormat` serde types, `load_request_file`, `split_targets` (positional `[METHOD] URL [ITEM]...`), and `resolve(args, file) -> CurlRequest` — a pure function holding all CLI-vs-file merge rules (CLI wins; headers go file → `--bearer` → `-H` → items; JSON bodies get a `Content-Type` unless one is set) |
| `url.rs` | `expand_url` (`:3000/x` → `http://localhost:3000/x`, bare host → `https://`), `append_query`, `percent_encode` |
| `items.rs` | httpie-style request items (`Name:v`, `n==v`, `n=v`, `n:=json`, `n=@file`, `n:=@file`, `n@file`): `parse_item` (earliest separator wins, ties go to the longest) and `build` → headers, query, JSON / urlencoded / multipart body |
| `vars.rs` | `{{NAME}}` substitution (`Vars`, precedence `--var` > env > `--env-file` > collection defaults) and the `.env` parser. Applied to request files only, on parsed JSON (`RequestFile::substitute`) |
| `curl.rs` | `CurlRequest`, `Body` (`Raw` → `--data-raw`, `File` → `--data-binary @path`, `Multipart` → `--form-string` / `-F name=@path`) and `build_curl_args` (pure, order matters: extra args go last before the URL so the user can override anything). `-X` is only passed when curl wouldn't infer the method; `HEAD` becomes `-I`. `display_command` is the shell-quoted command minus gurl's own plumbing; always pass it `req.masked()` unless `--show-secrets` |
| `exec.rs` | Spawns curl, plumbs its streams, returns the exit code. Never calls `process::exit` |
| `format.rs` | Banner, status footer, response-header coloring, JSON pretty-printer |

### Stream contract (the invariant to preserve)

- **stdout carries only the response.** Banner, status footer and errors all go to stderr, so `gurl url | jq` and `gurl url > file` just work. `-s/--silent` suppresses gurl's decorations entirely.
- curl is always run with `-sSN` (`-N`: curl's stdout is never a TTY, so it would otherwise buffer the whole response) and a `-w` format that starts with `%{stderr}`: response metadata (status, time, size, content type) arrives on curl's **stderr**, on a line starting with `__GURL_META__`. `exec::forward_stderr` forwards curl's stderr live (so `-v` works) and holds back only that line.
- `exec::output_mode(PrettyChoice, has_output_file, stdout_is_tty)` picks how stdout is wired. Pretty is the default on a terminal (`--pretty` forces it, `--raw` disables it). `Raw` **inherits** our stdout (streaming, binary-safe — never decode or buffer on this path). `Pretty` pipes through `exec::relay_response`: a body whose first non-space byte is `{` or `[` is buffered and pretty-printed if it parses (else written back untouched); anything else is streamed through as it arrives. With `-i` the whole response is buffered so header blocks can be split off.
- `exec::run` returns an `Outcome { exit_code, meta }`; `main` maps it to the process exit code (`--fail` → 22 on HTTP ≥ 400).

## Testing

- Unit tests live next to the code. Keep logic in pure functions (`resolve`, `build_curl_args`, `parse_meta_line`, `forward_stderr`, `split_headers`) and assert exact values — e.g. whole argument vectors, not `contains`.
- `tests/cli.rs` runs the real binary against a small `TcpListener` server defined in the same file. Add a route there rather than calling the network. Under `assert_cmd` stdout is not a TTY, so output is uncolored; the helper also sets `NO_COLOR`.

Rust edition 2024 is used, which enables `let` chains (`if let Some(..) = x && cond`).

## CI

GitHub Actions (`.github/workflows/ci.yml`): fmt + clippy, the full test suite on Linux, macOS and Windows, and `cargo check` on the minimum Rust version (`rust-version` in Cargo.toml, 1.88).
