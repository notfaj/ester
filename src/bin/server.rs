use ester::{get_proxies, test_proxy_timed, Proxy};
use rand::seq::SliceRandom;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, Semaphore};

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct ServerCfg {
    host: String,
    port: u16,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct FilteringCfg {
    speed_threshold_ms: u64,
    test_url: String,
    check_timeout: u64,
    concurrent_checks: usize,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct RotationCfg {
    strategy: String,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct HealthCfg {
    max_failures: u32,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct RecheckCfg {
    interval: u64,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct Config {
    server: ServerCfg,
    filtering: FilteringCfg,
    rotation: RotationCfg,
    health: HealthCfg,
    recheck: RecheckCfg,
}

#[derive(Debug, Clone)]
struct ProxyEntry {
    proxy: Proxy,
    failures: u32,
    latency_ms: u64,
}

struct PoolState {
    entries: Vec<ProxyEntry>,
    rr_index: usize,
}

type SharedPool = Arc<RwLock<PoolState>>;
type SharedConfig = Arc<RwLock<Config>>;

fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(path)?;
    let cfg: Config = serde_yaml::from_str(&contents)?;
    Ok(cfg)
}

async fn validate_pool(proxies: Vec<Proxy>, cfg: Config) -> Vec<ProxyEntry> {
    let sem = Arc::new(Semaphore::new(cfg.filtering.concurrent_checks));
    let mut handles = Vec::with_capacity(proxies.len());

    for proxy in proxies {
        let permit = match sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => break,
        };
        let test_url = cfg.filtering.test_url.clone();
        let timeout = cfg.filtering.check_timeout;
        let threshold = cfg.filtering.speed_threshold_ms;

        handles.push(tokio::spawn(async move {
            let _p = permit;
            let latency = test_proxy_timed(&proxy, &test_url, timeout).await?;
            if latency <= threshold {
                Some(ProxyEntry {
                    proxy,
                    failures: 0,
                    latency_ms: latency,
                })
            } else {
                None
            }
        }));
    }

    let mut entries = Vec::new();
    for h in handles {
        if let Ok(Some(e)) = h.await {
            entries.push(e);
        }
    }
    entries.sort_by_key(|e| e.latency_ms);
    entries
}

async fn refresh_pool(pool: SharedPool, cfg: SharedConfig) {
    println!("[refresh] scraping proxies...");
    let scraped = get_proxies().await;
    println!("[refresh] scraped {} proxies, validating...", scraped.len());

    let cfg_snapshot = cfg.read().await.clone();
    let entries = validate_pool(scraped, cfg_snapshot).await;

    let count = entries.len();
    {
        let mut p = pool.write().await;
        p.entries = entries;
        p.rr_index = 0;
    }
    println!("[refresh] Pool refreshed: {} proxies active", count);
}

async fn recheck_loop(pool: SharedPool, cfg: SharedConfig) {
    loop {
        let mut elapsed = 0u64;
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            elapsed += 10;
            let interval = cfg.read().await.recheck.interval;
            if elapsed >= interval {
                break;
            }
        }
        refresh_pool(pool.clone(), cfg.clone()).await;
    }
}

async fn config_watcher_loop(cfg: SharedConfig, path: String) {
    let mut last_mtime: Option<SystemTime> = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok());

    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let current_mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if Some(current_mtime) == last_mtime {
            continue;
        }
        last_mtime = Some(current_mtime);

        let new_cfg = match load_config(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[config] reload failed: {}", e);
                continue;
            }
        };

        let old_cfg = cfg.read().await.clone();
        if new_cfg.server != old_cfg.server {
            eprintln!("[config] WARNING: server.host/port changes ignored (cannot rebind live listener)");
        }

        let mut changes = Vec::new();
        if new_cfg.filtering != old_cfg.filtering {
            changes.push("filtering");
        }
        if new_cfg.rotation != old_cfg.rotation {
            changes.push("rotation");
        }
        if new_cfg.health != old_cfg.health {
            changes.push("health");
        }
        if new_cfg.recheck != old_cfg.recheck {
            changes.push("recheck");
        }

        *cfg.write().await = new_cfg;
        println!("[config] Config reloaded. Changed sections: {:?}", changes);
    }
}

async fn get_next_proxy(pool: &SharedPool, strategy: &str) -> Option<Proxy> {
    let mut p = pool.write().await;
    if p.entries.is_empty() {
        return None;
    }
    if strategy == "random" {
        let mut rng = rand::thread_rng();
        p.entries.choose(&mut rng).map(|e| e.proxy.clone())
    } else {
        let idx = p.rr_index % p.entries.len();
        p.rr_index = (p.rr_index + 1) % p.entries.len();
        Some(p.entries[idx].proxy.clone())
    }
}

async fn record_failure(pool: &SharedPool, addr: &str, max_failures: u32) {
    let mut p = pool.write().await;
    if let Some(idx) = p
        .entries
        .iter()
        .position(|e| format!("{}:{}", e.proxy.ip, e.proxy.port) == addr)
    {
        p.entries[idx].failures += 1;
        if p.entries[idx].failures >= max_failures {
            let ejected = p.entries.remove(idx);
            println!(
                "[health] Ejected {}:{} after {} failures",
                ejected.proxy.ip, ejected.proxy.port, ejected.failures
            );
        }
    }
}

async fn record_success(pool: &SharedPool, addr: &str) {
    let mut p = pool.write().await;
    if let Some(idx) = p
        .entries
        .iter()
        .position(|e| format!("{}:{}", e.proxy.ip, e.proxy.port) == addr)
    {
        p.entries[idx].failures = 0;
    }
}

async fn read_until_headers_end(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 65536 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "request headers too large",
            ));
        }
    }
    Ok(buf)
}

async fn handle_connect(
    mut client: TcpStream,
    target: String,
    pool: SharedPool,
    cfg: SharedConfig,
) {
    let (strategy, max_failures) = {
        let c = cfg.read().await;
        (c.rotation.strategy.clone(), c.health.max_failures)
    };

    let mut upstream: Option<(TcpStream, String)> = None;
    for _ in 0..3 {
        let proxy = match get_next_proxy(&pool, &strategy).await {
            Some(p) => p,
            None => break,
        };
        let proxy_addr = format!("{}:{}", proxy.ip, proxy.port);

        let mut up = match TcpStream::connect(&proxy_addr).await {
            Ok(s) => s,
            Err(_) => {
                record_failure(&pool, &proxy_addr, max_failures).await;
                continue;
            }
        };

        let req = format!(
            "CONNECT {} HTTP/1.1\r\nHost: {}\r\n\r\n",
            target, target
        );
        if up.write_all(req.as_bytes()).await.is_err() {
            record_failure(&pool, &proxy_addr, max_failures).await;
            continue;
        }

        let mut resp = Vec::with_capacity(1024);
        let mut tmp = [0u8; 1024];
        let mut got_headers = false;
        for _ in 0..8 {
            match up.read(&mut tmp).await {
                Ok(0) => break,
                Ok(n) => {
                    resp.extend_from_slice(&tmp[..n]);
                    if resp.windows(4).any(|w| w == b"\r\n\r\n") {
                        got_headers = true;
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        if got_headers
            && (resp.starts_with(b"HTTP/1.1 200") || resp.starts_with(b"HTTP/1.0 200"))
        {
            upstream = Some((up, proxy_addr));
            break;
        } else {
            record_failure(&pool, &proxy_addr, max_failures).await;
        }
    }

    let (mut up, proxy_addr) = match upstream {
        Some(x) => x,
        None => {
            let _ = client
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
                .await;
            return;
        }
    };

    if client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .is_err()
    {
        return;
    }

    match tokio::io::copy_bidirectional(&mut client, &mut up).await {
        Ok(_) => record_success(&pool, &proxy_addr).await,
        Err(_) => record_failure(&pool, &proxy_addr, max_failures).await,
    }
}

fn force_connection_close(request_buf: &[u8]) -> Vec<u8> {
    let header_end = match request_buf.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(i) => i,
        None => return request_buf.to_vec(),
    };
    let headers_str = match std::str::from_utf8(&request_buf[..header_end]) {
        Ok(s) => s,
        Err(_) => return request_buf.to_vec(),
    };
    let body = &request_buf[header_end..];

    let mut found = false;
    let new_headers: Vec<String> = headers_str
        .split("\r\n")
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("connection:") || lower.starts_with("proxy-connection:") {
                found = true;
                "Connection: close".to_string()
            } else {
                line.to_string()
            }
        })
        .collect();

    let mut result = if found {
        new_headers.join("\r\n").into_bytes()
    } else {
        let mut h = new_headers.join("\r\n").into_bytes();
        h.extend_from_slice(b"\r\nConnection: close");
        h
    };
    result.extend_from_slice(body);
    result
}

async fn handle_http(
    mut client: TcpStream,
    request_buf: Vec<u8>,
    pool: SharedPool,
    cfg: SharedConfig,
) {
    let (strategy, max_failures) = {
        let c = cfg.read().await;
        (c.rotation.strategy.clone(), c.health.max_failures)
    };

    let proxy = match get_next_proxy(&pool, &strategy).await {
        Some(p) => p,
        None => {
            let _ = client
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\n\r\n")
                .await;
            return;
        }
    };
    let proxy_addr = format!("{}:{}", proxy.ip, proxy.port);

    let up = match TcpStream::connect(&proxy_addr).await {
        Ok(s) => s,
        Err(_) => {
            record_failure(&pool, &proxy_addr, max_failures).await;
            let _ = client
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
                .await;
            return;
        }
    };

    let (mut up_read, mut up_write) = up.into_split();
    let modified = force_connection_close(&request_buf);

    if up_write.write_all(&modified).await.is_err() {
        record_failure(&pool, &proxy_addr, max_failures).await;
        return;
    }
    drop(up_write); // half-close: signal we're done sending

    match tokio::io::copy(&mut up_read, &mut client).await {
        Ok(_) => record_success(&pool, &proxy_addr).await,
        Err(_) => record_failure(&pool, &proxy_addr, max_failures).await,
    }
}

async fn handle_client(mut stream: TcpStream, pool: SharedPool, cfg: SharedConfig) {
    let buf = match read_until_headers_end(&mut stream).await {
        Ok(b) if !b.is_empty() => b,
        _ => return,
    };

    let first_line_end = match buf.windows(2).position(|w| w == b"\r\n") {
        Some(i) => i,
        None => return,
    };
    let first_line = match std::str::from_utf8(&buf[..first_line_end]) {
        Ok(s) => s,
        Err(_) => return,
    };

    let mut parts = first_line.split_whitespace();
    let method = match parts.next() {
        Some(m) => m,
        None => return,
    };
    let target = match parts.next() {
        Some(t) => t.to_string(),
        None => return,
    };

    if method.eq_ignore_ascii_case("CONNECT") {
        handle_connect(stream, target, pool, cfg).await;
    } else {
        handle_http(stream, buf, pool, cfg).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg_path = std::env::args().nth(1).unwrap_or_else(|| "config.yaml".to_string());
    let cfg = load_config(&cfg_path)?;
    let bind_addr = format!("{}:{}", cfg.server.host, cfg.server.port);
    let cfg: SharedConfig = Arc::new(RwLock::new(cfg));

    let pool: SharedPool = Arc::new(RwLock::new(PoolState {
        entries: Vec::new(),
        rr_index: 0,
    }));

    refresh_pool(pool.clone(), cfg.clone()).await;

    tokio::spawn(config_watcher_loop(cfg.clone(), cfg_path));
    tokio::spawn(recheck_loop(pool.clone(), cfg.clone()));

    let listener = TcpListener::bind(&bind_addr).await?;
    println!("Listening on {}", bind_addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let pool = pool.clone();
        let cfg = cfg.clone();
        tokio::spawn(handle_client(stream, pool, cfg));
    }
}
