"""DNS-over-HTTPS (DoH) security guard for malware/phishing/ad domain checks."""

from __future__ import annotations

import ipaddress
import json
import threading
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Dict, Optional, Tuple

DOH_PROVIDERS: Dict[str, Tuple[str, str]] = {
    "cloudflare_security": (
        "Cloudflare Güvenlik DNS (1.1.1.2 — Zararlı & Phishing)",
        "https://security.cloudflare-dns.com/dns-query?name={domain}&type=A",
    ),
    "adguard": (
        "AdGuard DNS (Reklam, İzleyici & Zararlı Site)",
        "https://dns.adguard-dns.com/resolve?name={domain}&type=A",
    ),
    "google": (
        "Google Public DoH (Şifreli DNS)",
        "https://dns.google/resolve?name={domain}&type=A",
    ),
    "off": (
        "Kapalı (Sistem DNS)",
        "",
    ),
}

_BLOCKED_IPS = {"0.0.0.0", "127.0.0.1", "::", "::1"}


def is_local_or_ip_host(host: str) -> bool:
    """Return True for localhost, .local domains, or raw IP addresses."""
    h = (host or "").strip().lower().rstrip(".")
    if not h or h == "localhost" or h.endswith(".local") or h.endswith(".internal"):
        return True
    try:
        ipaddress.ip_address(h)
        return True
    except ValueError:
        return False


@dataclass
class DnsCheckResult:
    """Result of a DoH security check for a domain."""

    host: str
    blocked: bool
    provider_id: str
    provider_name: str
    reason: str = ""
    resolved_ips: Tuple[str, ...] = ()
    checked_at: float = 0.0


class DnsGuard:
    """Thread-safe DoH resolver verifier with TTL cache and override list."""

    CACHE_TTL_SECONDS = 300.0

    def __init__(self, provider_id: str = "cloudflare_security", timeout: float = 1.5) -> None:
        self._lock = threading.RLock()
        self.provider_id = provider_id if provider_id in DOH_PROVIDERS else "cloudflare_security"
        self.timeout = timeout
        self.total_blocked: int = 0
        self.allowed_overrides: set[str] = set()
        self._cache: Dict[Tuple[str, str], DnsCheckResult] = {}

    def set_provider(self, provider_id: str) -> None:
        with self._lock:
            self.provider_id = provider_id if provider_id in DOH_PROVIDERS else "off"

    def allow_host_override(self, host: str) -> None:
        h = (host or "").strip().lower()
        if h:
            with self._lock:
                self.allowed_overrides.add(h)

    def get_provider_name(self) -> str:
        return DOH_PROVIDERS.get(self.provider_id, DOH_PROVIDERS["off"])[0]

    def get_cached_result(self, host: str) -> Optional[DnsCheckResult]:
        h = (host or "").strip().lower().rstrip(".")
        with self._lock:
            return self._cache.get((self.provider_id, h))

    def check_domain(self, host: str) -> DnsCheckResult:
        """Query the configured DoH security provider to verify whether a domain is blocked."""
        h = (host or "").strip().lower().rstrip(".")
        with self._lock:
            provider_id = self.provider_id
            provider_name, template = DOH_PROVIDERS.get(provider_id, DOH_PROVIDERS["off"])

            if provider_id == "off" or not template or not h or is_local_or_ip_host(h):
                return DnsCheckResult(
                    host=h,
                    blocked=False,
                    provider_id=provider_id,
                    provider_name=provider_name,
                    reason="skipped",
                    checked_at=time.time(),
                )

            if h in self.allowed_overrides:
                return DnsCheckResult(
                    host=h,
                    blocked=False,
                    provider_id=provider_id,
                    provider_name=provider_name,
                    reason="override",
                    checked_at=time.time(),
                )

            cached = self._cache.get((provider_id, h))
            if cached and (time.time() - cached.checked_at) < self.CACHE_TTL_SECONDS:
                return cached

        # Perform network DoH lookup outside lock
        result = self._query_doh(h, provider_id, provider_name, template)
        with self._lock:
            self._cache[(provider_id, h)] = result
            if result.blocked:
                self.total_blocked += 1
        return result

    def _query_doh(
        self,
        host: str,
        provider_id: str,
        provider_name: str,
        template: str,
    ) -> DnsCheckResult:
        url = template.format(domain=urllib.parse.quote(host, safe=""))
        req = urllib.request.Request(
            url,
            headers={"Accept": "application/dns-json", "User-Agent": "AdBlockBrowser/1.0"},
        )
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                data = json.loads(resp.read().decode("utf-8", errors="ignore"))
        except Exception:
            # Fail-open on transient network/offline errors so browsing isn't stuck
            return DnsCheckResult(
                host=host,
                blocked=False,
                provider_id=provider_id,
                provider_name=provider_name,
                reason="offline_fallback",
                checked_at=time.time(),
            )

        status = int(data.get("Status", 0))
        answers = data.get("Answer") or []
        ips = tuple(
            str(a.get("data", "")).strip()
            for a in answers
            if isinstance(a, dict) and a.get("type") in (1, 28)
        )
        comment_str = " ".join(str(c) for c in (data.get("Comment") or [])) if isinstance(data.get("Comment"), list) else str(data.get("Comment") or "")

        # Security DoH servers return 0.0.0.0 / :: or explicit "blocked" comment/EDE
        if any(ip in _BLOCKED_IPS for ip in ips):
            return DnsCheckResult(
                host=host,
                blocked=True,
                provider_id=provider_id,
                provider_name=provider_name,
                reason="DNS sinkhole (0.0.0.0)",
                resolved_ips=ips,
                checked_at=time.time(),
            )

        low_comment = comment_str.lower()
        if status == 3 and any(k in low_comment for k in ("block", "malware", "phish", "filtered", "censored")):
            return DnsCheckResult(
                host=host,
                blocked=True,
                provider_id=provider_id,
                provider_name=provider_name,
                reason=comment_str or "DNS tarafından engellendi",
                resolved_ips=ips,
                checked_at=time.time(),
            )

        return DnsCheckResult(
            host=host,
            blocked=False,
            provider_id=provider_id,
            provider_name=provider_name,
            reason="ok",
            resolved_ips=ips,
            checked_at=time.time(),
        )
