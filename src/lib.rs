use serde::{Deserialize, Serialize};
use reqwest::{Client, Proxy as RequestProxy, Method};
use scraper::{Html, Selector};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use std::io;
use std::env;
use std::fs::File;
use std::io::Read;


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
    pub combined: Option<String>
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
    pub parser: Parser
}

#[derive(Debug, Clone)]
pub struct Proxy {
    pub ip: String,
    pub port: u16,
}

impl Proxy {
    pub fn new(ip: &str, port: &str) -> Result<Self, std::num::ParseIntError> {
        let parsed_port: u16 = port.parse()?;
        Ok(Self {
            ip: ip.to_string(),
            port: parsed_port,
        })
    }

    pub fn to_url(&self) -> String {
        format!("http://{}:{}", self.ip, self.port)
    }
    
    pub async fn test_proxy(&self) -> bool {
        let proxy_url = self.to_url();

        let client = match reqwest::Client::builder()
            .proxy(RequestProxy::all(&proxy_url).expect("Invalid proxy URL"))
            .build()
        {
            Ok(client) => client,
            Err(_) => return false,
        };

        let response = match timeout(Duration::from_secs(10), client.get("https://steamcommunity.com/market/").send()).await {
            Ok(response) => response,
            Err(_) => return false,
        };

        if let Ok(response) = response {
            response.status().is_success()
        } else {
            false
        }
    }
}

pub async fn make_request(url: String, method: String, timeout_seconds: u64) -> Result<String, io::Error> {
    let req_method = match method.to_uppercase().as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Unsupported HTTP method")),
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let response = client
        .request(req_method, &url)
        .send()
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    if !response.status().is_success() {
        return Err(io::Error::new(io::ErrorKind::Other, format!("Request failed with status code: {}", response.status())));
    }

    let body = response
        .text()
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(body)
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

fn parse_proxies_from_response(response_str: &str, pandas_parser: &PandasParser) -> Vec<Proxy> {
    let mut proxies: Vec<Proxy> = Vec::new();

    // Parse the HTML document
    let document = Html::parse_document(response_str);

    // Check if pandas_parser.combined is None
    if pandas_parser.combined.is_none() {
        // Select the first table element in the document
        let table_selector = Selector::parse("table").unwrap();
        let table = match document.select(&table_selector).next() {
            Some(table) => table,
            None => {
                println!("Table not found");
                return proxies; // Return empty vector or handle not found case
            }
        };

        // Iterate over each row in the table (excluding the first child)
        for row in table.select(&Selector::parse("tr:not(:first-child)").unwrap()) {
            let ip_selector = Selector::parse("td:nth-child(1)").unwrap();
            let port_selector = Selector::parse("td:nth-child(2)").unwrap();

            // Extract IP address and port number from the row
            if let Some(ip_element) = row.select(&ip_selector).next() {
                if let Some(port_element) = row.select(&port_selector).next() {
                    let ip_address = ip_element.text().collect::<Vec<_>>()[0].to_string();
                    let port_number = port_element.text().collect::<Vec<_>>()[0].to_string();

                    let ip = ip_address.trim();
                    let port = port_number.trim();

                    // Attempt to create a Proxy instance from IP and port
                    match Proxy::new(ip, port) {
                        Ok(proxy) => {
                            // Successfully created Proxy instance
                            proxies.push(proxy);
                        },
                        Err(err) => {
                            // Handle error if parsing fails
                            eprintln!("Failed to create Proxy: {}", err);
                        }
                    }
                }
            }
        }
    } else if let Some(value) = pandas_parser.combined.as_ref() {
        match value.as_str() {
            "Proxy" => {
                println!("pandas_parser.combined is 'Proxy'");
                // Handle 'Proxy' case as needed
                // Example: Some specific logic or action
            },
            _ => {
                // Handle other cases if needed
                todo!(); // Placeholder for future implementation
            },
        }
    }

    proxies // Return the vector of proxies
}

pub async fn batch_test_proxies(proxy_list: Vec<Arc<Proxy>>, concurrent_limit: usize) -> Vec<Proxy> {
    let (tx, mut rx) = mpsc::channel(concurrent_limit);

    for proxy in proxy_list {
        let tx = tx.clone();
        let proxy_clone = proxy.clone();

        tokio::spawn(async move {
            if proxy_clone.test_proxy().await {
                if let Err(_) = tx.send((*proxy_clone).clone()).await {
                    eprintln!("Error sending result to channel");
                }
            }
        });
    }

    drop(tx);

    let mut tested_proxies = Vec::new();
    while let Some(result) = rx.recv().await {
        tested_proxies.push(result);
    }

    tested_proxies
}

fn process_response_and_update_proxies(response_str: &str, parser: &Parser) -> Vec<Proxy> {
    match parser{
        Parser {
            pandas: Some(pandas_parser),
            json: None,
            txt: None,
        } => {
            // println!("PandasParser found: {:?}", pandas_parser);
            // Handle PandasParser case
            parse_proxies_from_response(response_str, pandas_parser)
        }
        Parser {
            pandas: None,
            json: Some(_json_parser),
            txt: None,
        } => {
            // println!("JsonParser found: {:?}", json_parser);
            // Handle JsonParser case
            parse_proxies_json(response_str)
        }
        Parser {
            pandas: None,
            json: None,
            txt: Some(_txt_parser),
        } => {
            // println!("TxtParser found: {:?}", txt_parser);
            // Handle TxtParser case
            parse_proxies_txt(response_str)
        }
        _ => {
            println!("Unknown or unsupported parser configuration");
            Vec::new() // Return an empty vector or handle unsupported case
        }
    }

}

pub async fn get_proxies() -> Vec<Proxy> {
    let file_path = "src\\sources.json";
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let file_path = current_dir.join(file_path);
    let concurrent_limit = 50;
    // Read JSON file
    let mut file = File::open(file_path).expect("Failed to open file");
    let mut json_content = String::new();
    file.read_to_string(&mut json_content).expect("Failed to read file content");

    // Parse JSON content into Vec<Source>
    let proxysources: Vec<Source> = serde_json::from_str(&json_content).expect("Failed to parse JSON");

    // Print initial count of proxies
    debug_println!("Initial number of proxies: {}", proxysources.len());

    // Create tasks to process each source concurrently
    let mut tasks = vec![];
    for source in proxysources {
        let task = tokio::spawn(async move {
            let _url = source.url.to_string();
            let response = make_request(source.url.clone(), source.method.clone(), 10).await;
            let response_str = match response {
                Ok(result) => result,
                Err(err) => {
                    eprintln!("Error: {}", err);
                    return vec![]; // Return empty vector if there's an error
                }
            };

            let proxies = process_response_and_update_proxies(&response_str, &source.parser);
            let arc_proxies: Vec<Arc<Proxy>> = proxies.into_iter().map(Arc::new).collect();
            debug_println!("Initial length: {}, link:{}", arc_proxies.len(), _url);
            let tested_proxies = batch_test_proxies(arc_proxies, concurrent_limit).await;
            debug_println!("Final length: {}, link:{}", tested_proxies.len(), _url);
            tested_proxies
        });
        tasks.push(task);
    }

    // Collect results from tasks
    let mut proxy_list = vec![];
    for task in tasks {
        let result = task.await.expect("Task panicked");
        proxy_list.extend(result);
    }

    debug_println!("Total proxies left: {}", proxy_list.len());
    proxy_list
}
