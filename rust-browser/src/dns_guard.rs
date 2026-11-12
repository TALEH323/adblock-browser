use serde::Deserialize;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::sync::RwLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct DnsCheckResult {
    pub host: String,
    pub safe: bool,
    pub reason: String,
    pub resolved_ips: Vec<String>,
    pub checked_at: Instant,
}

#[derive(Deserialize)]
struct DohResponse {
    #[serde(rename = "Status")]
    status: Option<i32>,
    #[serde(rename = "Comment")]
    comment: Option<Vec<String>>,
    #[serde(rename = "Answer")]
    answer: Option<Vec<DohAnswer>>,
}

#[derive(Deserialize)]
struct DohAnswer {
    data: Option<String>,
}

const SINKHOLE_IPS: &[&str] = &["0.0.0.0", "127.0.0.1", "::", "::1", "127.0.0.2"];

pub struct DnsGuard {
    cache: RwLock<HashMap<String, DnsCheckResult>>,
    ttl: Duration,
    agent: ureq::Agent,
    doh_backoff_until: RwLock<Option<Instant>>,
}

impl DnsGuard {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(800))
            .timeout(Duration::from_millis(1200))
            .build();
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(300),
            agent,
            doh_backoff_until: RwLock::new(None),
        }
    }

    pub fn is_local_or_ip(host: &str) -> bool {
        let clean = host
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_lowercase();
        if clean.is_empty()
            || clean == "localhost"
            || clean.ends_with(".localhost")
            || clean.ends_with(".local")
            || clean.ends_with(".internal")
        {
            return true;
        }
        clean.parse::<IpAddr>().is_ok()
    }

    pub fn get_cached(&self, host: &str) -> Option<DnsCheckResult> {
        let key = host.trim().to_lowercase();
        let guard = self.cache.read().ok()?;
        let entry = guard.get(&key)?;
        if entry.checked_at.elapsed() <= self.ttl {
            Some(entry.clone())
        } else {
            None
        }
    }

    pub fn check_host(&self, host: &str, enabled: bool) -> DnsCheckResult {
        let clean = host.trim().trim_matches('.').to_lowercase();
        if !enabled || clean.is_empty() || Self::is_local_or_ip(&clean) {
            return DnsCheckResult {
                host: clean,
                safe: true,
                reason: if !enabled {
                    "DNS Koruması Kapalı".to_string()
                } else {
                    "Yerel / IP adresi".to_string()
                },
                resolved_ips: Vec::new(),
                checked_at: Instant::now(),
            };
        }

        if let Some(hit) = self.get_cached(&clean) {
            return hit;
        }

        let res = self.query_doh_or_system(&clean);
        if let Ok(mut guard) = self.cache.write() {
            guard.insert(clean.clone(), res.clone());
        }
        res
    }

    fn query_doh_or_system(&self, host: &str) -> DnsCheckResult {
        let skip_doh = self
            .doh_backoff_until
            .read()
            .ok()
            .and_then(|g| *g)
            .map(|until| Instant::now() < until)
            .unwrap_or(false);

        if !skip_doh {
            let doh_url = format!(
                "https://security.cloudflare-dns.com/dns-query?name={}&type=A",
                host
            );
            match self
                .agent
                .get(&doh_url)
                .set("Accept", "application/dns-json")
                .call()
            {
                Ok(resp) => {
                    if let Ok(raw_body) = resp.into_string() {
                        if let Ok(body) = serde_json::from_str::<DohResponse>(&raw_body) {
                            let comment_text =
                                body.comment.unwrap_or_default().join(" ").to_lowercase();
                            let answers: Vec<String> = body
                                .answer
                                .unwrap_or_default()
                                .into_iter()
                                .filter_map(|a| a.data)
                                .collect();

                            if comment_text.contains("blocked") || comment_text.contains("filtered")
                            {
                                return DnsCheckResult {
                                    host: host.to_string(),
                                    safe: false,
                                    reason: "Cloudflare Security DNS bu alan adını zararlı/oltalama olarak engelledi.".to_string(),
                                    resolved_ips: answers,
                                    checked_at: Instant::now(),
                                };
                            }
                            if body.status == Some(3) {
                                return DnsCheckResult {
                                    host: host.to_string(),
                                    safe: false,
                                    reason: "Alan adı çözümlenemedi (NXDOMAIN).".to_string(),
                                    resolved_ips: Vec::new(),
                                    checked_at: Instant::now(),
                                };
                            }
                            if answers.iter().any(|ip| SINKHOLE_IPS.contains(&ip.as_str())) {
                                return DnsCheckResult {
                                    host: host.to_string(),
                                    safe: false,
                                    reason:
                                        "DNS sinkhole adresi (0.0.0.0 / 127.0.0.1) döndü.".to_string(),
                                    resolved_ips: answers,
                                    checked_at: Instant::now(),
                                };
                            }
                            if !answers.is_empty() {
                                return DnsCheckResult {
                                    host: host.to_string(),
                                    safe: true,
                                    reason: "Cloudflare DoH (1.1.1.2) doğrulaması temiz."
                                        .to_string(),
                                    resolved_ips: answers,
                                    checked_at: Instant::now(),
                                };
                            }
                        }
                    }
                }
                Err(_) => {
                    if let Ok(mut g) = self.doh_backoff_until.write() {
                        *g = Some(Instant::now() + Duration::from_secs(30));
                    }
                }
            }
        }

        // System DNS fallback (handles offline fail-open gracefully)
        if let Ok(addrs) = (host, 443).to_socket_addrs() {
            let ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
            if ips
                .iter()
                .any(|ip| ip == &Ipv4Addr::UNSPECIFIED.to_string()
                    || ip == &Ipv4Addr::LOCALHOST.to_string()
                    || ip == &Ipv6Addr::LOCALHOST.to_string())
            {
                return DnsCheckResult {
                    host: host.to_string(),
                    safe: false,
                    reason: "Sistem DNS sinkhole adresi döndü.".to_string(),
                    resolved_ips: ips,
                    checked_at: Instant::now(),
                };
            }
            return DnsCheckResult {
                host: host.to_string(),
                safe: true,
                reason: "Sistem DNS doğrulaması temiz.".to_string(),
                resolved_ips: ips,
                checked_at: Instant::now(),
            };
        }

        // Offline / transient network timeout: fail open so user isn't locked out by temporary DNS hiccup
        DnsCheckResult {
            host: host.to_string(),
            safe: true,
            reason: "Çevrimdışı veya geçici DNS zaman aşımı (izin verildi).".to_string(),
            resolved_ips: Vec::new(),
            checked_at: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_local_and_ip_hosts() {
        assert!(DnsGuard::is_local_or_ip("localhost"));
        assert!(DnsGuard::is_local_or_ip("127.0.0.1"));
        assert!(DnsGuard::is_local_or_ip("[::1]"));
        assert!(DnsGuard::is_local_or_ip("printer.local"));
        assert!(!DnsGuard::is_local_or_ip("youtube.com"));
    }

    #[test]
    fn disabled_guard_allows_immediately() {
        let g = DnsGuard::new();
        let res = g.check_host("malware.testing.example", false);
        assert!(res.safe);
    }
}
