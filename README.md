# gurl

A simple, colorful CLI wrapper around `curl` for easier terminal usage.

## Features

- 🎨 **Syntax-highlighted JSON** - Pretty print responses with colored output
- ✏️ **Request items** - `gurl POST :3000/users name=Jo age:=30 X-Key:abc`, httpie style
- 📄 **Request files** - Load headers and body from JSON files, with `{{VARIABLES}}`
- 🔍 **Smart JSON detection** - Auto-adds `Content-Type: application/json` when sending JSON
- 🏠 **Localhost shorthand** - Use `:3000/api` instead of `http://localhost:3000/api`
- ⏱️ **Status at a glance** - `✓ HTTP 200 OK · 41ms · 1.2 KB` after every request
- 🚦 **Script-friendly** - Only the response on stdout; `--fail` exits non-zero on HTTP errors
- 🎯 **Method coloring** - Visual distinction between GET, POST, PUT, DELETE
- 🔗 **Pass-through** - Send any curl flag with `--`

## Installation

```bash
cargo install --path .
```

Shell completions:

```bash
gurl completions zsh  > ~/.zfunc/_gurl                            # zsh (~/.zfunc in fpath)
gurl completions bash > ~/.local/share/bash-completion/completions/gurl
gurl completions fish > ~/.config/fish/completions/gurl.fish
```

## Development

### Pre-commit Hooks

This project uses pre-commit hooks to ensure code quality. To set them up:

1. Install pre-commit:
   ```bash
   # Using pip
   pip install pre-commit
   
   # Or using homebrew (macOS)
   brew install pre-commit
   ```

2. Install the git hooks:
   ```bash
   pre-commit install
   ```

The hooks will automatically:
- Format code with `cargo fmt` before each commit
- Run `cargo clippy` to catch linting issues

To manually run the hooks:
```bash
pre-commit run --all-files
```

## Usage

### Basic Requests

```bash
# Simple GET
gurl https://api.example.com/users

# JSON is pretty-printed on a terminal; raw when piped
gurl https://jsonplaceholder.typicode.com/posts/1

# Localhost shorthand
gurl :3000/api/users
gurl :8080/health
```

### POST/PUT with Data

```bash
# POST with JSON (Content-Type auto-detected)
gurl -m POST -d '{"name":"John"}' https://api.example.com/users

# Sending data implies POST, as with curl
gurl -d '{"name":"John"}' https://api.example.com/users

# With pretty output and verbose mode
gurl --pretty -v -m POST -d '{"title":"Hello"}' https://api.example.com/posts

# Send a file (Content-Type set for .json), or stdin with @-
gurl -d @payload.json https://api.example.com/users
jq -n '{name: "John"}' | gurl -d @- https://api.example.com/users
```

`-d` sends its value exactly as written (curl's `--data-raw`). Only a leading
`@` reads a file, and the file is sent byte for byte. A `body` in a request
file is always sent as written, even if it starts with `@`.

### Request Items

After the URL, build the request from `key=value`-style items instead of
writing JSON by hand. An optional method goes before the URL.

```bash
gurl POST :3000/users name=Jo age:=30 admin:=true tags:='["a","b"]'
# → POST http://localhost:3000/users  {"name":"Jo","age":30,"admin":true,"tags":["a","b"]}

gurl :3000/search q==rust page==2 X-Api-Key:abc
# → GET http://localhost:3000/search?q=rust&page=2  with header X-Api-Key: abc

gurl --form :3000/upload title=Report doc@report.pdf   # multipart upload
gurl --form :3000/login user=jo pass=hunter2          # application/x-www-form-urlencoded
```

| Item          | Meaning                                                        |
| ------------- | -------------------------------------------------------------- |
| `Name:value`  | Header. `Name:` with nothing after it drops a header curl adds |
| `name==value` | Query parameter, URL-encoded                                   |
| `name=value`  | String field in the JSON body (form field with `--form`)       |
| `name:=json`  | Raw JSON field: number, boolean, null, array, object           |
| `name=@path`  | String field read from a text file                             |
| `name:=@path` | JSON field read from a file                                    |
| `name@path`   | File upload (`--form`, sent as multipart/form-data)            |

Body fields make the request a POST unless a method is given, and send
`Content-Type` and `Accept` for JSON. They can't be combined with `-d`.

### Shortcuts

```bash
gurl --bearer "$TOKEN" api.example.com/me          # Authorization: Bearer ...
gurl -q 'q=rust lang' -q page=2 api.example.com/s  # URL-encoded query parameters
gurl --json -d @payload api.example.com/items      # JSON Content-Type and Accept, whatever -d looks like
```

### Request Files

Load complex requests from JSON files:

```bash
# Load entire request from file
gurl --file request.json --pretty

# Override URL from file
gurl --file request.json https://different-url.com

# Override method from file
gurl --file request.json -m PUT

# Add to it with request items (the URL comes from the file)
gurl --file request.json X-Trace:1 name=Jo
```

**Request file format:**

```json
{
  "method": "POST",
  "url": "https://api.example.com/users",
  "headers": {
    "Authorization": "Bearer token123",
    "X-Custom": "value"
  },
  "body": {
    "name": "John",
    "email": "john@example.com"
  }
}
```

Headers are sent in the order written. A `null` value (`"Accept": null`)
removes a header curl would otherwise add. Headers can also be an array:

```json
{
  "headers": [
    "Authorization: Bearer token123",
    "Content-Type: application/json"
  ],
  "body": { "key": "value" }
}
```

**Variables.** Any string in a request file can use `{{NAME}}`, so the file
can be shared without the secrets in it:

```json
{
  "url": "https://{{API_HOST}}/users",
  "headers": { "Authorization": "Bearer {{TOKEN}}" }
}
```

```bash
gurl -f users.json --var API_HOST=api.example.com   # highest precedence
TOKEN=abc gurl -f users.json                        # then the environment
gurl -f users.json --env-file .env                  # then NAME=VALUE lines
```

An undefined variable is an error, raised before anything is sent. Values are
inserted into the parsed JSON, so quotes in a value can't break the body.
Write `\\{{` in a JSON string for a literal `{{`. Variables apply to request
files only; on the command line, use your shell's `$VARIABLES`.

### Headers & Auth

```bash
# Custom headers
gurl -H "Authorization: Bearer token123" https://api.example.com/me

# Basic auth
gurl -u admin:password https://api.example.com/admin

# Multiple headers
gurl -H "Accept: application/json" -H "X-Custom: value" https://api.example.com
```

### Options

| Flag | Long             | Description                                         |
| ---- | ---------------- | --------------------------------------------------- |
| `-m` | `--method`       | HTTP method (default GET, or POST when sending data) |
| `-d` | `--data`         | Request body, sent as-is; `@file` or `@-` for stdin |
| `-H` | `--header`       | Add header (repeatable; `--headers` also works)     |
| `-q` | `--query`        | Add a URL-encoded query parameter `NAME=VALUE`      |
|      | `--bearer`       | Send `Authorization: Bearer TOKEN`                  |
|      | `--json`         | Send JSON `Content-Type` and `Accept` headers       |
|      | `--form`         | Send request items as a form (multipart with files) |
| `-f` | `--file`         | Load request from JSON file                         |
|      | `--var`          | Set a `{{NAME}}` request-file variable `NAME=VALUE` |
|      | `--env-file`     | Read `{{NAME}}` variables from a `.env` file        |
| `-p` | `--pretty`       | Pretty print JSON, even when piped                  |
|      | `--raw`          | Never reformat the response, even on a terminal     |
|      | `--fail`         | Exit 22 on HTTP 4xx/5xx (body is still printed)     |
| `-v` | `--verbose`      | Show the curl command, request and curl's trace     |
| `-s` | `--silent`       | Only the response: no banner, no status             |
| `-L` | `--location`     | Follow redirects                                    |
| `-t` | `--timeout`      | Request timeout in seconds                          |
| `-o` | `--output`       | Save response to file                               |
| `-u` | `--user`         | Basic auth (user:password)                          |
| `-i` | `--include`      | Include response headers                            |
|      | `--dry-run`      | Print the equivalent curl command; send nothing     |
|      | `--show-secrets` | Don't mask credentials in `-v` / `--dry-run` output |

URLs without a scheme get `https://`, except local hosts (`localhost`,
`127.x.x.x`, `[::1]`, `*.localhost`), which get `http://`.

### See the curl command

`--dry-run` prints the curl command gurl would run, quoted so it can be pasted
into a shell or a bug report. `Authorization` headers and `-u` passwords are
shown as `***` unless you add `--show-secrets`.

```bash
$ gurl --dry-run -H 'Authorization: Bearer abc' -d '{"a":1}' :3000/items
curl -H 'Authorization: ***' -H 'Content-Type: application/json' --data-raw '{"a":1}' http://localhost:3000/items
```

### Pass-through to curl

Use `--` to pass additional flags directly to curl:

```bash
# Follow redirects and compressed response
gurl https://example.com -- -L --compressed

# Custom certificate
gurl https://internal.example.com -- --cacert /path/to/cert.pem
```

## Examples

```bash
# GET with pretty JSON
gurl --pretty https://jsonplaceholder.typicode.com/posts/1

# POST with auto Content-Type
gurl -m POST -d '{"userId":1,"title":"foo","body":"bar"}' \
  --pretty https://jsonplaceholder.typicode.com/posts

# Using a request file
gurl --file examples/create_post.json --pretty -v

# Quick localhost API testing
gurl :3000/api/health
gurl -m POST -d '{"email":"test@example.com"}' :3000/api/register
```

## Output

The response is the only thing written to stdout. The request banner and the
status line go to stderr, so piping and redirecting just work — binary and
streaming responses included:

```bash
gurl https://api.example.com/users | jq '.[0]'
gurl https://example.com/logo.png > logo.png
```

After each request gurl prints the status, curl's timing and the download size:

```
✓ HTTP 201 Created · 142ms · 1.2 KB
✗ HTTP 404 Not Found · 38ms · 21 B
✗ Failed · 3ms (curl exit code 7)
```

An HTTP error still exits 0, as with curl. Add `--fail` to exit with 22
instead, e.g. `gurl --fail -s :3000/health || echo down`. `-s` hides the
banner and status line.

On a terminal, JSON responses are pretty-printed (`--raw` turns that off;
`--pretty` forces it when piping). Other content types are passed through
untouched and streamed as they arrive. You get:

- **Blue** keys in JSON objects
- **Green** strings
- **Cyan** numbers
- **Yellow** booleans
- **Magenta** null values
- Color-coded HTTP methods (green GET, yellow POST, red DELETE, etc.)
- Status codes colored by class (2xx green, 3xx cyan, 4xx yellow, 5xx red)

Keys stay in the order the server sent them. Colors follow the
[`NO_COLOR`](https://no-color.org) / `CLICOLOR_FORCE` conventions.

## License

MIT
