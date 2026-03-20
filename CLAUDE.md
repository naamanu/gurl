# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

gurl is a colorful CLI wrapper around `curl` written in Rust. It shells out to `curl` via `std::process::Command` — it is not an HTTP client itself. The entire application lives in a single file: `src/main.rs`.

## Build & Development Commands

```bash
cargo build                  # Debug build
cargo build --release        # Release build
cargo install --path .       # Install locally
cargo test --verbose         # Run tests
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

Single-binary CLI app using:
- **clap** (derive) for argument parsing → `Args` struct
- **serde/serde_json** for JSON parsing (request files and response pretty-printing)
- **colored** for terminal color output
- **anyhow** for error handling

Key flow: parse CLI args → optionally load request file (`RequestFile` struct) → merge CLI overrides with file values (CLI wins) → build curl command args → execute curl → optionally pretty-print JSON response.

The `HeadersFormat` enum handles two request file header formats: array of strings or key-value object.

Rust edition 2024 is used (`Cargo.toml`), which enables `let chains` (used in the `if let Some(...) && ...` pattern for JSON auto-detection).

## CI

GitHub Actions (`.github/workflows/ci.yml`) runs tests, fmt check, clippy, and cross-platform builds (Linux, macOS, Windows) on push/PR to main.
