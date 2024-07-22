use serde_json;
use std::env;
use std::fs::File;
use std::io::Read;
use serde::Serialize;
use serde::Deserialize;
use serde_json::Value;
use reqwest::{Client, Proxy as RequestProxy, Method};
extern crate scraper;
use scraper::{Html, Selector};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use std::io;

#[derive(Debug, Deserialize, Serialize)]
struct Parser {
    pandas: Option<PandasParser>,
    json: Option<JsonParser>,
    txt: Option<TxtParser>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct PandasParser {
    table_index: u32,
    ip: Option<String>,
    port: Option<String>,
    combined: Option<String>
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct JsonParser {
    data: String,
    ip: String,
    port: String,
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct TxtParser {}

#[derive(Serialize, Deserialize, Debug)]
struct Source {
    id: String,
    url: String,
    method: String,
    parser: Parser
}

// Function to make an HTTP request
async fn make_request(url: String, method: String, timeout_seconds: u64) -> Result<String, io::Error> {
    // Parse the method string to reqwest::Method
    let req_method = match method.to_uppercase().as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        _ => {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Unsupported HTTP method"));
        }
    };

    // Create a reqwest client with timeout
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    // Make the HTTP request
    let response = client
        .request(req_method.clone(), &url)
        .send()
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    // Check if the request was successful
    if !response.status().is_success() {
        return Err(io::Error::new(io::ErrorKind::Other, format!("Request failed with status code: {}", response.status())));
    }

    // Read the response body as a string
    let body = response
        .text()
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(body)
}

#[derive(Debug, Clone)] 
struct Proxy {
    ip: String,
    port: u16,
}

impl Proxy {
    fn new(ip: &str, port: &str) -> Result<Self, std::num::ParseIntError> {
        let parsed_port: u16 = port.parse()?;
        Ok(Self {
            ip: ip.to_string(),
            port: parsed_port,
        })
    }

    async fn test_proxy(&self) -> bool {
        let proxy_url = format!("http://{}:{}", self.ip, self.port);

        // Create a reqwest client with proxy settings
        let client = match reqwest::Client::builder()
            .proxy(RequestProxy::all(&proxy_url).expect("Invalid proxy URL"))
            .build()
        {
            Ok(client) => client,
            Err(_) => return false,
        };

        // Make a GET request using the client with a timeout
        let response = match timeout(Duration::from_secs(10), client.get("https://steamcommunity.com/market/").send()).await {
            Ok(response) => response,
            Err(_) => return false, // Timeout or other error
        };

        // Check if the response status is successful
        if let Ok(response) = response {
            response.status().is_success()
        } else {
            false // Timeout occurred
        }
    }

}
fn parse_proxies_txt(input: &str) -> Vec<Proxy> {
    input
        .lines()
        .filter_map(|line| {
            // Split each line into parts using ':' as the delimiter
            let parts: Vec<&str> = line.trim().split(':').collect();
            // Check if there are exactly two parts (IP and port)
            if parts.len() == 2 {
                // Attempt to create a Proxy instance from the parts
                match Proxy::new(parts[0], parts[1]) {
                    // If successful, return Some(proxy)
                    Ok(proxy) => Some(proxy),
                    // If parsing fails, return None
                    Err(_) => None,
                }
            } else {
                // If there are not exactly two parts, return None
                None
            }
        })
        .collect()
}

fn parse_proxies_json(response_str: &str) -> Vec<Proxy> {
    // Parse JSON string into a serde_json::Value
    let json_value: Value = match serde_json::from_str(response_str) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("Error parsing JSON: {}", err);
            return Vec::new(); // Return empty vector if parsing fails
        }
    };

    // Accessing the "data" field
    let data_field = match json_value.get("data") {
        Some(field) => field,
        None => {
            eprintln!("Expected a \"data\" field");
            return Vec::new(); // Return empty vector if "data" field is missing
        }
    };

    // Check if "data" is an array
    let data_array = match data_field.as_array() {
        Some(array) => array,
        None => {
            eprintln!("Expected an array in \"data\" field");
            return Vec::new(); // Return empty vector if "data" field is not an array
        }
    };

    // Initialize a vector to hold proxies
    let mut proxies = Vec::new();

    // Iterate over elements in the array
    for element in data_array {
        // Access "ip" and "port" fields from each element
        let ip = match element.get("ip").and_then(|v| v.as_str()) {
            Some(ip_str) => ip_str,
            None => {
                eprintln!("Missing or invalid \"ip\" field in element");
                continue; // Skip this element if "ip" field is missing or invalid
            }
        };
        
        let port = match element.get("port").and_then(|v| v.as_str()) {
            Some(port_str) => port_str,
            None => {
                eprintln!("Missing or invalid \"port\" field in element");
                continue; // Skip this element if "port" field is missing or invalid
            }
        };

        // Attempt to create a Proxy instance from the parts
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

    // Return the vector of proxies
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

fn process_response_and_update_proxies(response_str: &str, parser: &Parser) -> Vec<Proxy> {
    match parser{
        Parser {
            pandas: Some(pandas_parser),
            json: None,
            txt: None,
        } => {
            // println!("PandasParser found: {:?}", pandas_parser);
            // Handle PandasParser case
            parse_proxies_from_response(&response_str, &pandas_parser)
        }
        Parser {
            pandas: None,
            json: Some(_json_parser),
            txt: None,
        } => {
            // println!("JsonParser found: {:?}", json_parser);
            // Handle JsonParser case
            parse_proxies_json(&response_str)
        }
        Parser {
            pandas: None,
            json: None,
            txt: Some(_txt_parser),
        } => {
            // println!("TxtParser found: {:?}", txt_parser);
            // Handle TxtParser case
            parse_proxies_txt(&response_str)
        }
        _ => {
            println!("Unknown or unsupported parser configuration");
            Vec::new() // Return an empty vector or handle unsupported case
        }
    }

}

async fn batch_test_proxies(proxy_list: Vec<Arc<Proxy>>, concurrent_limit: usize) -> Vec<Proxy> {
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
        tested_proxies.push(result); // Push Proxy directly
    }

    tested_proxies
}

async fn load_and_test_proxies_from_file(file_path: &str, concurrent_limit: usize) -> Vec<Proxy> {
    // Read JSON file
    let mut file = File::open(file_path).expect("Failed to open file");
    let mut json_content = String::new();
    file.read_to_string(&mut json_content).expect("Failed to read file content");

    // Parse JSON content into Vec<Source>
    let proxysources: Vec<Source> = serde_json::from_str(&json_content).expect("Failed to parse JSON");

    // Print initial count of proxies
    println!("Initial number of proxies: {}", proxysources.len());

    // Create tasks to process each source concurrently
    let mut tasks = vec![];
    for source in proxysources {
        let task = tokio::spawn(async move {
            let url = source.url.to_string();
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
            println!("Initial length: {}, link:{}", arc_proxies.len(), url);
            let tested_proxies = batch_test_proxies(arc_proxies, concurrent_limit).await;
            println!("Final length: {}, link:{}", tested_proxies.len(), url);
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

    println!("Total proxies tested: {}", proxy_list.len());
    proxy_list
}

#[tokio::main]
async fn main() {
    // Adjust the file path as needed
    let file_path = "src\\sources.json";
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let file_path = current_dir.join(file_path);

    let proxy_list = load_and_test_proxies_from_file(file_path.to_str().unwrap(), 50).await;
    println!("Final list of proxies: {:?}", proxy_list);

    // let arc_proxies: Vec<Arc<Proxy>> = proxy_list.into_iter().map(Arc::new).collect();
    // test_proxies_speed(arc_proxies, url, method).await;

    // Print the IP and port for each parsed proxy
    // for proxy in &proxy_list {
    //     println!("IP: {}, Port: {}", proxy.ip, proxy.port);
    // }
    print!("done")

}
