# ester

This project is a Rust application designed to scrape proxy sources from a JSON file, test their connectivity, and provide a list of working proxies.

## Overview

The main functionality of this project revolves around the following steps:
1. **Reading Proxy Sources**: Reads a JSON file (`src\sources.json`) containing proxy configurations.
2. **Making HTTP Requests**: Asynchronously makes HTTP requests to gather proxy lists from various sources.
3. **Testing Proxies**: Tests the gathered proxies on ('https://steamcommunity.com/market/') for connectivity and reliability.
4. **Output**: Outputs the list of working proxies.

## Usage

### Installation

To use this project, follow these steps:

1. **Clone the Repository**:
   ```bash
   git clone https://github.com/notfaj/ester.git
   cd your-repository

2. **Install Rust (if not already installed)**:
Follow the [official Rust installation guide](https://www.rust-lang.org/tools/install) to install Rust and Cargo.


3. **Build and Run**:
    ```bash
    cargo build --release
    cargo run
4. **Debugging**
   
    To enable debugging prints during runtime, you can use the debug-print feature with Cargo. This will enable additional debug prints throughout the codebase:
    ```bash
      cargo run --features debug-print
**Output**
Upon running the application, it will:
Output the number of proxies successfully tested and deemed reliable.

**Example**
   ```bash
Final list of proxies: [
      Proxy { ip: "116.202.165.119", port: 3124 },
      Proxy { ip: "154.236.177.100", port: 1977 },
      Proxy { ip: "184.168.124.233", port: 5402 }
]
