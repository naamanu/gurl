# gurl

A simple, colorful CLI wrapper around `curl` for easier terminal usage.

## Features

- 🎨 **Syntax-highlighted JSON** - Pretty print responses with colored output
- 📄 **Request files** - Load headers and body from JSON files
- 🔍 **Smart JSON detection** - Auto-adds `Content-Type: application/json` when sending JSON
- 🏠 **Localhost shorthand** - Use `:3000/api` instead of `http://localhost:3000/api`
- ⏱️ **Response timing** - See how long requests take
- 🎯 **Method coloring** - Visual distinction between GET, POST, PUT, DELETE
- 🔗 **Pass-through** - Send any curl flag with `--`

## Installation

```bash
cargo install --path .
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

# With pretty JSON output
gurl --pretty https://jsonplaceholder.typicode.com/posts/1

# Localhost shorthand
gurl :3000/api/users
gurl :8080/health
```

### POST/PUT with Data

```bash
# POST with JSON (Content-Type auto-detected)
gurl -m POST -d '{"name":"John"}' https://api.example.com/users

# With pretty output and verbose mode
gurl --pretty -v -m POST -d '{"title":"Hello"}' https://api.example.com/posts
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

Headers can also be an array:

```json
{
  "headers": [
    "Authorization: Bearer token123",
    "Content-Type: application/json"
  ],
  "body": { "key": "value" }
}
```

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

| Flag | Long         | Description                                 |
| ---- | ------------ | ------------------------------------------- |
| `-m` | `--method`   | HTTP method (GET, POST, PUT, DELETE, PATCH) |
| `-d` | `--data`     | Request body (JSON auto-detected)           |
| `-H` | `--headers`  | Add header (repeatable)                     |
| `-f` | `--file`     | Load request from JSON file                 |
| `-p` | `--pretty`   | Pretty print JSON with syntax highlighting  |
| `-v` | `--verbose`  | Show full curl command and request details  |
| `-s` | `--silent`   | Suppress progress meter                     |
| `-L` | `--location` | Follow redirects                            |
| `-t` | `--timeout`  | Request timeout in seconds                  |
| `-o` | `--output`   | Save response to file                       |
| `-u` | `--user`     | Basic auth (user:password)                  |
| `-i` | `--include`  | Include response headers                    |

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

With `--pretty`, you get:

- **Blue** keys in JSON objects
- **Green** strings
- **Cyan** numbers
- **Yellow** booleans
- **Magenta** null values
- Color-coded HTTP methods (green GET, yellow POST, red DELETE, etc.)
- Response timing

## License

MIT
