use crate::adblock_engine::{BlockedItem, SharedAdBlock};
use crate::dns_guard::DnsGuard;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum ProxyEvent {
    Blocked(BlockedItem),
    DnsBlocked { host: String, reason: String },
}

/// Lightweight local HTTP + HTTPS CONNECT proxy performing domain-level AdBlock and DNS Guard checks.
pub struct LocalSecurityProxy {
    pub port: u16,
    shutdown: Arc<AtomicBool>,
}

impl LocalSecurityProxy {
    pub fn start(
        adblock: SharedAdBlock,
        dns_guard: Arc<DnsGuard>,
        dns_enabled: Arc<AtomicBool>,
        events_tx: Sender<ProxyEvent>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_bg = shutdown.clone();

        thread::spawn(move || {
            while !shutdown_bg.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let ab = adblock.clone();
                        let dns = dns_guard.clone();
                        let dns_en = dns_enabled.clone();
                        let tx = events_tx.clone();
                        thread::spawn(move || {
                            let _ = handle_client(stream, ab, dns, dns_en, tx);
                        });
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { port, shutdown })
    }
}

impl Drop for LocalSecurityProxy {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

fn handle_client(
    mut client: TcpStream,
    adblock: SharedAdBlock,
    dns_guard: Arc<DnsGuard>,
    dns_enabled: Arc<AtomicBool>,
    events_tx: Sender<ProxyEvent>,
) -> io::Result<()> {
    client.set_nonblocking(false)?;
    client.set_read_timeout(Some(Duration::from_secs(30)))?;

    let mut reader = BufReader::new(client.try_clone()?);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line)? == 0 {
        return Ok(());
    }

    let mut header_lines = Vec::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        header_lines.push(line);
    }

    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let version = parts.next().unwrap_or("HTTP/1.1");

    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = split_host_port(target, 443);
        let check_url = format!("https://{host}/");
        if should_block_host(
            &host,
            &check_url,
            "https",
            &adblock,
            &dns_guard,
            dns_enabled.load(Ordering::Relaxed),
            &events_tx,
        ) {
            client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")?;
            return Ok(());
        }

        let Ok(mut upstream) = TcpStream::connect((host.as_str(), port)) else {
            client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")?;
            return Ok(());
        };
        let _ = client.set_nodelay(true);
        let _ = upstream.set_nodelay(true);
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        client.set_read_timeout(None)?;
        let _ = relay_bidirectional(client, &mut upstream);
        return Ok(());
    }

    // Plain HTTP request forwarding
    let parsed_url = if target.starts_with("http://") || target.starts_with("https://") {
        target.to_string()
    } else {
        let host_hdr = header_lines
            .iter()
            .find_map(|h| {
                let (k, v) = h.split_once(':')?;
                if k.trim().eq_ignore_ascii_case("host") {
                    Some(v.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        format!("http://{host_hdr}{target}")
    };

    let Ok(u) = url::Url::parse(&parsed_url) else {
        client.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")?;
        return Ok(());
    };
    let host = u.host_str().unwrap_or("").to_string();
    let port = u.port_or_known_default().unwrap_or(80);
    if should_block_host(
        &host,
        &parsed_url,
        "http",
        &adblock,
        &dns_guard,
        dns_enabled.load(Ordering::Relaxed),
        &events_tx,
    ) {
        client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")?;
        return Ok(());
    }

    let Ok(mut upstream) = TcpStream::connect((host.as_str(), port)) else {
        client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")?;
        return Ok(());
    };
    let _ = client.set_nodelay(true);
    let _ = upstream.set_nodelay(true);

    let path_and_query = match u.query() {
        Some(q) => format!("{}?{}", u.path(), q),
        None => u.path().to_string(),
    };
    let mut req_buf = format!("{method} {path_and_query} {version}\r\n");
    for h in &header_lines {
        if !h.to_ascii_lowercase().starts_with("proxy-connection:") {
            req_buf.push_str(h);
        }
    }
    req_buf.push_str("\r\n");
    upstream.write_all(req_buf.as_bytes())?;

    // Flush any buffered bytes from BufReader before bidirectional relay
    let buffered = reader.buffer();
    if !buffered.is_empty() {
        upstream.write_all(buffered)?;
    }
    client.set_read_timeout(None)?;
    let _ = relay_bidirectional(client, &mut upstream);
    Ok(())
}

fn should_block_host(
    host: &str,
    check_url: &str,
    rtype: &str,
    adblock: &SharedAdBlock,
    dns_guard: &DnsGuard,
    dns_on: bool,
    events_tx: &Sender<ProxyEvent>,
) -> bool {
    if host.is_empty() || DnsGuard::is_local_or_ip(host) {
        return false;
    }
    if let Ok(ab) = adblock.read() {
        let outcome = ab.should_block(check_url, "", rtype);
        if outcome.blocked {
            let _ = events_tx.send(ProxyEvent::Blocked(BlockedItem {
                resource_type: rtype.to_string(),
                request_url: check_url.to_string(),
                rule: outcome.rule,
            }));
            return true;
        }
    }
    if dns_on {
        let dns_res = dns_guard.check_host(host, true);
        if !dns_res.safe {
            let _ = events_tx.send(ProxyEvent::DnsBlocked {
                host: host.to_string(),
                reason: dns_res.reason,
            });
            return true;
        }
    }
    false
}

fn split_host_port(target: &str, default_port: u16) -> (String, u16) {
    if let Some((h, p)) = target.rsplit_once(':') {
        if let Ok(port) = p.parse::<u16>() {
            return (h.trim_matches(&['[', ']'][..]).to_string(), port);
        }
    }
    (target.to_string(), default_port)
}

fn relay_bidirectional(mut client: TcpStream, upstream: &mut TcpStream) -> io::Result<()> {
    let mut c_read = client.try_clone()?;
    let mut u_write = upstream.try_clone()?;
    let c2u = thread::spawn(move || {
        let mut buf = [0u8; 65536];
        while let Ok(n) = c_read.read(&mut buf) {
            if n == 0 || u_write.write_all(&buf[..n]).is_err() {
                break;
            }
        }
        let _ = u_write.shutdown(std::net::Shutdown::Write);
    });

    let mut buf = [0u8; 65536];
    while let Ok(n) = upstream.read(&mut buf) {
        if n == 0 || client.write_all(&buf[..n]).is_err() {
            break;
        }
    }
    let _ = client.shutdown(std::net::Shutdown::Write);
    let _ = c2u.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adblock_engine::AdBlockService;
    use std::fs;
    use std::sync::{mpsc, RwLock};

    #[test]
    fn proxy_blocks_ad_domain_connect_with_403() {
        let tmp = std::env::temp_dir().join(format!("ab_test_proxy_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let ab = Arc::new(RwLock::new(AdBlockService::load(&tmp, true, true, 0, vec![])));
        let dns = Arc::new(DnsGuard::new());
        let dns_en = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let proxy = LocalSecurityProxy::start(ab, dns, dns_en, tx).unwrap();
        let mut conn = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
        conn.write_all(b"CONNECT doubleclick.net:443 HTTP/1.1\r\nHost: doubleclick.net:443\r\n\r\n")
            .unwrap();
        let mut resp = String::new();
        let mut buf = [0u8; 256];
        let n = conn.read(&mut buf).unwrap();
        resp.push_str(&String::from_utf8_lossy(&buf[..n]));
        assert!(resp.contains("403 Forbidden"));

        let evt = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        match evt {
            ProxyEvent::Blocked(b) => assert!(b.request_url.contains("doubleclick.net")),
            _ => panic!("expected Blocked event"),
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
