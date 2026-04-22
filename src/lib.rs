use reqwest::{Client, Method, Proxy as RequestProxy};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::Duration;

#[cfg(feature = "debug-print")]
macro_rules! debug_println {
    ($fmt:expr) => {
        println!($fmt);
    };
    ($fmt:expr, $($arg:tt)*) => {
        println!($fmt, $($arg)*);
    };
}

#[cfg(not(feature = "debug-print"))]
macro_rules! debug_println {
    ($fmt:expr) => {};
    ($fmt:expr, $($arg:tt)*) => {};
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Parser {
    pub pandas: Option<PandasParser>,
    pub json: Option<JsonParser>,
    pub txt: Option<TxtParser>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct PandasParser {
    pub table_index: u32,
    pub ip: Option<String>,
    pub port: Option<String>,
    pub combined: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct JsonParser {
    pub data: String,
    pub ip: String,
    pub port: String,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct TxtParser {}

#[derive(Serialize, Deserialize, Debug)]
pub struct Source {
    pub id: String,
    pub url: String,
    pub method: String,
    pub parser: Parser,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Proxy {
    pub ip: String,
    pub port: u16,
    pub source: Option<String>,
    pub location: Option<String>,
}

impl Proxy {
    pub fn new(ip: &str, port: &str) -> Result<Self, std::num::ParseIntError> {
        let parsed_port: u16 = port.parse()?;
        Ok(Self {
            ip: ip.to_string(),
            port: parsed_port,
            source: None,
            location: None,
        })
    }

    pub fn with_source(mut self, source_id: impl Into<String>) -> Self {
        self.source = Some(source_id.into());
        self
    }

    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    pub fn to_url(&self) -> String {
        format!("http://{}:{}", self.ip, self.port)
    }

    pub async fn test_proxy(&self) -> bool {
        let proxy_url = self.to_url();

        let client = match reqwest::Client::builder()
            .proxy(RequestProxy::all(&proxy_url).expect("Invalid proxy URL"))
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(client) => client,
            Err(_err) => {
                debug_println!("Proxy client build failed: {}", _err);
                return false;
            }
        };

        match client.get("https://icanhazip.com/").send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    debug_println!("Proxy request returned bad status: {}", response.status());
                    return false;
                }

                match response.text().await {
                    Ok(body) => {
                        let received_ip = body.trim();
                        let expected_ip = self.ip.trim();
                        if received_ip == expected_ip {
                            true
                        } else {
                            debug_println!(
                                "Proxy IP mismatch: expected {}, got {}",
                                expected_ip,
                                received_ip
                            );
                            false
                        }
                    }
                    Err(_err) => {
                        debug_println!("Failed to read proxy response body: {}", _err);
                        false
                    }
                }
            }
            Err(_err) => {
                debug_println!("Proxy request failed: {}", _err);
                false
            }
        }
    }
}

pub async fn make_request(client: &Client, url: &str, method: &str) -> Result<String, io::Error> {
    println!("make_request: {} {}", method, url);
    let req_method: Method = method
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Unsupported HTTP method"))?;

    let response = client.request(req_method, url).send().await.map_err(|e| {
        eprintln!("make_request error for {} {}: {}", method, url, e);
        io::Error::new(io::ErrorKind::Other, e)
    })?;

    if !response.status().is_success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Request failed with status code: {}", response.status()),
        ));
    }

    response
        .text()
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

pub fn parse_proxies_txt(input: &str) -> Vec<Proxy> {
    input
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.trim().split(':').collect();
            if parts.len() == 2 {
                match Proxy::new(parts[0], parts[1]) {
                    Ok(proxy) => Some(proxy),
                    Err(_) => None,
                }
            } else {
                None
            }
        })
        .collect()
}

pub fn parse_proxies_json(response_str: &str) -> Vec<Proxy> {
    let json_value: Value = match serde_json::from_str(response_str) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("Error parsing JSON: {}", err);
            return Vec::new();
        }
    };

    let data_field = match json_value.get("data") {
        Some(field) => field,
        None => {
            eprintln!("Expected a \"data\" field");
            return Vec::new();
        }
    };

    let data_array = match data_field.as_array() {
        Some(array) => array,
        None => {
            eprintln!("Expected an array in \"data\" field");
            return Vec::new();
        }
    };

    let mut proxies = Vec::new();

    for element in data_array {
        let ip = match element.get("ip").and_then(|v| v.as_str()) {
            Some(ip_str) => ip_str,
            None => {
                eprintln!("Missing or invalid \"ip\" field in element");
                continue;
            }
        };

        let port = match element.get("port").and_then(|v| v.as_str()) {
            Some(port_str) => port_str,
            None => {
                eprintln!("Missing or invalid \"port\" field in element");
                continue;
            }
        };

        match Proxy::new(ip, port) {
            Ok(proxy) => proxies.push(proxy),
            Err(err) => eprintln!("Failed to create Proxy: {}", err),
        }
    }

    proxies
}

pub fn parse_proxies_from_response(response_str: &str, pandas_parser: &PandasParser) -> Vec<Proxy> {
    let document = Html::parse_document(response_str);
    let table_selector = Selector::parse("table").expect("Invalid table selector");
    let row_selector = Selector::parse("tr:not(:first-child)").expect("Invalid row selector");
    let cell_selector = Selector::parse("td").expect("Invalid td selector");

    let table = match document.select(&table_selector).next() {
        Some(table) => table,
        None => {
            eprintln!("Table not found");
            return Vec::new();
        }
    };

    let combined_column = pandas_parser.combined.is_some();

    table
        .select(&row_selector)
        .filter_map(|row| {
            let cells: Vec<String> = row
                .select(&cell_selector)
                .map(|cell| cell.text().collect::<String>().trim().to_string())
                .collect();

            if combined_column {
                let first_cell = cells.get(0)?;
                let mut parts = first_cell.split(':');
                let ip = parts.next()?.trim();
                let port = parts.next()?.trim();
                Proxy::new(ip, port).ok()
            } else if cells.len() >= 2 {
                Proxy::new(&cells[0], &cells[1]).ok()
            } else {
                None
            }
        })
        .collect()
}

pub async fn batch_test_proxies(proxy_list: Vec<Proxy>, semaphore: Arc<Semaphore>) -> Vec<Proxy> {
    println!("batch_test_proxies: testing {} proxies", proxy_list.len());
    let mut handles = Vec::with_capacity(proxy_list.len());

    for proxy in proxy_list {
        let permit = match semaphore.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };

        handles.push(tokio::spawn(async move {
            let _permit = permit;
            if proxy.test_proxy().await {
                Some(proxy)
            } else {
                None
            }
        }));
    }

    let mut tested_proxies = Vec::new();
    for handle in handles {
        if let Ok(Some(proxy)) = handle.await {
            tested_proxies.push(proxy);
        }
    }

    tested_proxies
}

pub fn process_response_and_update_proxies(response_str: &str, parser: &Parser) -> Vec<Proxy> {
    if let Some(pandas_parser) = parser.pandas.as_ref() {
        parse_proxies_from_response(response_str, pandas_parser)
    } else if parser.json.is_some() {
        parse_proxies_json(response_str)
    } else if parser.txt.is_some() {
        parse_proxies_txt(response_str)
    } else {
        eprintln!("Unknown or unsupported parser configuration");
        Vec::new()
    }
}

pub async fn get_proxies() -> Vec<Proxy> {
    let test_concurrent_limit = 100;

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to build HTTP client");

    let test_semaphore = Arc::new(Semaphore::new(test_concurrent_limit));

    // Include the file as bytes
    let file_bytes: &'static [u8] = include_bytes!("../assets/sources.json");

    let json_content = String::from_utf8_lossy(file_bytes);
    let proxysources: Vec<Source> =
        serde_json::from_str(&json_content).expect("Failed to parse JSON");

    println!("Starting get_proxies with {} sources", proxysources.len());
    debug_println!("Initial number of proxy sources: {}", proxysources.len());

    let mut tasks = Vec::with_capacity(proxysources.len());
    for source in proxysources {
        let client = client.clone();
        let test_semaphore = test_semaphore.clone();
        println!("Spawning task for source {}", source.id);
        tasks.push(tokio::spawn(async move {
            println!("[{}] request start", source.id);
            let response = make_request(&client, &source.url, &source.method).await;
            let response_str = match response {
                Ok(result) => {
                    println!("[{}] request success ({} bytes)", source.id, result.len());
                    result
                }
                Err(err) => {
                    eprintln!("[{}] Error requesting {}: {}", source.id, source.url, err);
                    return Vec::new();
                }
            };

            let proxies = process_response_and_update_proxies(&response_str, &source.parser)
                .into_iter()
                .map(|proxy| proxy.with_source(source.id.clone()))
                .collect::<Vec<_>>();
            println!("[{}] parsed {} proxies", source.id, proxies.len());
            let tested_proxies = batch_test_proxies(proxies, test_semaphore.clone()).await;
            println!("[{}] tested {} proxies", source.id, tested_proxies.len());
            tested_proxies
        }));
    }

    let mut proxy_list = Vec::new();
    for task in tasks {
        if let Ok(result) = task.await {
            proxy_list.extend(result);
        } else {
            eprintln!("A proxy source task panicked");
        }
    }

    // Remove duplicates based on ip:port combination
    let mut seen = HashSet::new();
    let deduplicated_proxies = proxy_list
        .into_iter()
        .filter(|proxy| {
            let key = format!("{}:{}", proxy.ip, proxy.port);
            seen.insert(key)
        })
        .collect::<Vec<_>>();

    debug_println!("Total proxies after deduplication: {}", deduplicated_proxies.len());
    deduplicated_proxies
}
