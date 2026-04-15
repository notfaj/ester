use ester::get_proxies;
use std::fs;
use std::io;
use std::time::Instant;

#[tokio::main]
async fn main() -> io::Result<()> {
    let start = Instant::now();
    let proxy_list = get_proxies().await;

    let output = proxy_list
        .iter()
        .map(|proxy| format!("{}:{}\n", proxy.ip, proxy.port))
        .collect::<String>();

    fs::write("proxies.txt", output)?;
    let elapsed = start.elapsed();
    println!("Wrote {} proxies to proxies.txt", proxy_list.len());
    println!("Elapsed time: {:.2?}", elapsed);

    Ok(())
}
