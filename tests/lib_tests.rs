use ester::{
    parse_proxies_json, parse_proxies_txt, process_response_and_update_proxies, JsonParser,
    PandasParser, Parser, Proxy, TxtParser,
};

#[test]
fn proxy_new_and_to_url() {
    let proxy = Proxy::new("1.2.3.4", "8080").expect("Proxy parsing failed");
    assert_eq!(proxy.ip, "1.2.3.4");
    assert_eq!(proxy.port, 8080);
    assert_eq!(proxy.to_url(), "http://1.2.3.4:8080");
}

#[test]
fn proxy_new_rejects_invalid_port() {
    assert!(Proxy::new("1.2.3.4", "notaport").is_err());
}

#[test]
fn parse_proxies_txt_parses_valid_lines() {
    let input = "1.1.1.1:80\ninvalid\n2.2.2.2:8080\n";
    let proxies = parse_proxies_txt(input);
    assert_eq!(proxies.len(), 2);
    assert_eq!(proxies[0].ip, "1.1.1.1");
    assert_eq!(proxies[0].port, 80);
    assert_eq!(proxies[1].ip, "2.2.2.2");
    assert_eq!(proxies[1].port, 8080);
}

#[test]
fn parse_proxies_json_returns_proxies() {
    let input = r#"
    {
        "data": [
            {"ip": "3.3.3.3", "port": "3128"},
            {"ip": "4.4.4.4", "port": "8080"}
        ]
    }
    "#;
    let proxies = parse_proxies_json(input);
    assert_eq!(proxies.len(), 2);
    assert_eq!(proxies[0].ip, "3.3.3.3");
    assert_eq!(proxies[0].port, 3128);
    assert_eq!(proxies[1].ip, "4.4.4.4");
    assert_eq!(proxies[1].port, 8080);
}

#[test]
fn parse_proxies_json_missing_data_returns_empty() {
    let input = r#"{"foo": []}"#;
    let proxies = parse_proxies_json(input);
    assert!(proxies.is_empty());
}

#[test]
fn parse_proxies_from_response_table_columns() {
    let html = r#"
        <html>
            <body>
                <table>
                    <tr><th>IP Address</th><th>Port</th></tr>
                    <tr><td>5.5.5.5</td><td>3128</td></tr>
                    <tr><td>6.6.6.6</td><td>8080</td></tr>
                </table>
            </body>
        </html>
    "#;

    let parser = Parser {
        pandas: Some(PandasParser {
            table_index: 0,
            ip: Some("IP Address".to_string()),
            port: Some("Port".to_string()),
            combined: None,
        }),
        json: None,
        txt: None,
    };

    let proxies = process_response_and_update_proxies(html, &parser);
    assert_eq!(proxies.len(), 2);
    assert_eq!(proxies[0].ip, "5.5.5.5");
    assert_eq!(proxies[0].port, 3128);
    assert_eq!(proxies[1].ip, "6.6.6.6");
    assert_eq!(proxies[1].port, 8080);
}

#[test]
fn process_response_and_update_proxies_dispatches_correct_parser() {
    let html = r#"
        <html>
            <body>
                <table>
                    <tr><th>IP Address</th><th>Port</th></tr>
                    <tr><td>7.7.7.7</td><td>3128</td></tr>
                </table>
            </body>
        </html>
    "#;

    let parser = Parser {
        pandas: Some(PandasParser {
            table_index: 0,
            ip: Some("IP Address".to_string()),
            port: Some("Port".to_string()),
            combined: None,
        }),
        json: None,
        txt: None,
    };

    let proxies = process_response_and_update_proxies(html, &parser);
    assert_eq!(proxies.len(), 1);
    assert_eq!(proxies[0].ip, "7.7.7.7");
}
#[test]
fn parse_proxies_txt_ignores_invalid_and_blank_lines() {
    let input = "\n1.1.1.1:80\n  \nnot-a-proxy\n2.2.2.2:8080\n";
    let proxies = parse_proxies_txt(input);
    assert_eq!(proxies.len(), 2);
    assert_eq!(proxies[0].ip, "1.1.1.1");
    assert_eq!(proxies[1].ip, "2.2.2.2");
}

#[test]
fn parse_proxies_json_invalid_returns_empty() {
    let proxies = parse_proxies_json("{ invalid json }");
    assert!(proxies.is_empty());
}

#[test]
fn process_response_and_update_proxies_handles_txt_parser() {
    let parser = Parser {
        pandas: None,
        json: None,
        txt: Some(TxtParser {}),
    };

    let input = "8.8.8.8:8080\n9.9.9.9:3128\n";
    let proxies = process_response_and_update_proxies(input, &parser);
    assert_eq!(proxies.len(), 2);
    assert_eq!(proxies[0].ip, "8.8.8.8");
    assert_eq!(proxies[1].port, 3128);
}

#[test]
fn process_response_and_update_proxies_handles_json_parser() {
    let parser = Parser {
        pandas: None,
        json: Some(JsonParser {
            data: "data".to_string(),
            ip: "ip".to_string(),
            port: "port".to_string(),
        }),
        txt: None,
    };

    let input = r#"{"data":[{"ip":"10.10.10.10","port":"8080"}]}"#;
    let proxies = process_response_and_update_proxies(input, &parser);
    assert_eq!(proxies.len(), 1);
    assert_eq!(proxies[0].ip, "10.10.10.10");
}

#[test]
fn parse_proxies_from_response_combined_column() {
    let html = r#"
        <html>
            <body>
                <table>
                    <tr><th>Proxy</th></tr>
                    <tr><td>11.11.11.11:3128</td></tr>
                </table>
            </body>
        </html>
    "#;

    let parser = Parser {
        pandas: Some(PandasParser {
            table_index: 0,
            ip: None,
            port: None,
            combined: Some("Proxy".to_string()),
        }),
        json: None,
        txt: None,
    };

    let proxies = process_response_and_update_proxies(html, &parser);
    assert_eq!(proxies.len(), 1);
    assert_eq!(proxies[0].ip, "11.11.11.11");
    assert_eq!(proxies[0].port, 3128);
}

#[test]
fn process_response_and_update_proxies_no_parser_returns_empty() {
    let parser = Parser {
        pandas: None,
        json: None,
        txt: None,
    };
    let proxies = process_response_and_update_proxies("ignored", &parser);
    assert!(proxies.is_empty());
}
