# ester

This project is a Rust application designed to scrape proxy sources, test their connectivity, and write a list of working proxies.

## Overview

The main functionality of this project revolves around the following steps:
1. **Reading Proxy Sources**: Reads a JSON file (`assets/sources.json`) containing proxy source configurations (URL, HTTP method, and parser type).
2. **Making HTTP Requests**: Asynchronously fetches proxy lists from each source.
3. **Parsing Responses**: Supports `pandas` (HTML table), `json`, and `txt` parsers to extract IP/port pairs.
4. **Testing Proxies**: Tests each proxy against `https://icanhazip.com/` and verifies the returned IP matches the proxy's IP.
5. **Output**: Writes the list of working proxies to `proxies.txt` as `ip:port` lines.

## Usage

### Installation

To use this project, follow these steps:

1. **Clone the Repository**:
   ```bash
   git clone https://github.com/notfaj/ester.git
   cd ester
   ```

2. **Install Rust (if not already installed)**:
Follow the [official Rust installation guide](https://www.rust-lang.org/tools/install) to install Rust and Cargo.

3. **Build and Run**:
    ```bash
    cargo build --release
    cargo run --release
    ```

4. **Debugging**

    To enable debugging prints during runtime, you can use the `debug-print` feature with Cargo. This will enable additional debug prints throughout the codebase:
    ```bash
    cargo run --features debug-print
    ```

5. **Tests**
    ```bash
    cargo test
    ```

**Output**

Upon running the application, it will:
- Print per-source progress (requests made, proxies parsed, proxies that passed testing).
- Write the working proxies to `proxies.txt` (one `ip:port` per line).
- Print the total number of proxies written and elapsed time.

**Example**
```
Wrote 42 proxies to proxies.txt
Elapsed time: 18.43s
```

`proxies.txt` contents:
```
116.202.165.119:3124
154.236.177.100:1977
184.168.124.233:5402
```

## Automation

A GitHub Actions workflow (`.github/workflows/update-proxies.yml`) refreshes `proxies.txt` on a schedule and commits the result back to the repository.
