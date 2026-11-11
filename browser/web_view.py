"""WebEngine tab and page wrappers with AdBlock cosmetic injection and popup handling."""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Callable, List, Optional

from PyQt6.QtCore import QUrl, pyqtSignal
from PyQt6.QtWebChannel import QWebChannel
from PyQt6.QtWebEngineCore import QWebEnginePage, QWebEngineProfile, QWebEngineScript, QWebEngineSettings
from PyQt6.QtWebEngineWidgets import QWebEngineView

from adblock.cosmetic import build_cosmetic_script, remove_cosmetic_script
from adblock.engine import AdBlockEngine, extract_host


@dataclass
class BlockedRequestLog:
    """Single blocked request entry for display in the AdBlock shield panel."""

    request_url: str
    resource_type: str
    rule: str


class BrowserPage(QWebEnginePage):
    """Custom QWebEnginePage that routes new window/tab requests and internal actions."""

    internalActionRequested = pyqtSignal(str)

    def __init__(
        self,
        profile: QWebEngineProfile,
        create_tab_callback: Callable[[], "BrowserTab"],
        engine: AdBlockEngine,
        parent=None,
    ) -> None:
        super().__init__(profile, parent)
        self._create_tab_callback = create_tab_callback
        self._engine = engine

    def createWindow(self, win_type: QWebEnginePage.WebWindowType) -> QWebEnginePage:
        # Create a new tab cleanly for target="_blank" / window.open
        new_tab = self._create_tab_callback()
        return new_tab.page()

    def acceptNavigationRequest(
        self,
        url: QUrl,
        nav_type: QWebEnginePage.NavigationType,
        isMainFrame: bool,
    ) -> bool:
        if isMainFrame and url.scheme() == "adblock":
            self.internalActionRequested.emit(url.toString())
            return False
        if isMainFrame and url.scheme() == "adblock-cmd" and self.url().scheme() == "adblock":
            self.internalActionRequested.emit(url.toString())
            return False
        return super().acceptNavigationRequest(url, nav_type, isMainFrame)


class BrowserTab(QWebEngineView):
    """Single browser tab holding a QWebEngineView and per-tab AdBlock/security telemetry."""

    blockedCountChanged = pyqtSignal(int)
    certificateRejected = pyqtSignal(str, str, bool)
    internalActionRequested = pyqtSignal(str)

    MAX_LOG_ITEMS = 250

    def __init__(
        self,
        profile: QWebEngineProfile,
        engine: AdBlockEngine,
        create_tab_callback: Callable[[], "BrowserTab"],
        pw_bridge=None,
        cert_exception_hosts: Optional[set] = None,
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.engine = engine
        self.blocked_count: int = 0
        self.blocked_items: List[BlockedRequestLog] = []
        self._cosmetic_injected_for_nav: bool = False
        self.is_home_page: bool = False
        self.internal_url: str = ""
        self.last_cert_error: str = ""
        self.last_https_upgrade_http_url: str = ""
        self.last_https_upgrade_target: str = ""
        self.is_security_interstitial: bool = False
        self._keep_security_state: bool = False
        self._cert_exception_hosts = (
            cert_exception_hosts if cert_exception_hosts is not None else set()
        )

        custom_page = BrowserPage(profile, create_tab_callback, engine, self)
        custom_page.certificateError.connect(self._on_certificate_error)
        custom_page.internalActionRequested.connect(self.internalActionRequested.emit)
        if pw_bridge is not None:
            channel = QWebChannel(custom_page)
            channel.registerObject("pwBridge", pw_bridge)
            custom_page.setWebChannel(channel, QWebEngineScript.ScriptWorldId.ApplicationWorld)
        self.setPage(custom_page)

        settings = self.settings()
        settings.setAttribute(QWebEngineSettings.WebAttribute.PluginsEnabled, True)
        settings.setAttribute(QWebEngineSettings.WebAttribute.JavascriptEnabled, True)
        settings.setAttribute(QWebEngineSettings.WebAttribute.LocalStorageEnabled, True)
        settings.setAttribute(QWebEngineSettings.WebAttribute.FullScreenSupportEnabled, True)
        settings.setAttribute(QWebEngineSettings.WebAttribute.ScrollAnimatorEnabled, True)

        self.loadStarted.connect(self._on_load_started)
        self.loadProgress.connect(self._on_load_progress)
        self.loadFinished.connect(self._on_load_finished)
        self.urlChanged.connect(lambda _q: self.apply_cosmetic_filtering())

    def current_host(self) -> str:
        if self.is_home_page:
            return ""
        return extract_host(self.url().toString())

    def record_blocked(self, request_url: str, resource_type: str, rule: str) -> None:
        self.blocked_count += 1
        if len(self.blocked_items) < self.MAX_LOG_ITEMS:
            self.blocked_items.insert(0, BlockedRequestLog(request_url, resource_type, rule))
        self.blockedCountChanged.emit(self.blocked_count)

    def _on_load_started(self) -> None:
        self.blocked_count = 0
        self.blocked_items.clear()
        self._cosmetic_injected_for_nav = False
        if self._keep_security_state:
            self._keep_security_state = False
        else:
            self.last_cert_error = ""
            self.is_security_interstitial = False
            self.last_https_upgrade_http_url = ""
            self.last_https_upgrade_target = ""
        self.blockedCountChanged.emit(0)

    def _on_certificate_error(self, error) -> None:
        host = extract_host(error.url().toString())
        if error.isOverridable() and host and host in self._cert_exception_hosts:
            error.acceptCertificate()
            return
        desc = error.description()
        is_overridable = error.isOverridable()
        if error.isMainFrame():
            self.last_cert_error = desc
            self.certificateRejected.emit(error.url().toString(), desc, is_overridable)
        error.rejectCertificate()

    def set_interstitial_html(self, html_str: str, base_url: QUrl, cert_error: str = "") -> None:
        """Render a security warning interstitial without wiping active cert error state."""
        self.is_home_page = False
        self.is_security_interstitial = True
        self._keep_security_state = True
        if cert_error:
            self.last_cert_error = cert_error
        self.setHtml(html_str, base_url)

    def _on_load_progress(self, progress: int) -> None:
        if progress >= 20 and not self._cosmetic_injected_for_nav:
            self._cosmetic_injected_for_nav = True
            self.apply_cosmetic_filtering()

    def _on_load_finished(self, ok: bool) -> None:
        if ok:
            self.apply_cosmetic_filtering()

    def apply_cosmetic_filtering(self) -> None:
        """Inject or remove cosmetic CSS rules based on current AdBlock settings."""
        url_str = self.url().toString()
        if not url_str or url_str.startswith("adblock://") or url_str.startswith("data:"):
            return

        selectors = self.engine.get_cosmetic_selectors(url_str)
        if selectors:
            js = build_cosmetic_script(selectors, url_str)
            if js:
                self.page().runJavaScript(js)
        else:
            self.page().runJavaScript(remove_cosmetic_script())

    def fill_credentials(self, username: str, password: str) -> None:
        """Fill credentials into the active page's login inputs in ApplicationWorld."""
        u_json = json.dumps(username)
        p_json = json.dumps(password)
        self.page().runJavaScript(
            f"window.__adblockFillCreds && window.__adblockFillCreds({u_json}, {p_json});",
            QWebEngineScript.ScriptWorldId.ApplicationWorld,
        )
