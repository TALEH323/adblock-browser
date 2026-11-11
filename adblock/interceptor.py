"""Qt WebEngine URL request interceptor for AdBlock, HTTPS-Only upgrade, and DNS Guard."""

from __future__ import annotations

import threading
from typing import Optional

from PyQt6.QtCore import QUrl, pyqtSignal
from PyQt6.QtWebEngineCore import QWebEngineUrlRequestInfo, QWebEngineUrlRequestInterceptor

from .dns_guard import DnsGuard, is_local_or_ip_host
from .engine import AdBlockEngine, extract_host

_RT = QWebEngineUrlRequestInfo.ResourceType

_RESOURCE_TYPE_MAP = {
    _RT.ResourceTypeMainFrame: "main_frame",
    _RT.ResourceTypeNavigationPreloadMainFrame: "main_frame",
    _RT.ResourceTypeSubFrame: "subdocument",
    _RT.ResourceTypeNavigationPreloadSubFrame: "subdocument",
    _RT.ResourceTypeStylesheet: "stylesheet",
    _RT.ResourceTypeScript: "script",
    _RT.ResourceTypeImage: "image",
    _RT.ResourceTypeFontResource: "font",
    _RT.ResourceTypeSubResource: "other",
    _RT.ResourceTypeObject: "object",
    _RT.ResourceTypeMedia: "media",
    _RT.ResourceTypeWorker: "script",
    _RT.ResourceTypeSharedWorker: "script",
    _RT.ResourceTypeServiceWorker: "script",
    _RT.ResourceTypePrefetch: "other",
    _RT.ResourceTypeFavicon: "image",
    _RT.ResourceTypeXhr: "xmlhttprequest",
    _RT.ResourceTypePing: "ping",
    _RT.ResourceTypeCspReport: "other",
    _RT.ResourceTypePluginResource: "object",
    _RT.ResourceTypeWebSocket: "websocket",
    _RT.ResourceTypeJson: "xmlhttprequest",
    _RT.ResourceTypeUnknown: "other",
}


class AdBlockInterceptor(QWebEngineUrlRequestInterceptor):
    """Intercepts WebEngine requests for AdBlock filtering, HTTPS-Only upgrades, and DoH checks."""

    requestBlocked = pyqtSignal(str, str, str, str)
    httpsUpgraded = pyqtSignal(str, str, str)
    dnsBlocked = pyqtSignal(str, str, str)

    def __init__(
        self,
        engine: AdBlockEngine,
        dns_guard: Optional[DnsGuard] = None,
        https_only_enabled: bool = True,
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.engine = engine
        self.dns_guard = dns_guard
        self._lock = threading.RLock()
        self.https_only_enabled: bool = https_only_enabled
        self.http_allowed_hosts: set[str] = set()
        self.https_upgraded_count: int = 0

    def allow_http_for_host(self, host: str) -> None:
        h = (host or "").strip().lower()
        if h:
            with self._lock:
                self.http_allowed_hosts.add(h)

    def is_http_allowed_for_host(self, host: str) -> bool:
        h = (host or "").strip().lower()
        with self._lock:
            return h in self.http_allowed_hosts

    def disallow_http_for_host(self, host: str) -> None:
        h = (host or "").strip().lower()
        with self._lock:
            self.http_allowed_hosts.discard(h)

    def interceptRequest(self, info: QWebEngineUrlRequestInfo) -> None:
        request_url = info.requestUrl().toString()
        first_party_url = info.firstPartyUrl().toString()
        resource_type = _RESOURCE_TYPE_MAP.get(info.resourceType(), "other")
        req_host = extract_host(request_url)

        # 1. HTTPS-Only automatic upgrade (skip local/IP hosts and explicit HTTP exceptions)
        if request_url.lower().startswith("http://"):
            with self._lock:
                upgrade_active = self.https_only_enabled and req_host not in self.http_allowed_hosts
            if upgrade_active and req_host and not is_local_or_ip_host(req_host):
                upgraded_url = "https://" + request_url[7:]
                with self._lock:
                    self.https_upgraded_count += 1
                info.redirect(QUrl(upgraded_url))
                self.httpsUpgraded.emit(first_party_url, request_url, upgraded_url)
                return

        # 2. DoH DNS Security check for main-frame navigations
        if self.dns_guard is not None and resource_type == "main_frame" and req_host:
            dns_res = self.dns_guard.check_domain(req_host)
            if dns_res.blocked:
                info.block(True)
                self.dnsBlocked.emit(request_url, dns_res.provider_name, dns_res.reason)
                return

        # 3. EasyList / AdBlock network filtering
        result = self.engine.should_block(
            request_url=request_url,
            first_party_url=first_party_url,
            resource_type=resource_type,
        )
        if result.blocked:
            info.block(True)
            self.requestBlocked.emit(first_party_url, request_url, resource_type, result.rule)
