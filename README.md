# gurl

A very simple CLI wrapper around `curl` for simplified terminal usage.

## Usage

```bash
# Simple GET
gurl http://example.com

# POST with data
gurl http://example.com -m POST -d '{"foo":"bar"}'

# Headers
gurl http://example.com -H "Authorization: Bearer token"

# Verbose output
gurl http://example.com -v

# Pass through extra args to curl (use --)
# Example: Silent mode and follow redirects
gurl http://example.com -- -s -L
```

## Features

- **Simplified Syntax**: Common flags like `-m` for method, `-d` for data, `-H` for headers.
- **Visual Feedback**: Prints the executed command (in dimmed color) to stderr.
- **Pass-through**: Use `--` to pass any extra arguments directly to `curl`.

## Installation

```bash
cargo install --path .
```
