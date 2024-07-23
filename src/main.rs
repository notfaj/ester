use ester::get_proxies;

#[tokio::main]
async fn main() {
    let proxy_list = get_proxies().await;
    println!("Final list of proxies ({}): {:?}", proxy_list.len(), proxy_list);
}
