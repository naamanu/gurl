# Changelog

All notable changes to gurl are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org).

## 0.3.0 - Unreleased

### Breaking changes

- **stdout carries only the response.** The request banner, status line and
  gurl's errors now go to stderr, so `gurl url | jq` and `gurl url > file`
  get the response alone. Scripts that parsed gurl's decorations from stdout
  need to read stderr.
- **JSON is pretty-printed by default on a terminal.** Use `--raw` for the
  response exactly as received. When piped, output is raw as before.
- **`-s/--silent` hides gurl's banner and status line.** It used to be passed
  to curl, where it only hid the progress meter, which gurl now always hides.
- **`-d` sends its value as written** (`--data-raw`). Only a leading `@` reads
  a file (`-d @path`, `-d @-` for stdin), which is now sent byte for byte,
  newlines included. A string `body` in a request file is never read as a file.
- **Sending data without `-m` means POST**, as in curl. It used to be sent as
  a GET with a body.
- **URLs without a scheme keep their scheme choice per host.** `localhost`,
  `127.x.x.x`, `[::1]` and `*.localhost` now get `http://` instead of
  `https://`, and any `scheme://` (`ws://`, `file://`, …) is kept as is.
- **Positional arguments are `[METHOD] URL [ITEM]...`.** A word after the URL
  used to be an error. It's now a request item, and a leading method word
  (`gurl DELETE url`) is recognised.

### Added

- Request items, httpie style: `Name:value` headers, `name==value` query
  parameters, `name=value` / `name:=json` body fields, `name=@file` /
  `name:=@file` fields from files, and `name@file` uploads with `--form`.
- `--bearer TOKEN`, `--json`, `--form`, and `-q/--query NAME=VALUE`.
- `{{NAME}}` variables in request files, from `--var`, the environment and
  `--env-file`.
- Collections: `gurl run collection.json` lists named requests, and
  `gurl run collection.json name` runs one, with shared `vars` and default
  headers. See `examples/collection.json`.
- A status line after every request, e.g. `✓ HTTP 201 Created · 142ms · 1.2 KB`.
- `--fail`: exit 22 on HTTP 4xx/5xx, still printing the body.
- `--dry-run` prints the equivalent, shell-quoted curl command without
  sending anything. `-v` shows the same command. Credentials are masked
  unless `--show-secrets`.
- `--header` as an alias of `--headers`, as in curl.
- `gurl completions <shell>` for bash, zsh, fish, elvish and PowerShell.
- `-i` with `--pretty` highlights the response headers and still formats the
  body.
- Prebuilt binaries for Linux (x86_64, aarch64), macOS (x86_64, aarch64) and
  Windows (x86_64) on each GitHub release.

### Fixed

- `-v` showed nothing on success, and curl's own error messages were hidden.
- Binary responses were corrupted, and slow or streaming responses only
  appeared once complete.
- The JSON pretty-printer didn't escape strings, so a quote or newline in a
  value produced invalid JSON. It also sorted object keys; the server's order
  is kept now.
- An HTTP 4xx/5xx response was reported with a green "Response received".
- `-m HEAD` hung waiting for a body.
- A forced `-X` was reapplied to every redirect followed with `-L`.
- Request-file header objects were sent in random order.
- `Content-Type-Options` (and other headers starting with "content-type") was
  mistaken for `Content-Type`, turning off JSON detection.
- Piping into a command that exits early (`gurl url | head`) no longer
  reports a broken pipe.
- A missing `curl` gives a clear error.

## 0.2.0

- Request files (`-f/--file`) with headers, body, method and URL.
- Syntax-highlighted JSON with `--pretty`, `:port/path` localhost shorthand,
  response timing, and method coloring.
