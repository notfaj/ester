# ester

A Rust application that scrapes proxy sources, validates them, and runs a local rotating proxy server you can point any HTTP client at.

## Overview

Two binaries are included:

- **`ester`** — scrapes 7 proxy sources, tests each proxy, and writes working ones to `proxies.txt`
- **`server`** — a local rotating proxy server that re-scrapes on startup and on a configurable interval, filters by speed and site accessibility, and rotates through the pool on every request

## Proxy Server

### Starting the server

```bash
cargo run --release --bin server
```

On startup the server scrapes all proxy sources, validates each proxy against the configured `test_url` within the `speed_threshold_ms` limit, then begins accepting connections:

```
[refresh] scraping proxies...
[refresh] scraped 37 proxies, validating...
[refresh] Pool refreshed: 22 proxies active
Listening on 127.0.0.1:8080
```

### Using it from Python

```python
import requests

PROXIES = {
    "http":  "http://127.0.0.1:8080",
    "https": "http://127.0.0.1:8080",
}

response = requests.get("https://example.com", proxies=PROXIES, timeout=15)
```

Every request is forwarded through a different upstream proxy (round-robin by default). HTTPS is fully supported via CONNECT tunneling — the server never decrypts your traffic.

> **Note:** Use `https://` URLs. The proxies in the pool are CONNECT-only and do not support plain HTTP forwarding.

### Configuration

Edit `config.yaml` while the server is running — changes are picked up automatically within 10 seconds, no restart needed.

```yaml
server:
  host: "127.0.0.1"
  port: 8080

filtering:
  speed_threshold_ms: 3000        # reject proxies slower than this (ms)
  test_url: "https://www.google.com/"  # proxy must reach this URL to be admitted
  check_timeout: 8                # per-proxy test timeout (seconds)
  concurrent_checks: 20           # parallel validation workers

rotation:
  strategy: "round-robin"         # or "random"

health:
  max_failures: 3                 # consecutive failures before a proxy is ejected

recheck:
  interval: 3600                  # seconds between full re-scrape + revalidation
```

| Setting | Effect when changed live |
|---|---|
| `strategy` | Immediate — next request uses new strategy |
| `max_failures` | Immediate — next failure check uses new threshold |
| `filtering.*` | Next recheck cycle |
| `recheck.interval` | Next sleep iteration |
| `server.host` / `server.port` | Ignored (cannot rebind live listener) |

### Testing

```bash
pip install requests
python test_proxy.py
```

Runs a single request, a 5-request rotation check, and a 20-thread concurrency test.

---

## Proxy Scraper (standalone)

### Usage

```bash
cargo run --release
```

Writes working proxies to `proxies.txt` as `ip:port` lines.

```
Wrote 42 proxies to proxies.txt
Elapsed time: 18.43s
```

### Debugging

```bash
cargo run --features debug-print
```

### Tests

```bash
cargo test
```

## Automation

A GitHub Actions workflow (`.github/workflows/update-proxies.yml`) refreshes `proxies.txt` hourly and commits the result back to the repository.
