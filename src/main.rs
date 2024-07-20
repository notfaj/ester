use serde_json;
use std::fs::File;
use std::io::Read;
use serde::Serialize;
use serde::Deserialize;
use serde_json::Value;

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

use reqwest::{Client, Method};

// Function to make an HTTP request
async fn make_request(url: String, method: String) -> Result<String, reqwest::Error> {
    // Parse the method string to reqwest::Method
    let req_method = match method.to_uppercase().as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        _ => todo!()
    };

    // Create a reqwest client
    let client = Client::new();

    // Make the HTTP request
    let response = client.request(req_method, url).send().await?;

    // Read the response body as a string
    let body = response.text().await?;
    return Ok(body)
    
}

#[derive(Debug)] 
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

// Function to process JSON response string and create proxies
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

#[tokio::main]
async fn main() {

    let file_name = "src\\sources.json";
    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    let file_path = current_dir.join(file_name);
    println!("{:#?}", file_path);
    let mut file = File::open(&file_path).expect("Failed to open file");

    // Read the file content into a string
    let mut json_content = String::new();
    file.read_to_string(&mut json_content)
        .expect("Failed to read file content");

    // Parse the JSON content into a Person struct
    let proxysource: Vec<Source> = serde_json::from_str(&json_content).expect("Failed to parse JSON");
    // Create an empty vector to hold proxies
    let mut proxy_list: Vec<Proxy> = Vec::new();
    //Print the parsed data
    for source in proxysource {
        // println!("Source ID: {}, URL: {}, Parser: {:#?}", source.id, source.url, source.parser);
        let response: Result<String, reqwest::Error> = make_request(source.url, source.method).await;
        let response_str: String = match response{
                Ok(result) => result,
                Err(err) => {
                    eprintln!("Error: {}", err);
                    continue;
                }
            };
        if source.parser.txt.is_none() && source.parser.json.is_none(){
            // type panda
            println!("panda");
        } else if source.parser.pandas.is_none() && source.parser.json.is_none() {
            // txt
            println!("txt");
            let proxies: Vec<Proxy> = parse_proxies_txt(&response_str);
            proxy_list.extend(proxies)
        } else if source.parser.pandas.is_none() && source.parser.txt.is_none() {
            // json 
            println!("json");
            let proxies: Vec<Proxy> = parse_proxies_json(&response_str);
            proxy_list.extend(proxies)
            
            
        } else {
            println!("no type");

        }
        
    }
    // Print the IP and port for each parsed proxy
    for proxy in &proxy_list {
        println!("IP: {}, Port: {}", proxy.ip, proxy.port);
    }

    // Get the number of elements in the vector
    let num_objects = &proxy_list.len();
    // Print the number of objects
    println!("Number of objects in the vector: {}", num_objects);

}
