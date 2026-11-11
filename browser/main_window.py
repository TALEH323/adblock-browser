"""Main browser window with tabbed browsing, AdBlock shield, bookmarks, and history."""

from __future__ import annotations

import os
from pathlib import Path
from typing import List, Optional
from urllib.parse import parse_qs

from PyQt6.QtCore import QSize, QStringListModel, Qt, QTimer, QUrl
from PyQt6.QtGui import QAction, QIcon, QKeySequence, QShortcut
from PyQt6.QtWebEngineCore import (
    QWebEngineDownloadRequest,
    QWebEngineFullScreenRequest,
    QWebEnginePage,
    QWebEngineProfile,
    QWebEngineScript,
)
from PyQt6.QtWidgets import (
    QApplication,
    QCompleter,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMenu,
    QProgressBar,
    QPushButton,
    QStackedWidget,
    QStatusBar,
    QTabBar,
    QVBoxLayout,
    QWidget,
)

from adblock.cosmetic import build_youtube_early_script
from adblock.dns_guard import DnsGuard
from adblock.engine import AdBlockEngine, extract_host
from adblock.interceptor import AdBlockInterceptor
from .devtools_dialog import DevToolsWindow, FindBar
from .downloads import DownloadsManager
from .home_page import render_home_html
from .icons import svg_icon, svg_pixmap
from .internal_pages import (
    render_adblock_page,
    render_bookmarks_page,
    render_downloads_page,
    render_history_page,
    render_passwords_page,
    render_settings_page,
)
from .password_script import build_password_manager_script
from .passwords import PasswordBridge, PasswordManager
from .popovers import (
    AdBlockPopover,
    ChromeSidePanel,
    DownloadsPopover,
    SiteSecurityPopover,
    _open_local_path,
    _reveal_in_folder,
)
from .security_dialog import (
    build_cert_error_html,
    build_dns_blocked_html,
    build_https_failed_html,
)
from .shield_panel import FilterUpdateThread
from .storage import BookmarksManager, HistoryManager, SettingsManager
from .web_view import BrowserTab


class _ChromeTabDeck:
    """Pairs a top Chrome-style QTabBar with a QStackedWidget viewport below the toolbar."""

    def __init__(self, parent=None) -> None:
        self._bar = QTabBar(parent)
        self._bar.setDocumentMode(True)
        self._bar.setExpanding(False)
        self._bar.setDrawBase(False)
        self._bar.setTabsClosable(False)
        self._bar.setMovable(True)
        self._bar.setElideMode(Qt.TextElideMode.ElideRight)
        self._bar.setUsesScrollButtons(True)

        self._stack = QStackedWidget(parent)
        self._bar.currentChanged.connect(self._on_bar_current_changed)
        self._bar.tabMoved.connect(self._on_tab_moved)

        self.currentChanged = self._bar.currentChanged

    def _on_bar_current_changed(self, idx: int) -> None:
        w = self.widget(idx)
        if w is not None:
            self._stack.setCurrentWidget(w)

    def _on_tab_moved(self, _from: int, _to: int) -> None:
        w = self.currentWidget()
        if w is not None:
            self._stack.setCurrentWidget(w)

    def tabBar(self) -> QTabBar:
        return self._bar

    def stackWidget(self) -> QStackedWidget:
        return self._stack

    def addTab(self, widget: QWidget, label: str, on_close_cb=None) -> int:
        self._stack.addWidget(widget)
        idx = self._bar.addTab(label)
        self._bar.setTabData(idx, widget)
        close_btn = QPushButton()
        close_btn.setObjectName("TabCloseBtn")
        close_btn.setFixedSize(16, 16)
        close_btn.setIcon(svg_icon("close", "#9aa0a6", 11))
        close_btn.setIconSize(QSize(11, 11))
        close_btn.setToolTip("Sekmeyi Kapat")
        if on_close_cb is not None:
            close_btn.clicked.connect(lambda _=False, w=widget: on_close_cb(self.indexOf(w)))
        self._bar.setTabButton(idx, QTabBar.ButtonPosition.RightSide, close_btn)
        return idx

    def removeTab(self, index: int) -> None:
        w = self.widget(index)
        self._bar.removeTab(index)
        if w is not None:
            self._stack.removeWidget(w)

    def count(self) -> int:
        return self._bar.count()

    def currentIndex(self) -> int:
        return self._bar.currentIndex()

    def setCurrentIndex(self, index: int) -> None:
        self._bar.setCurrentIndex(index)
        w = self.widget(index)
        if w is not None:
            self._stack.setCurrentWidget(w)

    def currentWidget(self) -> Optional[QWidget]:
        return self.widget(self._bar.currentIndex())

    def widget(self, index: int) -> Optional[QWidget]:
        if 0 <= index < self._bar.count():
            data = self._bar.tabData(index)
            if isinstance(data, QWidget):
                return data
        return None

    def indexOf(self, widget: QWidget) -> int:
        for i in range(self._bar.count()):
            if self._bar.tabData(i) is widget:
                return i
        return -1

    def setTabText(self, index: int, text: str) -> None:
        self._bar.setTabText(index, text)

    def setTabToolTip(self, index: int, text: str) -> None:
        self._bar.setTabToolTip(index, text)

    def setTabIcon(self, index: int, icon: QIcon) -> None:
        self._bar.setTabIcon(index, icon)


def resolve_user_input_to_url(raw_text: str, settings: SettingsManager) -> QUrl:
    """Convert address bar input into either a direct URL or a search engine query."""
    text = raw_text.strip()
    if not text:
        return QUrl(settings.get("homepage", "adblock://home"))

    if text.startswith("adblock://"):
        return QUrl(text)

    if "://" in text:
        return QUrl.fromUserInput(text)

    # Detect host-like inputs (example.com, localhost:8000, 127.0.0.1) without spaces
    if " " not in text and ("." in text or text.startswith("localhost")):
        return QUrl.fromUserInput("https://" + text if not text.startswith("localhost") else "http://" + text)

    return QUrl(settings.get_search_url(text))


class MainWindow(QMainWindow):
    """Primary desktop browser window with integrated AdBlock controls."""

    def __init__(self, data_dir: Path, incognito: bool = False, parent=None) -> None:
        super().__init__(parent)
        self.data_dir = Path(data_dir)
        self.data_dir.mkdir(parents=True, exist_ok=True)
        self.incognito = incognito
        self._closed_tabs: List[str] = []
        self._child_windows: List[QMainWindow] = []
        self._devtools_windows: List[DevToolsWindow] = []
        self._cert_exception_hosts: set[str] = set()

        # Storage managers
        self.settings = SettingsManager(self.data_dir)
        self.dns_guard = DnsGuard(
            provider_id=str(self.settings.get("doh_provider", "cloudflare_security"))
        )
        self.bookmarks = BookmarksManager(self.data_dir)
        self.history = HistoryManager(self.data_dir)
        self.downloads = DownloadsManager(self)
        self.pw_manager = PasswordManager(self.data_dir)
        self.pw_bridge = PasswordBridge(
            self.pw_manager,
            is_enabled_fn=lambda: (not self.incognito) and bool(self.settings.get("password_manager_enabled", True)),
            parent=self,
        )
        self.pw_bridge.savePromptRequested.connect(self._on_save_password_prompt)
        self._pending_pw: Optional[tuple[str, str, str]] = None

        # AdBlock engine + WebEngine profile + URL interceptor
        self.engine = AdBlockEngine(
            filter_dir=self.data_dir / "filter_lists",
            custom_rules_path=self.data_dir / "custom_filters.txt",
        )
        self.engine.enabled = bool(self.settings.get("adblock_enabled", True))
        self.engine.cosmetic_enabled = bool(self.settings.get("cosmetic_enabled", True))
        self.engine.whitelisted_domains = set(self.settings.get("whitelisted_domains", []))
        self.engine.total_blocked = int(self.settings.get("total_blocked", 0))

        if self.incognito:
            # Off-the-record ephemeral Chromium profile (no disk cookies/cache)
            self.profile = QWebEngineProfile(self)
        else:
            profile_storage = str(self.data_dir / "web_profile")
            self.profile = QWebEngineProfile("AdBlockBrowserProfile", self)
            self.profile.setPersistentStoragePath(profile_storage)
            self.profile.setCachePath(str(self.data_dir / "web_cache"))
        self.profile.downloadRequested.connect(self._on_download_requested)

        self.interceptor = AdBlockInterceptor(
            self.engine,
            dns_guard=self.dns_guard,
            https_only_enabled=bool(self.settings.get("https_only_enabled", True)),
            parent=self,
        )
        for allowed_host in self.settings.get("http_allowed_domains", []):
            self.interceptor.allow_http_for_host(allowed_host)
        self.interceptor.requestBlocked.connect(self._on_request_blocked)
        self.interceptor.httpsUpgraded.connect(self._on_https_upgraded)
        self.interceptor.dnsBlocked.connect(self._on_dns_blocked)
        self.profile.setUrlRequestInterceptor(self.interceptor)
        self._sync_profile_scripts()

        self.setWindowTitle("Gizli Pencere — AdBlock Tarayıcı" if self.incognito else "AdBlock Tarayıcı")
        self.setWindowIcon(svg_icon("incognito" if self.incognito else "shield", "#8ab4f8", 20))
        self.resize(1240, 820)
        self._is_html_fullscreen = False
        self._was_maximized_before_fullscreen = False
        self._apply_theme()
        self._build_ui()
        self._setup_shortcuts()

        # Persist total_blocked periodically every 15 seconds
        self._stats_timer = QTimer(self)
        self._stats_timer.timeout.connect(self._persist_stats)
        self._stats_timer.start(15_000)

        # Open initial tab
        self.open_new_tab(self.settings.get("homepage", "adblock://home"))

    def _apply_theme(self) -> None:
        bar_bg = "#282433" if self.incognito else "#2f3033"
        strip_bg = "#191622" if self.incognito else "#1c1d1f"
        hover_bg = "#383347" if self.incognito else "#3c3e42"
        pill_bg = "#262330" if self.incognito else "#2b2c2f"
        omni_bg = "#1e1b26" if self.incognito else "#1d1e20"
        self.setStyleSheet(
            f"""
            QMainWindow {{ background-color: {strip_bg}; color: #e8eaed; }}
            QWidget#TabStrip {{
                background-color: {strip_bg};
            }}
            QWidget#NavToolbar {{
                background-color: {bar_bg};
                border: none;
            }}
            QPushButton#TabSearchBtn {{
                background-color: {pill_bg};
                border: none;
                border-radius: 13px;
                min-width: 26px;
                max-width: 26px;
                min-height: 26px;
                max-height: 26px;
            }}
            QPushButton#TabSearchBtn:hover {{
                background-color: {hover_bg};
            }}
            QPushButton#NewTabBtn {{
                background-color: transparent;
                border: none;
                border-radius: 14px;
                min-width: 28px;
                max-width: 28px;
                min-height: 28px;
                max-height: 28px;
            }}
            QPushButton#NewTabBtn:hover {{
                background-color: {hover_bg};
            }}
            QPushButton#TopGuardPill {{
                background-color: {pill_bg};
                color: #e8eaed;
                border: 1px solid #3c4043;
                border-radius: 13px;
                padding: 3px 11px;
                min-height: 26px;
                font-size: 12px;
                font-weight: 500;
            }}
            QPushButton#TopGuardPill:hover {{
                background-color: {hover_bg};
                border-color: #5f6368;
            }}
            QPushButton#NavBtn {{
                background-color: transparent;
                color: #e8eaed;
                border: none;
                border-radius: 14px;
                min-width: 28px;
                min-height: 28px;
                padding: 4px;
                font-size: 13px;
                font-weight: 500;
            }}
            QPushButton#NavBtn:hover {{
                background-color: {hover_bg};
            }}
            QPushButton#NavBtn:disabled {{
                color: #5f6368;
            }}
            QPushButton#TabCloseBtn {{
                background-color: transparent;
                border: none;
                border-radius: 8px;
                padding: 2px;
            }}
            QPushButton#TabCloseBtn:hover {{
                background-color: rgba(255, 255, 255, 0.14);
            }}
            QWidget#OmniboxPill {{
                background-color: {omni_bg};
                border: 1px solid transparent;
                border-radius: 16px;
                min-height: 32px;
                max-height: 32px;
            }}
            QWidget#OmniboxPill:hover {{
                background-color: #242528;
                border-color: #44474b;
            }}
            QPushButton#SecChip {{
                background-color: #2b2d30;
                color: #e8eaed;
                border: none;
                border-radius: 11px;
                padding: 2px 9px;
                min-height: 22px;
                max-height: 22px;
                font-size: 11.5px;
                font-weight: 600;
            }}
            QPushButton#SecChip:hover {{
                background-color: #383a3e;
            }}
            QLineEdit#UrlBar {{
                background-color: transparent;
                color: #e8eaed;
                border: none;
                padding: 4px 4px;
                font-size: 13px;
                selection-background-color: #39629c;
            }}
            QPushButton#ShieldBtn {{
                border-radius: 13px;
                padding: 3px 9px;
                min-height: 26px;
                font-size: 11.5px;
                font-weight: 600;
            }}
            QFrame#ToolbarSep {{
                background-color: #44474b;
                min-width: 1px;
                max-width: 1px;
                min-height: 16px;
                max-height: 16px;
                margin-left: 3px;
                margin-right: 3px;
            }}
            QTabBar {{
                background-color: transparent;
            }}
            QTabBar::tab {{
                background-color: transparent;
                color: #9aa0a6;
                border: none;
                border-radius: 8px;
                padding: 6px 10px 6px 12px;
                margin: 2px 2px 4px 2px;
                min-width: 120px;
                max-width: 215px;
                height: 24px;
                font-size: 12px;
            }}
            QTabBar::tab:selected {{
                background-color: {bar_bg};
                color: #e8eaed;
                border-radius: 0px;
                border-top-left-radius: 10px;
                border-top-right-radius: 10px;
                margin: 0px 2px 0px 2px;
                padding: 7px 10px 7px 12px;
                height: 28px;
            }}
            QTabBar::tab:hover:!selected {{
                background-color: #28292c;
                color: #e8eaed;
            }}
            QWidget#BookmarksBar {{
                background-color: {bar_bg};
                border-bottom: 1px solid #3c4043;
            }}
            QPushButton#BmPill {{
                background-color: transparent;
                color: #bdc1c6;
                border: none;
                border-radius: 11px;
                padding: 2px 10px;
                font-size: 12px;
            }}
            QPushButton#BmPill:hover {{
                background-color: {hover_bg};
                color: #e8eaed;
            }}
            QProgressBar {{
                background-color: #202124;
                border: none;
                height: 2px;
            }}
            QProgressBar::chunk {{
                background-color: #8ab4f8;
            }}
            QStatusBar {{
                background-color: #202124;
                color: #9aa0a6;
                border-top: 1px solid #3c4043;
                font-size: 12px;
            }}
            QStatusBar QLabel {{
                color: #9aa0a6;
                font-size: 12px;
            }}
            """
        )

    def _build_ui(self) -> None:
        central = QWidget()
        root_layout = QVBoxLayout(central)
        root_layout.setContentsMargins(0, 0, 0, 0)
        root_layout.setSpacing(0)

        # Top Chrome-style Tab Strip (Tab Search pill + Tabs + New Tab "+" + Top-Right Guard Pill)
        self.tabs = _ChromeTabDeck(self)
        self.tabs.currentChanged.connect(self._on_current_tab_changed)
        self.tabs.tabBar().setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.tabs.tabBar().customContextMenuRequested.connect(self._on_tab_context_menu)

        self.tab_strip = QWidget()
        self.tab_strip.setObjectName("TabStrip")
        strip_layout = QHBoxLayout(self.tab_strip)
        strip_layout.setContentsMargins(10, 6, 10, 0)
        strip_layout.setSpacing(6)

        self.tab_search_btn = QPushButton()
        self.tab_search_btn.setObjectName("TabSearchBtn")
        self.tab_search_btn.setIcon(svg_icon("chevron_down", "#bdc1c6", 14))
        self.tab_search_btn.setIconSize(QSize(14, 14))
        self.tab_search_btn.setToolTip("Açık Sekmeleri Ara")
        self.tab_search_btn.clicked.connect(self._open_tab_search_menu)

        self.new_tab_btn = QPushButton()
        self.new_tab_btn.setObjectName("NewTabBtn")
        self.new_tab_btn.setIcon(svg_icon("plus", "#bdc1c6", 15))
        self.new_tab_btn.setIconSize(QSize(15, 15))
        self.new_tab_btn.setToolTip("Yeni Sekme (Cmd+T)")
        self.new_tab_btn.clicked.connect(lambda: self.open_new_tab("adblock://home"))

        self.top_guard_pill = QPushButton(" DoH + AdBlock")
        self.top_guard_pill.setObjectName("TopGuardPill")
        self.top_guard_pill.setIcon(svg_icon("shield", "#8ab4f8", 14))
        self.top_guard_pill.setIconSize(QSize(14, 14))
        self.top_guard_pill.setToolTip("Site Güvenliği, HTTPS ve DNS Koruması")
        self.top_guard_pill.clicked.connect(self._open_site_security_dialog)

        strip_layout.addWidget(self.tab_search_btn, 0, Qt.AlignmentFlag.AlignVCenter)
        strip_layout.addWidget(self.tabs.tabBar())
        strip_layout.addWidget(self.new_tab_btn, 0, Qt.AlignmentFlag.AlignVCenter)
        strip_layout.addStretch(1)
        strip_layout.addWidget(self.top_guard_pill, 0, Qt.AlignmentFlag.AlignVCenter)
        root_layout.addWidget(self.tab_strip)

        # Chrome Navigation Toolbar seamlessly flush under active tab
        self.nav_toolbar = QWidget()
        self.nav_toolbar.setObjectName("NavToolbar")
        nav_layout = QHBoxLayout(self.nav_toolbar)
        nav_layout.setContentsMargins(8, 5, 8, 5)
        nav_layout.setSpacing(2)

        self.back_btn = QPushButton()
        self.back_btn.setIcon(svg_icon("back", "#e8eaed", 16))
        self.back_btn.setIconSize(QSize(16, 16))
        self.back_btn.setObjectName("NavBtn")
        self.back_btn.setToolTip("Geri (Cmd+[)")
        self.back_btn.clicked.connect(lambda: self.current_tab() and self.current_tab().back())

        self.forward_btn = QPushButton()
        self.forward_btn.setIcon(svg_icon("forward", "#e8eaed", 16))
        self.forward_btn.setIconSize(QSize(16, 16))
        self.forward_btn.setObjectName("NavBtn")
        self.forward_btn.setToolTip("İleri (Cmd+])")
        self.forward_btn.clicked.connect(lambda: self.current_tab() and self.current_tab().forward())

        self.reload_btn = QPushButton()
        self.reload_btn.setIcon(svg_icon("reload", "#e8eaed", 16))
        self.reload_btn.setIconSize(QSize(16, 16))
        self.reload_btn.setObjectName("NavBtn")
        self.reload_btn.setToolTip("Yenile (Cmd+R)")
        self.reload_btn.clicked.connect(self._on_reload_or_stop)

        self.home_btn = QPushButton()
        self.home_btn.setIcon(svg_icon("home", "#e8eaed", 16))
        self.home_btn.setIconSize(QSize(16, 16))
        self.home_btn.setObjectName("NavBtn")
        self.home_btn.setToolTip("Ana Sayfa")
        self.home_btn.clicked.connect(self._go_home)

        # Chrome-style Omnibox Pill containing nested security chip + URL input + bookmark star
        omnibox = QWidget()
        omnibox.setObjectName("OmniboxPill")
        omni_layout = QHBoxLayout(omnibox)
        omni_layout.setContentsMargins(5, 2, 5, 2)
        omni_layout.setSpacing(6)

        self.security_badge = QPushButton(" AdBlock")
        self.security_badge.setObjectName("SecChip")
        self.security_badge.setIcon(svg_icon("shield", "#8ab4f8", 13))
        self.security_badge.setIconSize(QSize(13, 13))
        self.security_badge.setCursor(Qt.CursorShape.PointingHandCursor)
        self.security_badge.setToolTip("Bağlantı Güvenliği ve DNS Durumu")
        self.security_badge.clicked.connect(self._open_site_security_dialog)

        self.url_bar = QLineEdit()
        self.url_bar.setObjectName("UrlBar")
        self.url_bar.setPlaceholderText("Google'da arayın veya bir URL yazın")
        self.url_bar.returnPressed.connect(self._navigate_from_url_bar)

        self._url_completer_model = QStringListModel(self)
        self._url_completer = QCompleter(self._url_completer_model, self)
        self._url_completer.setCaseSensitivity(Qt.CaseSensitivity.CaseInsensitive)
        self._url_completer.setFilterMode(Qt.MatchFlag.MatchContains)
        self.url_bar.setCompleter(self._url_completer)
        self._refresh_omnibox_suggestions()

        self.bookmark_star_btn = QPushButton()
        self.bookmark_star_btn.setIcon(svg_icon("star_outline", "#9aa0a6", 15))
        self.bookmark_star_btn.setIconSize(QSize(15, 15))
        self.bookmark_star_btn.setObjectName("NavBtn")
        self.bookmark_star_btn.setFixedSize(26, 26)
        self.bookmark_star_btn.setToolTip("Yer İmine Ekle / Çıkar (Cmd+D)")
        self.bookmark_star_btn.clicked.connect(self._toggle_current_bookmark)

        omni_layout.addWidget(self.security_badge)
        omni_layout.addWidget(self.url_bar, 1)
        omni_layout.addWidget(self.bookmark_star_btn)

        self.shield_btn = QPushButton("0")
        self.shield_btn.setIcon(svg_icon("shield", "#81c995", 15))
        self.shield_btn.setIconSize(QSize(15, 15))
        self.shield_btn.setObjectName("ShieldBtn")
        self.shield_btn.setToolTip("AdBlock Kalkanı ve Ayarları")
        self.shield_btn.clicked.connect(self._open_shield_panel)

        self.passwords_btn = QPushButton()
        self.passwords_btn.setIcon(svg_icon("key", "#bdc1c6", 16))
        self.passwords_btn.setIconSize(QSize(16, 16))
        self.passwords_btn.setObjectName("NavBtn")
        self.passwords_btn.setToolTip("Kayıtlı Şifreler")
        self.passwords_btn.clicked.connect(self._open_passwords_dialog)

        self.bookmarks_btn = QPushButton()
        self.bookmarks_btn.setIcon(svg_icon("bookmarks", "#bdc1c6", 16))
        self.bookmarks_btn.setIconSize(QSize(16, 16))
        self.bookmarks_btn.setObjectName("NavBtn")
        self.bookmarks_btn.setToolTip("Yer İmlerini Yönet")
        self.bookmarks_btn.clicked.connect(self._open_bookmarks_dialog)

        self.history_btn = QPushButton()
        self.history_btn.setIcon(svg_icon("history", "#bdc1c6", 16))
        self.history_btn.setIconSize(QSize(16, 16))
        self.history_btn.setObjectName("NavBtn")
        self.history_btn.setToolTip("Geçmiş (Cmd+H)")
        self.history_btn.clicked.connect(self._open_history_dialog)

        self.downloads_btn = QPushButton()
        self.downloads_btn.setIcon(svg_icon("download", "#bdc1c6", 16))
        self.downloads_btn.setIconSize(QSize(16, 16))
        self.downloads_btn.setObjectName("NavBtn")
        self.downloads_btn.setToolTip("İndirilenler (Cmd+J)")
        self.downloads_btn.clicked.connect(self._open_downloads_dialog)

        self.settings_btn = QPushButton()
        self.settings_btn.setIcon(svg_icon("settings", "#bdc1c6", 16))
        self.settings_btn.setIconSize(QSize(16, 16))
        self.settings_btn.setObjectName("NavBtn")
        self.settings_btn.setToolTip("Ayarlar")
        self.settings_btn.clicked.connect(self._open_settings_dialog)

        self.menu_btn = QPushButton()
        self.menu_btn.setIcon(svg_icon("more_vert", "#bdc1c6", 16))
        self.menu_btn.setIconSize(QSize(16, 16))
        self.menu_btn.setObjectName("NavBtn")
        self.menu_btn.setToolTip("Tarayıcı Menüsü")
        self.menu_btn.clicked.connect(self._open_chrome_menu)

        def _make_sep() -> QFrame:
            sep = QFrame()
            sep.setObjectName("ToolbarSep")
            sep.setFrameShape(QFrame.Shape.VLine)
            return sep

        nav_layout.addWidget(self.back_btn)
        nav_layout.addWidget(self.forward_btn)
        nav_layout.addWidget(self.reload_btn)
        nav_layout.addWidget(self.home_btn)
        nav_layout.addSpacing(4)
        nav_layout.addWidget(omnibox, 1)
        nav_layout.addSpacing(4)
        nav_layout.addWidget(self.shield_btn)
        nav_layout.addWidget(_make_sep())
        nav_layout.addWidget(self.passwords_btn)
        nav_layout.addWidget(self.bookmarks_btn)
        nav_layout.addWidget(self.history_btn)
        nav_layout.addWidget(self.downloads_btn)
        nav_layout.addWidget(_make_sep())

        if self.incognito:
            inc_badge = QPushButton(" Gizli")
            inc_badge.setIcon(svg_icon("incognito", "#e8eaed", 14))
            inc_badge.setIconSize(QSize(14, 14))
            inc_badge.setEnabled(False)
            inc_badge.setStyleSheet(
                "background-color: #5b4b8a; color: #e8eaed; border: none; border-radius: 12px; "
                "padding: 3px 9px; font-size: 11px; font-weight: 600;"
            )
            nav_layout.addWidget(inc_badge)

        nav_layout.addWidget(self.settings_btn)
        nav_layout.addWidget(self.menu_btn)
        root_layout.addWidget(self.nav_toolbar)

        # Bookmarks Bar
        self.bookmarks_bar = QWidget()
        self.bookmarks_bar.setObjectName("BookmarksBar")
        self.bm_bar_layout = QHBoxLayout(self.bookmarks_bar)
        self.bm_bar_layout.setContentsMargins(8, 3, 8, 3)
        self.bm_bar_layout.setSpacing(4)
        self._rebuild_bookmarks_bar()
        self.bookmarks_bar.setVisible(bool(self.settings.get("show_bookmarks_bar", True)))
        root_layout.addWidget(self.bookmarks_bar)

        # Non-blocking Save Password Prompt Bar
        self.pw_banner = QWidget()
        self.pw_banner.setStyleSheet(
            "background-color: #292a2d; border-bottom: 1px solid #3c4043; padding: 5px 12px;"
        )
        pw_banner_layout = QHBoxLayout(self.pw_banner)
        pw_banner_layout.setContentsMargins(10, 4, 10, 4)
        pw_icon = QLabel()
        pw_icon.setPixmap(svg_pixmap("key", "#8ab4f8", 16))
        self.pw_banner_lbl = QLabel("")
        self.pw_banner_lbl.setStyleSheet("color: #e8eaed; font-size: 13px; border: none;")
        self.pw_save_btn = QPushButton("Kaydet")
        self.pw_save_btn.setStyleSheet(
            "background-color: #8ab4f8; color: #202124; border: none; border-radius: 14px; padding: 5px 14px; font-weight: 600;"
        )
        self.pw_save_btn.clicked.connect(self._accept_save_password)
        self.pw_later_btn = QPushButton("Şimdi Değil")
        self.pw_later_btn.setObjectName("NavBtn")
        self.pw_later_btn.clicked.connect(self._dismiss_save_password)
        self.pw_never_btn = QPushButton("Bu Sitede Asla")
        self.pw_never_btn.setObjectName("NavBtn")
        self.pw_never_btn.clicked.connect(self._never_save_password)
        pw_banner_layout.addWidget(pw_icon)
        pw_banner_layout.addWidget(self.pw_banner_lbl, 1)
        pw_banner_layout.addWidget(self.pw_save_btn)
        pw_banner_layout.addWidget(self.pw_later_btn)
        pw_banner_layout.addWidget(self.pw_never_btn)
        self.pw_banner.hide()
        root_layout.addWidget(self.pw_banner)

        # Thin load progress bar
        self.progress_bar = QProgressBar()
        self.progress_bar.setMaximumHeight(2)
        self.progress_bar.setTextVisible(False)
        self.progress_bar.hide()
        root_layout.addWidget(self.progress_bar)

        # Inline Chrome-style Find-in-Page bar
        self.find_bar = FindBar(self.current_tab, self)
        root_layout.addWidget(self.find_bar)

        # Tab viewport stack + Right Chrome Side Panel
        content_row = QWidget()
        content_layout = QHBoxLayout(content_row)
        content_layout.setContentsMargins(0, 0, 0, 0)
        content_layout.setSpacing(0)
        content_layout.addWidget(self.tabs.stackWidget(), 1)
        self.side_panel = ChromeSidePanel(
            history=self.history,
            bookmarks=self.bookmarks,
            on_open_url=lambda u: self.current_tab() and self._load_in_tab(self.current_tab(), u),
            on_open_in_tab=self._open_internal_tab,
            on_bookmarks_changed=lambda: (self._rebuild_bookmarks_bar(), self._update_bookmark_star()),
            parent=self,
        )
        content_layout.addWidget(self.side_panel)
        root_layout.addWidget(content_row, 1)
        self.setCentralWidget(central)

        # Status bar
        self.status_bar = QStatusBar()
        self.setStatusBar(self.status_bar)
        self.status_lbl = QLabel("AdBlock Kalkanı Hazır")
        self.status_bar.addPermanentWidget(self.status_lbl)

    def _setup_shortcuts(self) -> None:
        QShortcut(QKeySequence("Ctrl+T"), self).activated.connect(lambda: self.open_new_tab("adblock://home"))
        QShortcut(QKeySequence("Ctrl+W"), self).activated.connect(lambda: self.close_tab(self.tabs.currentIndex()))
        QShortcut(QKeySequence("Ctrl+L"), self).activated.connect(self._focus_url_bar)
        QShortcut(QKeySequence("Ctrl+R"), self).activated.connect(self._reload_current_tab)
        QShortcut(QKeySequence("Ctrl+D"), self).activated.connect(self._toggle_current_bookmark)
        QShortcut(QKeySequence("Ctrl+H"), self).activated.connect(self._open_history_dialog)
        QShortcut(QKeySequence("Ctrl+Shift+B"), self).activated.connect(self._toggle_bookmarks_bar)
        QShortcut(QKeySequence("Ctrl+Shift+T"), self).activated.connect(self._reopen_closed_tab)
        QShortcut(QKeySequence("Ctrl+Shift+N"), self).activated.connect(self.open_incognito_window)
        QShortcut(QKeySequence("Ctrl+F"), self).activated.connect(self.find_bar.open_and_focus)
        QShortcut(QKeySequence("Ctrl+J"), self).activated.connect(self._open_downloads_dialog)
        QShortcut(QKeySequence("Ctrl+P"), self).activated.connect(self._export_current_pdf)
        QShortcut(QKeySequence("Ctrl+S"), self).activated.connect(self._save_current_page)
        QShortcut(QKeySequence("Ctrl+U"), self).activated.connect(self._view_current_source)
        QShortcut(QKeySequence("F12"), self).activated.connect(self._open_devtools)
        QShortcut(QKeySequence("Ctrl+Shift+I"), self).activated.connect(self._open_devtools)
        QShortcut(QKeySequence("Ctrl+["), self).activated.connect(lambda: self.current_tab() and self.current_tab().back())
        QShortcut(QKeySequence("Ctrl+]"), self).activated.connect(lambda: self.current_tab() and self.current_tab().forward())
        QShortcut(QKeySequence("Ctrl+="), self).activated.connect(lambda: self._adjust_zoom(0.1))
        QShortcut(QKeySequence("Ctrl+-"), self).activated.connect(lambda: self._adjust_zoom(-0.1))
        QShortcut(QKeySequence("Ctrl+0"), self).activated.connect(lambda: self._set_zoom(1.0))
        QShortcut(QKeySequence("Escape"), self).activated.connect(self._on_escape_pressed)

    def current_tab(self) -> Optional[BrowserTab]:
        widget = self.tabs.currentWidget()
        return widget if isinstance(widget, BrowserTab) else None

    def open_new_tab(self, url: str = "adblock://home", switch_to: bool = True) -> BrowserTab:
        tab = BrowserTab(
            profile=self.profile,
            engine=self.engine,
            create_tab_callback=lambda: self.open_new_tab("", switch_to=True),
            pw_bridge=self.pw_bridge,
            cert_exception_hosts=self._cert_exception_hosts,
            parent=self,
        )
        idx = self.tabs.addTab(tab, "Yeni Sekme", on_close_cb=self.close_tab)

        tab.urlChanged.connect(lambda qurl, t=tab: self._on_tab_url_changed(t, qurl))
        tab.titleChanged.connect(lambda title, t=tab: self._on_tab_title_changed(t, title))
        tab.iconChanged.connect(lambda icon, t=tab: self._on_tab_icon_changed(t, icon))
        tab.loadStarted.connect(lambda t=tab: self._on_tab_load_started(t))
        tab.loadProgress.connect(lambda p, t=tab: self._on_tab_load_progress(t, p))
        tab.loadFinished.connect(lambda ok, t=tab: self._on_tab_load_finished(t, ok))
        tab.blockedCountChanged.connect(lambda _c, t=tab: self._on_tab_blocked_changed(t))
        tab.certificateRejected.connect(
            lambda u, d, ov, t=tab: self._on_certificate_rejected(t, u, d, ov)
        )
        tab.internalActionRequested.connect(
            lambda action_url, t=tab: self._on_internal_action(t, action_url)
        )
        tab.page().fullScreenRequested.connect(
            lambda req, t=tab: self._on_fullscreen_requested(t, req)
        )
        tab.page().recentlyAudibleChanged.connect(
            lambda _audible, t=tab: self._refresh_tab_title(t)
        )

        if switch_to:
            self.tabs.setCurrentIndex(idx)

        if url:
            self._load_in_tab(tab, url)
        return tab

    def _load_in_tab(self, tab: BrowserTab, url_or_text: str) -> None:
        clean_url = (url_or_text or "").strip().rstrip("/")
        if clean_url == "adblock://home":
            tab.is_home_page = True
            tab.internal_url = "adblock://home"
            html_content = render_home_html(
                total_blocked=self.engine.total_blocked,
                rules_count=self.engine.total_rules_count,
                adblock_enabled=self.engine.enabled,
                settings=self.settings,
                bookmarks=self.bookmarks,
            )
            tab.setHtml(html_content, QUrl("adblock://home"))
            if tab == self.current_tab():
                self.url_bar.setText("")
            idx_home = self.tabs.indexOf(tab)
            self.tabs.setTabText(idx_home, "Yeni Sekme")
            self.tabs.setTabIcon(idx_home, svg_icon("shield", "#8ab4f8", 14))
            self._update_shield_button()
            self._update_bookmark_star()
            self._update_security_badge()
            return

        internal_map = {
            "adblock://settings": (
                "Tarayıcı Ayarları",
                "settings",
                lambda: render_settings_page(self.settings, self.engine),
            ),
            "adblock://downloads": (
                "İndirilenler",
                "download",
                lambda: render_downloads_page(self.downloads),
            ),
            "adblock://passwords": (
                "Şifre Yöneticisi",
                "key",
                lambda: render_passwords_page(self.pw_manager),
            ),
            "adblock://history": (
                "Geçmiş",
                "history",
                lambda: render_history_page(self.history),
            ),
            "adblock://bookmarks": (
                "Yer İmleri",
                "bookmarks",
                lambda: render_bookmarks_page(self.bookmarks),
            ),
            "adblock://adblock": (
                "AdBlock Yönetimi",
                "shield",
                lambda: render_adblock_page(self.engine),
            ),
        }
        if clean_url in internal_map:
            title, icon_name, renderer = internal_map[clean_url]
            tab.is_home_page = False
            tab.internal_url = clean_url
            tab.setHtml(renderer(), QUrl(clean_url))
            if tab == self.current_tab():
                self.url_bar.setText(clean_url)
            idx_tab = self.tabs.indexOf(tab)
            self.tabs.setTabText(idx_tab, title)
            self.tabs.setTabIcon(idx_tab, svg_icon(icon_name, "#8ab4f8", 14))
            self._update_shield_button()
            self._update_bookmark_star()
            self._update_security_badge()
            return

        tab.is_home_page = False
        tab.internal_url = ""
        qurl = resolve_user_input_to_url(url_or_text, self.settings)
        tab.setUrl(qurl)

    def _open_internal_tab(self, internal_url: str) -> BrowserTab:
        """Focus existing internal adblock:// tab if open, otherwise open a new tab."""
        target = internal_url.rstrip("/")
        for i in range(self.tabs.count()):
            t = self.tabs.widget(i)
            if isinstance(t, BrowserTab) and (
                t.internal_url == target or t.url().toString().rstrip("/") == target
            ):
                self.tabs.setCurrentIndex(i)
                self._load_in_tab(t, target)
                return t
        return self.open_new_tab(target, switch_to=True)

    def close_tab(self, index: int) -> None:
        if index < 0:
            return
        if self.tabs.count() <= 1:
            # Reset last tab to Home instead of leaving window empty
            tab = self.current_tab()
            if tab:
                if not tab.is_home_page:
                    url_str = tab.url().toString()
                    if url_str.startswith(("http://", "https://")):
                        self._closed_tabs.append(url_str)
                self._load_in_tab(tab, "adblock://home")
            return
        widget = self.tabs.widget(index)
        if isinstance(widget, BrowserTab) and not widget.is_home_page:
            url_str = widget.url().toString()
            if url_str.startswith(("http://", "https://")):
                self._closed_tabs.append(url_str)
                if len(self._closed_tabs) > 30:
                    self._closed_tabs.pop(0)
        self.tabs.removeTab(index)
        if widget:
            if isinstance(widget, BrowserTab):
                page = widget.page()
                widget.setPage(None)
                if page is not None:
                    page.deleteLater()
            widget.deleteLater()

    def _focus_url_bar(self) -> None:
        self.url_bar.setFocus()
        self.url_bar.selectAll()

    def _navigate_from_url_bar(self) -> None:
        tab = self.current_tab()
        if not tab:
            tab = self.open_new_tab()
        self._load_in_tab(tab, self.url_bar.text())

    def _go_home(self) -> None:
        tab = self.current_tab()
        if tab:
            self._load_in_tab(tab, self.settings.get("homepage", "adblock://home"))

    def _reload_current_tab(self) -> None:
        tab = self.current_tab()
        if not tab:
            return
        url_str = tab.url().toString()
        if tab.is_home_page or url_str.startswith("adblock://"):
            self._load_in_tab(tab, url_str or "adblock://home")
        else:
            tab.reload()

    def _on_reload_or_stop(self) -> None:
        tab = self.current_tab()
        if not tab:
            return
        if self.progress_bar.isVisible():
            tab.stop()
        else:
            self._reload_current_tab()

    def _on_tab_url_changed(self, tab: BrowserTab, qurl: QUrl) -> None:
        url_str = qurl.toString()
        if url_str != "adblock://home":
            tab.is_home_page = False
        if tab == self.current_tab():
            self.url_bar.setText("" if tab.is_home_page else url_str)
            self._update_nav_buttons()
            self._update_bookmark_star()
            self._update_shield_button()
            self._update_security_badge()

    def _refresh_tab_title(self, tab: BrowserTab) -> None:
        idx = self.tabs.indexOf(tab)
        if idx < 0:
            return
        title = tab.title() or "Yeni Sekme"
        short = (title[:20] + "…") if len(title) > 21 else title
        self.tabs.setTabText(idx, short)
        self.tabs.setTabToolTip(idx, title)
        if tab.page().isAudioMuted():
            self.tabs.setTabIcon(idx, svg_icon("volume_off", "#f28b82", 14))
        elif tab.page().recentlyAudible():
            self.tabs.setTabIcon(idx, svg_icon("volume_on", "#8ab4f8", 14))
        elif tab.is_home_page:
            self.tabs.setTabIcon(idx, svg_icon("shield", "#8ab4f8", 14))
        elif tab.url().scheme() == "adblock":
            ico_name = {
                "settings": "settings",
                "downloads": "download",
                "passwords": "key",
                "history": "history",
                "bookmarks": "bookmarks",
            }.get(tab.url().host(), "shield")
            self.tabs.setTabIcon(idx, svg_icon(ico_name, "#8ab4f8", 14))
        else:
            self.tabs.setTabIcon(idx, tab.icon())

    def _on_tab_title_changed(self, tab: BrowserTab, title: str) -> None:
        self._refresh_tab_title(tab)
        if tab == self.current_tab():
            suffix = "Gizli Pencere — AdBlock Tarayıcı" if self.incognito else "AdBlock Tarayıcı"
            self.setWindowTitle(f"{title} — {suffix}" if title else suffix)

    def _on_tab_icon_changed(self, tab: BrowserTab, _icon: QIcon) -> None:
        self._refresh_tab_title(tab)

    def _on_tab_load_started(self, tab: BrowserTab) -> None:
        if tab == self.current_tab():
            self.progress_bar.setValue(5)
            self.progress_bar.show()
            self.reload_btn.setIcon(svg_icon("stop", "#e8eaed", 17))
            self._update_shield_button()

    def _on_tab_load_progress(self, tab: BrowserTab, progress: int) -> None:
        if tab == self.current_tab():
            self.progress_bar.setValue(progress)

    def _on_tab_load_finished(self, tab: BrowserTab, ok: bool) -> None:
        if tab == self.current_tab():
            self.progress_bar.hide()
            self.reload_btn.setIcon(svg_icon("reload", "#e8eaed", 17))
            self._update_nav_buttons()
            self._update_bookmark_star()
            self._update_shield_button()
            self._update_security_badge()

        url_str = tab.url().toString()
        if (
            not ok
            and not tab.is_security_interstitial
            and tab.last_https_upgrade_target
            and url_str == tab.last_https_upgrade_target
        ):
            host = extract_host(url_str)
            interstitial = build_https_failed_html(
                tab.last_https_upgrade_target,
                tab.last_https_upgrade_http_url,
                host,
            )
            tab.set_interstitial_html(interstitial, QUrl(url_str))
            return

        if ok and not tab.is_home_page and not tab.is_security_interstitial and not self.incognito and url_str.startswith(("http://", "https://")):
            self.history.record(tab.title(), url_str)
            self._refresh_omnibox_suggestions()

    def _on_current_tab_changed(self, _index: int) -> None:
        tab = self.current_tab()
        if not tab:
            return
        self.url_bar.setText("" if tab.is_home_page else tab.url().toString())
        title = tab.title()
        suffix = "Gizli Pencere — AdBlock Tarayıcı" if self.incognito else "AdBlock Tarayıcı"
        self.setWindowTitle(f"{title} — {suffix}" if title else suffix)
        self._update_nav_buttons()
        self._update_bookmark_star()
        self._update_shield_button()
        self._update_security_badge()

    def _update_nav_buttons(self) -> None:
        tab = self.current_tab()
        if not tab:
            self.back_btn.setEnabled(False)
            self.forward_btn.setEnabled(False)
            return
        self.back_btn.setEnabled(tab.history().canGoBack())
        self.forward_btn.setEnabled(tab.history().canGoForward())

    def _update_bookmark_star(self) -> None:
        tab = self.current_tab()
        if not tab or tab.is_home_page or tab.url().scheme() == "adblock":
            self.bookmark_star_btn.setIcon(svg_icon("star_outline", "#9aa0a6", 16))
            return
        is_bm = self.bookmarks.is_bookmarked(tab.url().toString())
        if is_bm:
            self.bookmark_star_btn.setIcon(svg_icon("star_filled", "#8ab4f8", 16))
        else:
            self.bookmark_star_btn.setIcon(svg_icon("star_outline", "#9aa0a6", 16))

    def _update_shield_button(self) -> None:
        tab = self.current_tab()
        count = tab.blocked_count if tab else 0
        host = tab.current_host() if tab else ""
        whitelisted = self.engine.is_site_whitelisted(host) if host else False

        if not self.engine.enabled:
            self.shield_btn.setIcon(svg_icon("shield_off", "#f28b82", 16))
            self.shield_btn.setText("Kapalı")
            self.shield_btn.setStyleSheet(
                "background-color: rgba(242, 139, 130, 0.16); color: #f28b82; border: 1px solid rgba(242, 139, 130, 0.4);"
            )
            self.status_lbl.setText(f"Reklam engelleme kapalı  |  Toplam engellenen: {self.engine.total_blocked:,}")
        elif whitelisted:
            self.shield_btn.setIcon(svg_icon("shield_off", "#fdd663", 16))
            self.shield_btn.setText("İzinli")
            self.shield_btn.setStyleSheet(
                "background-color: rgba(253, 214, 99, 0.16); color: #fdd663; border: 1px solid rgba(253, 214, 99, 0.4);"
            )
            self.status_lbl.setText(f"{host} beyaz listede  |  Toplam engellenen: {self.engine.total_blocked:,}")
        else:
            self.shield_btn.setIcon(svg_icon("shield", "#81c995", 16))
            self.shield_btn.setText(f"{count}")
            self.shield_btn.setStyleSheet(
                "background-color: rgba(129, 201, 149, 0.16); color: #81c995; border: 1px solid rgba(129, 201, 149, 0.4);"
            )
            self.status_lbl.setText(
                f"Koruma Aktif  |  Bu sekmede: {count}  |  Toplam engellenen: {self.engine.total_blocked:,}"
            )

    def _on_request_blocked(self, first_party_url: str, request_url: str, resource_type: str, rule: str) -> None:
        fp_host = extract_host(first_party_url)
        matched_any = False
        for i in range(self.tabs.count()):
            tab = self.tabs.widget(i)
            if isinstance(tab, BrowserTab):
                tab_host = tab.current_host()
                if tab_host and fp_host and (tab_host == fp_host or fp_host.endswith("." + tab_host) or tab_host.endswith("." + fp_host)):
                    tab.record_blocked(request_url, resource_type, rule)
                    matched_any = True

        if not matched_any:
            tab = self.current_tab()
            if tab and not tab.is_home_page:
                tab.record_blocked(request_url, resource_type, rule)

        self._update_shield_button()

    def _on_tab_blocked_changed(self, tab: BrowserTab) -> None:
        if tab == self.current_tab():
            self._update_shield_button()

    def _toggle_current_bookmark(self) -> None:
        tab = self.current_tab()
        if not tab or tab.is_home_page or tab.url().scheme() == "adblock":
            return
        url_str = tab.url().toString()
        if not url_str.startswith(("http://", "https://")):
            return
        self.bookmarks.toggle(tab.title(), url_str)
        self._update_bookmark_star()
        self._rebuild_bookmarks_bar()

    def _rebuild_bookmarks_bar(self) -> None:
        while self.bm_bar_layout.count():
            child = self.bm_bar_layout.takeAt(0)
            if child.widget():
                child.widget().deleteLater()

        for bm in self.bookmarks.items[:14]:
            btn = QPushButton((bm.title[:20] + "…") if len(bm.title) > 21 else bm.title)
            btn.setObjectName("BmPill")
            btn.setToolTip(bm.url)
            btn.clicked.connect(lambda _checked=False, u=bm.url: self._open_in_current_or_new(u))
            self.bm_bar_layout.addWidget(btn)

        self.bm_bar_layout.addStretch()

    def _open_in_current_or_new(self, url: str) -> None:
        tab = self.current_tab()
        if tab:
            self._load_in_tab(tab, url)
        else:
            self.open_new_tab(url)

    def _toggle_bookmarks_bar(self) -> None:
        visible = not self.bookmarks_bar.isVisible()
        self.bookmarks_bar.setVisible(visible)
        self.settings.set("show_bookmarks_bar", visible)

    def _sync_profile_scripts(self) -> None:
        """Install or remove early DocumentCreation YouTube & ApplicationWorld password scripts."""
        scripts = self.profile.scripts()
        for script_name in ("__adblock_youtube_early", "__adblock_password_manager"):
            for existing in scripts.find(script_name):
                scripts.remove(existing)

        should_enable = self.engine.enabled and not self.engine.is_site_whitelisted("youtube.com")
        if should_enable:
            yt_script = QWebEngineScript()
            yt_script.setName("__adblock_youtube_early")
            yt_script.setSourceCode(build_youtube_early_script())
            yt_script.setInjectionPoint(QWebEngineScript.InjectionPoint.DocumentCreation)
            yt_script.setWorldId(QWebEngineScript.ScriptWorldId.MainWorld)
            yt_script.setRunsOnSubFrames(False)
            scripts.insert(yt_script)

        if bool(self.settings.get("password_manager_enabled", True)):
            pw_script = QWebEngineScript()
            pw_script.setName("__adblock_password_manager")
            pw_script.setSourceCode(build_password_manager_script())
            pw_script.setInjectionPoint(QWebEngineScript.InjectionPoint.DocumentCreation)
            pw_script.setWorldId(QWebEngineScript.ScriptWorldId.ApplicationWorld)
            pw_script.setRunsOnSubFrames(True)
            scripts.insert(pw_script)

    def _open_shield_panel(self) -> None:
        pop = AdBlockPopover(
            engine=self.engine,
            settings=self.settings,
            current_tab=self.current_tab(),
            on_state_changed=self._on_adblock_state_changed,
            on_reload_tab=self._reload_current_tab,
            on_open_full_tab=lambda: self._open_internal_tab("adblock://adblock"),
            parent=self,
        )
        self._active_popover = pop
        pop.show_below(self.shield_btn)

    def _on_adblock_state_changed(self) -> None:
        self._sync_profile_scripts()
        self._update_shield_button()

    def _open_history_dialog(self) -> None:
        self.side_panel.toggle_mode("history")

    def _open_bookmarks_dialog(self) -> None:
        self.side_panel.toggle_mode("bookmarks")

    def _open_passwords_dialog(self) -> None:
        self._open_internal_tab("adblock://passwords")

    def _on_save_password_prompt(self, origin: str, username: str, password: str) -> None:
        self._pending_pw = (origin, username, password)
        host = extract_host(origin) or origin
        is_update = self.pw_manager.has_username_for_host(origin, username)
        user_disp = username or "(Kullanıcı adı yok)"
        if is_update:
            self.pw_banner_lbl.setText(f'"{host}" için "{user_disp}" şifresi güncellensin mi?')
            self.pw_save_btn.setText("Güncelle")
        else:
            self.pw_banner_lbl.setText(f'"{host}" için "{user_disp}" şifresi kaydedilsin mi?')
            self.pw_save_btn.setText("Kaydet")
        self.pw_banner.show()

    def _accept_save_password(self) -> None:
        if self._pending_pw:
            origin, username, password = self._pending_pw
            self.pw_manager.save_credential(origin, username, password)
            self._pending_pw = None
            self.status_bar.showMessage("Şifre şifreli olarak kaydedildi.", 4000)
        self.pw_banner.hide()

    def _dismiss_save_password(self) -> None:
        self._pending_pw = None
        self.pw_banner.hide()

    def _never_save_password(self) -> None:
        if self._pending_pw:
            origin, _, _ = self._pending_pw
            self.pw_manager.add_never_save(origin)
            self._pending_pw = None
        self.pw_banner.hide()

    def _open_settings_dialog(self) -> None:
        self._open_internal_tab("adblock://settings")

    def _on_download_requested(self, download: QWebEngineDownloadRequest) -> None:
        suggested = download.downloadFileName() or "indirilen_dosya"
        default_dir = str(Path.home() / "Downloads" / suggested)
        save_path, _ = QFileDialog.getSaveFileName(self, "Dosyayı Kaydet", default_dir)
        if not save_path:
            download.cancel()
            return
        download.setDownloadDirectory(os.path.dirname(save_path))
        download.setDownloadFileName(os.path.basename(save_path))
        download.accept()
        self.downloads.track(download, save_path)
        self.status_bar.showMessage(f"İndiriliyor: {os.path.basename(save_path)}", 4000)
        download.isFinishedChanged.connect(
            lambda: download.isFinished() and self.status_bar.showMessage(f"İndirme tamamlandı: {os.path.basename(save_path)}", 5000)
        )

    def _refresh_omnibox_suggestions(self) -> None:
        urls: List[str] = []
        seen: set[str] = set()
        for bm in self.bookmarks.items:
            if bm.url not in seen:
                seen.add(bm.url)
                urls.append(bm.url)
        for h in self.history.items[:200]:
            if h.url not in seen:
                seen.add(h.url)
                urls.append(h.url)
        self._url_completer_model.setStringList(urls)

    def _update_security_badge(self) -> None:
        tab = self.current_tab()
        doh_label = self.dns_guard.get_provider_name()
        if self.dns_guard.provider_id == "off":
            self.top_guard_pill.setIcon(svg_icon("shield_off", "#9aa0a6", 14))
            self.top_guard_pill.setText(" DoH Kapalı")
        else:
            self.top_guard_pill.setIcon(svg_icon("shield", "#8ab4f8", 14))
            self.top_guard_pill.setText(" DoH Korumalı")

        if not tab or tab.is_home_page or tab.url().scheme() == "adblock":
            self.security_badge.setIcon(svg_icon("shield", "#8ab4f8", 13))
            self.security_badge.setText(" AdBlock")
            self.security_badge.setStyleSheet(
                "QPushButton#SecChip { background-color: #2b2d30; color: #e8eaed; }"
                "QPushButton#SecChip:hover { background-color: #383a3e; }"
            )
            self.security_badge.setToolTip(f"Dahili Güvenli Sayfa\nDNS: {doh_label}")
            return
        url_str = tab.url().toString()
        if tab.last_cert_error:
            self.security_badge.setIcon(svg_icon("warning", "#f28b82", 13))
            self.security_badge.setText(" Güvensiz")
            self.security_badge.setStyleSheet(
                "QPushButton#SecChip { background-color: rgba(242, 139, 130, 0.16); color: #f28b82; }"
                "QPushButton#SecChip:hover { background-color: rgba(242, 139, 130, 0.25); }"
            )
            self.security_badge.setToolTip(
                f"Sertifika Hatası: {tab.last_cert_error}\nDNS: {doh_label} (Detaylar için tıklayın)"
            )
        elif url_str.startswith("https://"):
            self.security_badge.setIcon(svg_icon("lock", "#81c995", 13))
            self.security_badge.setText(" HTTPS")
            self.security_badge.setStyleSheet(
                "QPushButton#SecChip { background-color: #2b2d30; color: #81c995; }"
                "QPushButton#SecChip:hover { background-color: #383a3e; }"
            )
            self.security_badge.setToolTip(
                f"Güvenli Bağlantı (HTTPS)\nDNS: {doh_label} (Detaylar için tıklayın)"
            )
        elif url_str.startswith("http://"):
            self.security_badge.setIcon(svg_icon("warning", "#fdd663", 13))
            self.security_badge.setText(" HTTP")
            self.security_badge.setStyleSheet(
                "QPushButton#SecChip { background-color: rgba(253, 214, 99, 0.16); color: #fdd663; }"
                "QPushButton#SecChip:hover { background-color: rgba(253, 214, 99, 0.25); }"
            )
            self.security_badge.setToolTip(
                f"Şifrelenmemiş Bağlantı (HTTP)\nDNS: {doh_label} (Detaylar için tıklayın)"
            )
        else:
            self.security_badge.setIcon(svg_icon("info", "#9aa0a6", 13))
            self.security_badge.setText("")
            self.security_badge.setStyleSheet("")
            self.security_badge.setToolTip(f"{url_str}\nDNS: {doh_label}")

    def _open_tab_search_menu(self) -> None:
        menu = QMenu(self)
        menu.setStyleSheet(
            """
            QMenu { background-color: #292a2d; color: #e8eaed; border: 1px solid #3c4043; border-radius: 10px; padding: 6px; }
            QMenu::item { padding: 7px 14px; border-radius: 6px; }
            QMenu::item:selected { background-color: #35363a; }
            """
        )
        for i in range(self.tabs.count()):
            tab = self.tabs.widget(i)
            if isinstance(tab, BrowserTab):
                title = tab.title() or "Yeni Sekme"
                short = (title[:38] + "…") if len(title) > 39 else title
                prefix = "• " if i == self.tabs.currentIndex() else "  "
                act = QAction(f"{prefix}{short}", self)
                act.triggered.connect(lambda _=False, idx=i: self.tabs.setCurrentIndex(idx))
                menu.addAction(act)
        menu.exec(self.tab_search_btn.mapToGlobal(self.tab_search_btn.rect().bottomLeft()))

    def _on_https_upgraded(self, _first_party: str, http_url: str, https_url: str) -> None:
        req_host = extract_host(https_url)
        for i in range(self.tabs.count()):
            t = self.tabs.widget(i)
            if isinstance(t, BrowserTab):
                t_host = t.current_host()
                if not t_host or t_host == req_host or t == self.current_tab():
                    t.last_https_upgrade_http_url = http_url
                    t.last_https_upgrade_target = https_url
        self.status_bar.showMessage(f"HTTPS-Only: {req_host} HTTPS'e yükseltildi", 3000)

    def _on_dns_blocked(self, request_url: str, provider_name: str, reason: str) -> None:
        host = extract_host(request_url)
        tab = self.current_tab()
        if not tab:
            return
        interstitial = build_dns_blocked_html(request_url, host, provider_name, reason)
        tab.set_interstitial_html(interstitial, QUrl(request_url))
        self._update_security_badge()

    def _on_certificate_rejected(
        self, tab: BrowserTab, url_str: str, desc: str, is_overridable: bool
    ) -> None:
        host = extract_host(url_str)
        interstitial = build_cert_error_html(url_str, host, desc, is_overridable)
        tab.set_interstitial_html(interstitial, QUrl(url_str), cert_error=desc)
        if tab == self.current_tab():
            self._update_security_badge()

    def _on_internal_action(self, tab: BrowserTab, action_url: str) -> None:
        qurl = QUrl(action_url)
        scheme = qurl.scheme()
        action = qurl.host()
        params = parse_qs(qurl.query(), keep_blank_values=True)

        if scheme == "adblock":
            if action in ("home", "settings", "downloads", "passwords", "history", "bookmarks", "adblock"):
                self._load_in_tab(tab, f"adblock://{action}")
                return
            target = (params.get("url") or [""])[0]
            if not target:
                return
            host = extract_host(target)
            if action == "allow-dns":
                self.dns_guard.allow_host_override(host)
                self._load_in_tab(tab, target)
            elif action == "allow-http":
                self._toggle_http_allowed(host, True)
                self._load_in_tab(tab, target)
            elif action == "accept-cert":
                if host:
                    self._cert_exception_hosts.add(host)
                self._load_in_tab(tab, target)
            return

        if scheme == "adblock-cmd":
            if action == "save-settings":
                self.settings.set(
                    "search_engine",
                    (params.get("search_engine") or [self.settings.get("search_engine", "google")])[0],
                )
                self.settings.set(
                    "doh_provider",
                    (params.get("doh_provider") or [self.settings.get("doh_provider", "cloudflare_security")])[0],
                )
                hp = (params.get("homepage") or ["adblock://home"])[0].strip() or "adblock://home"
                self.settings.set("homepage", hp)
                self.settings.set("https_only_enabled", "https_only_enabled" in params)
                self.settings.set("adblock_enabled", "adblock_enabled" in params)
                self.settings.set("cosmetic_enabled", "cosmetic_enabled" in params)
                self.settings.set("show_bookmarks_bar", "show_bookmarks_bar" in params)
                self.settings.set("password_manager_enabled", "password_manager_enabled" in params)

                self.engine.enabled = bool(self.settings.get("adblock_enabled", True))
                self.engine.cosmetic_enabled = bool(self.settings.get("cosmetic_enabled", True))
                self.dns_guard.set_provider(str(self.settings.get("doh_provider", "cloudflare_security")))
                self.interceptor.https_only_enabled = bool(self.settings.get("https_only_enabled", True))
                self.bookmarks_bar.setVisible(bool(self.settings.get("show_bookmarks_bar", True)))
                self._on_adblock_state_changed()
                self._update_security_badge()
                self.status_bar.showMessage("Tarayıcı ayarları kaydedildi.", 3500)
                self._load_in_tab(tab, "adblock://settings")
            elif action in ("dl-open", "dl-reveal", "dl-cancel"):
                try:
                    idx = int((params.get("idx") or ["-1"])[0])
                except ValueError:
                    idx = -1
                if 0 <= idx < len(self.downloads.records):
                    rec = self.downloads.records[idx]
                    if action == "dl-open":
                        _open_local_path(rec.save_path)
                    elif action == "dl-reveal":
                        _reveal_in_folder(rec.save_path)
                    elif action == "dl-cancel" and rec.request is not None:
                        rec.request.cancel()
                        self._load_in_tab(tab, "adblock://downloads")
            elif action == "dl-clear":
                self.downloads.clear_finished()
                self._load_in_tab(tab, "adblock://downloads")
            elif action in ("pw-copy", "pw-del"):
                try:
                    idx = int((params.get("idx") or ["-1"])[0])
                except ValueError:
                    idx = -1
                if 0 <= idx < len(self.pw_manager.entries):
                    cred = self.pw_manager.entries[idx]
                    if action == "pw-copy":
                        cb = QApplication.clipboard()
                        if cb is not None:
                            cb.setText(cred.password)
                        self.status_bar.showMessage("Şifre panoya kopyalandı.", 3000)
                    else:
                        self.pw_manager.delete_credential(cred.origin, cred.username)
                        self._load_in_tab(tab, "adblock://passwords")
            elif action == "hist-clear":
                self.history.clear()
                if self.side_panel.isVisible() and self.side_panel.mode == "history":
                    self.side_panel.refresh()
                self._load_in_tab(tab, "adblock://history")
            elif action == "bm-add":
                title = (params.get("title") or [""])[0].strip()
                url = (params.get("url") or [""])[0].strip()
                if url:
                    if "://" not in url:
                        url = "https://" + url
                    self.bookmarks.add(title or url, url)
                    self._rebuild_bookmarks_bar()
                    self._update_bookmark_star()
                    if self.side_panel.isVisible() and self.side_panel.mode == "bookmarks":
                        self.side_panel.refresh()
                self._load_in_tab(tab, "adblock://bookmarks")
            elif action == "bm-del":
                try:
                    idx = int((params.get("idx") or ["-1"])[0])
                except ValueError:
                    idx = -1
                if 0 <= idx < len(self.bookmarks.items):
                    self.bookmarks.remove(self.bookmarks.items[idx].url)
                    self._rebuild_bookmarks_bar()
                    self._update_bookmark_star()
                    if self.side_panel.isVisible() and self.side_panel.mode == "bookmarks":
                        self.side_panel.refresh()
                self._load_in_tab(tab, "adblock://bookmarks")
            elif action == "ab-save-rules":
                rules_text = (params.get("rules") or [""])[0]
                self.engine.set_custom_rules(rules_text)
                self._on_adblock_state_changed()
                self.status_bar.showMessage("Özel filtre kuralları kaydedildi.", 3500)
                self._load_in_tab(tab, "adblock://adblock")
            elif action == "ab-wl-add":
                host = (params.get("host") or [""])[0].strip()
                if host:
                    self.engine.set_site_whitelisted(host, True)
                    self.settings.set("whitelisted_domains", sorted(self.engine.whitelisted_domains))
                    self._on_adblock_state_changed()
                self._load_in_tab(tab, "adblock://adblock")
            elif action == "ab-wl-del":
                host = (params.get("host") or [""])[0].strip()
                if host:
                    self.engine.set_site_whitelisted(host, False)
                    self.settings.set("whitelisted_domains", sorted(self.engine.whitelisted_domains))
                    self._on_adblock_state_changed()
                self._load_in_tab(tab, "adblock://adblock")
            elif action == "ab-update-lists":
                self.status_bar.showMessage("Filtre listeleri güncelleniyor...", 4000)
                worker = FilterUpdateThread(self.engine, self)
                self._ab_update_worker = worker

                def _done(_res: dict) -> None:
                    self._on_adblock_state_changed()
                    self.status_bar.showMessage(
                        f"Filtre listeleri güncellendi ({self.engine.total_rules_count:,} kural).",
                        5000,
                    )
                    if tab.internal_url == "adblock://adblock":
                        self._load_in_tab(tab, "adblock://adblock")

                worker.finished_with_results.connect(_done)
                worker.start()

    def _toggle_http_allowed(self, host: str, allowed: bool) -> None:
        if not host:
            return
        if allowed:
            self.interceptor.allow_http_for_host(host)
        else:
            self.interceptor.disallow_http_for_host(host)
        saved = set(self.settings.get("http_allowed_domains", []))
        clean = host.lower().strip(".")
        if allowed:
            saved.add(clean)
        else:
            saved.discard(clean)
        self.settings.set("http_allowed_domains", sorted(saved))

    def _toggle_adblock_whitelist(self, host: str, paused: bool) -> None:
        if not host:
            return
        self.engine.set_site_whitelisted(host, paused)
        self.settings.set("whitelisted_domains", sorted(self.engine.whitelisted_domains))
        self._on_adblock_state_changed()

    def _open_site_security_dialog(self) -> None:
        tab = self.current_tab()
        if not tab:
            return
        url_str = "adblock://home" if tab.is_home_page else tab.url().toString()
        scheme = QUrl(url_str).scheme() or "adblock"
        host = "" if tab.is_home_page or scheme == "adblock" else extract_host(url_str)
        dns_res = self.dns_guard.get_cached_result(host) if host else None
        pop = SiteSecurityPopover(
            url_str=url_str,
            host=host,
            scheme=scheme,
            dns_guard=self.dns_guard,
            dns_result=dns_res,
            cert_error_desc=tab.last_cert_error,
            https_only_enabled=self.interceptor.https_only_enabled,
            is_http_allowed=self.interceptor.is_http_allowed_for_host(host) if host else False,
            is_adblock_whitelisted=self.engine.is_site_whitelisted(host) if host else False,
            on_toggle_http_allowed=lambda h, a: (self._toggle_http_allowed(h, a), self._update_security_badge()),
            on_toggle_adblock_whitelist=self._toggle_adblock_whitelist,
            on_open_settings_tab=lambda: self._open_internal_tab("adblock://settings"),
            parent=self,
        )
        self._active_popover = pop
        pop.show_below(self.security_badge)

    def _reopen_closed_tab(self) -> None:
        if not self._closed_tabs:
            self.status_bar.showMessage("Geri açılacak kapatılmış sekme yok.", 2500)
            return
        url = self._closed_tabs.pop()
        self.open_new_tab(url, switch_to=True)

    def open_incognito_window(self) -> "MainWindow":
        win = MainWindow(data_dir=self.data_dir, incognito=True)
        self._child_windows.append(win)
        win.show()
        return win

    def _open_downloads_dialog(self) -> None:
        pop = DownloadsPopover(
            manager=self.downloads,
            on_open_full_tab=lambda: self._open_internal_tab("adblock://downloads"),
            parent=self,
        )
        self._active_popover = pop
        pop.show_below(self.downloads_btn)

    def _on_tab_context_menu(self, pos) -> None:
        idx = self.tabs.tabBar().tabAt(pos)
        if idx < 0:
            return
        tab = self.tabs.widget(idx)
        if not isinstance(tab, BrowserTab):
            return

        menu = QMenu(self)
        dup_act = QAction("Sekmeyi Çoğalt", self)
        dup_act.triggered.connect(lambda: self._duplicate_tab(tab))

        muted = tab.page().isAudioMuted()
        mute_act = QAction("Sekmenin Sesini Aç" if muted else "Sekmenin Sesini Kapat", self)
        mute_act.triggered.connect(lambda: self._toggle_tab_mute(tab))

        reload_act = QAction("Yenile", self)
        reload_act.triggered.connect(lambda: tab.reload() if not tab.is_home_page else self._load_in_tab(tab, "adblock://home"))

        reopen_act = QAction("Kapatılan Sekmeyi Geri Aç (Cmd+Shift+T)", self)
        reopen_act.setEnabled(bool(self._closed_tabs))
        reopen_act.triggered.connect(self._reopen_closed_tab)

        close_others_act = QAction("Diğer Sekmeleri Kapat", self)
        close_others_act.setEnabled(self.tabs.count() > 1)
        close_others_act.triggered.connect(lambda: self._close_other_tabs(tab))

        close_act = QAction("Sekmeyi Kapat", self)
        close_act.triggered.connect(lambda: self.close_tab(self.tabs.indexOf(tab)))

        menu.addAction(dup_act)
        menu.addAction(mute_act)
        menu.addAction(reload_act)
        menu.addSeparator()
        menu.addAction(reopen_act)
        menu.addAction(close_others_act)
        menu.addAction(close_act)
        menu.exec(self.tabs.tabBar().mapToGlobal(pos))

    def _duplicate_tab(self, tab: BrowserTab) -> None:
        url = "adblock://home" if tab.is_home_page else tab.url().toString()
        self.open_new_tab(url, switch_to=True)

    def _toggle_tab_mute(self, tab: BrowserTab) -> None:
        tab.page().setAudioMuted(not tab.page().isAudioMuted())
        self._refresh_tab_title(tab)

    def _close_other_tabs(self, keep_tab: BrowserTab) -> None:
        for i in range(self.tabs.count() - 1, -1, -1):
            w = self.tabs.widget(i)
            if w is not keep_tab:
                self.close_tab(i)

    def _open_chrome_menu(self) -> None:
        menu = QMenu(self)
        menu.setStyleSheet(
            """
            QMenu { background-color: #292a2d; color: #e8eaed; border: 1px solid #3c4043; border-radius: 8px; padding: 6px; }
            QMenu::item { padding: 7px 18px; border-radius: 6px; }
            QMenu::item:selected { background-color: #35363a; }
            QMenu::separator { height: 1px; background: #3c4043; margin: 4px 8px; }
            """
        )

        inc_act = QAction(svg_icon("incognito", "#e8eaed", 16), "Yeni Gizli Pencere (Cmd+Shift+N)", self)
        inc_act.triggered.connect(self.open_incognito_window)

        reopen_act = QAction(svg_icon("reopen", "#e8eaed", 16), "Kapatılan Sekmeyi Geri Aç (Cmd+Shift+T)", self)
        reopen_act.setEnabled(bool(self._closed_tabs))
        reopen_act.triggered.connect(self._reopen_closed_tab)

        find_act = QAction(svg_icon("search", "#e8eaed", 16), "Sayfada Bul (Cmd+F)", self)
        find_act.triggered.connect(self.find_bar.open_and_focus)

        dl_act = QAction(svg_icon("download", "#e8eaed", 16), "İndirilenler (Cmd+J)", self)
        dl_act.triggered.connect(lambda: self._open_internal_tab("adblock://downloads"))

        set_act = QAction(svg_icon("settings", "#e8eaed", 16), "Ayarlar", self)
        set_act.triggered.connect(lambda: self._open_internal_tab("adblock://settings"))

        pw_act = QAction(svg_icon("key", "#e8eaed", 16), "Şifre Yöneticisi", self)
        pw_act.triggered.connect(lambda: self._open_internal_tab("adblock://passwords"))

        hist_act = QAction(svg_icon("history", "#e8eaed", 16), "Geçmiş", self)
        hist_act.triggered.connect(lambda: self._open_internal_tab("adblock://history"))

        bm_act = QAction(svg_icon("bookmarks", "#e8eaed", 16), "Yer İmleri", self)
        bm_act.triggered.connect(lambda: self._open_internal_tab("adblock://bookmarks"))

        pdf_act = QAction(svg_icon("pdf", "#e8eaed", 16), "PDF Olarak Kaydet (Cmd+P)", self)
        pdf_act.triggered.connect(self._export_current_pdf)

        save_act = QAction(svg_icon("download", "#8ab4f8", 16), "Sayfayı Kaydet (Cmd+S)", self)
        save_act.triggered.connect(self._save_current_page)

        src_act = QAction(svg_icon("code", "#e8eaed", 16), "Sayfa Kaynağını Görüntüle (Cmd+U)", self)
        src_act.triggered.connect(self._view_current_source)

        dt_act = QAction(svg_icon("devtools", "#e8eaed", 16), "Geliştirici Araçları (F12)", self)
        dt_act.triggered.connect(self._open_devtools)

        menu.addAction(inc_act)
        menu.addAction(reopen_act)
        menu.addSeparator()
        menu.addAction(find_act)
        menu.addAction(dl_act)
        menu.addAction(hist_act)
        menu.addAction(bm_act)
        menu.addAction(pw_act)
        menu.addAction(set_act)
        menu.addSeparator()
        menu.addAction(pdf_act)
        menu.addAction(save_act)
        menu.addAction(src_act)
        menu.addAction(dt_act)
        menu.exec(self.menu_btn.mapToGlobal(self.menu_btn.rect().bottomLeft()))

    def _export_current_pdf(self) -> None:
        tab = self.current_tab()
        if not tab or tab.is_home_page or tab.url().scheme() == "adblock":
            return
        suggested = (tab.title() or "sayfa").replace("/", "_")[:50] + ".pdf"
        default_path = str(Path.home() / "Downloads" / suggested)
        save_path, _ = QFileDialog.getSaveFileName(self, "PDF Olarak Kaydet", default_path, "PDF (*.pdf)")
        if save_path:
            tab.page().printToPdf(save_path)
            self.status_bar.showMessage(f"PDF kaydedildi: {os.path.basename(save_path)}", 4000)

    def _save_current_page(self) -> None:
        tab = self.current_tab()
        if not tab or tab.is_home_page or tab.url().scheme() == "adblock":
            return
        suggested = (tab.title() or "sayfa").replace("/", "_")[:50] + ".html"
        default_path = str(Path.home() / "Downloads" / suggested)
        save_path, _ = QFileDialog.getSaveFileName(
            self,
            "Sayfayı Kaydet",
            default_path,
            "Web Sayfası (*.html);;Tek Dosya (*.mhtml)",
        )
        if save_path:
            fmt = (
                QWebEngineDownloadRequest.SavePageFormat.MimeHtmlSaveFormat
                if save_path.lower().endswith(".mhtml")
                else QWebEngineDownloadRequest.SavePageFormat.CompleteHtmlSaveFormat
            )
            tab.page().save(save_path, fmt)
            self.status_bar.showMessage(f"Sayfa kaydediliyor: {os.path.basename(save_path)}", 4000)

    def _view_current_source(self) -> None:
        tab = self.current_tab()
        if not tab or tab.is_home_page or tab.url().scheme() == "adblock":
            return
        url_str = tab.url().toString()
        if url_str and not url_str.startswith("view-source:"):
            self.open_new_tab("view-source:" + url_str, switch_to=True)

    def _open_devtools(self) -> None:
        tab = self.current_tab()
        if not tab:
            return
        dt = DevToolsWindow(tab.page(), parent=self)
        self._devtools_windows.append(dt)
        dt.show()

    def _adjust_zoom(self, delta: float) -> None:
        tab = self.current_tab()
        if tab:
            new_zoom = max(0.3, min(3.0, tab.zoomFactor() + delta))
            tab.setZoomFactor(new_zoom)
            self.status_bar.showMessage(f"Yakınlaştırma: {int(round(new_zoom * 100))}%", 2000)

    def _set_zoom(self, factor: float) -> None:
        tab = self.current_tab()
        if tab:
            tab.setZoomFactor(factor)
            self.status_bar.showMessage("Yakınlaştırma: 100%", 2000)

    def _on_fullscreen_requested(self, tab: BrowserTab, request: QWebEngineFullScreenRequest) -> None:
        request.accept()
        if request.toggleOn():
            idx = self.tabs.indexOf(tab)
            if idx >= 0:
                self.tabs.setCurrentIndex(idx)
            if not self._is_html_fullscreen:
                self._was_maximized_before_fullscreen = self.isMaximized()
                self._is_html_fullscreen = True
                self.tab_strip.hide()
                self.nav_toolbar.hide()
                self.bookmarks_bar.hide()
                self.side_panel.hide()
                self.status_bar.hide()
                self.showFullScreen()
        else:
            self._exit_html_fullscreen()

    def _exit_html_fullscreen(self) -> None:
        if not self._is_html_fullscreen:
            return
        self._is_html_fullscreen = False
        self.tab_strip.show()
        self.nav_toolbar.show()
        self.status_bar.show()
        self.bookmarks_bar.setVisible(bool(self.settings.get("show_bookmarks_bar", True)))
        if self._was_maximized_before_fullscreen:
            self.showMaximized()
        else:
            self.showNormal()

    def _on_escape_pressed(self) -> None:
        if self._is_html_fullscreen:
            tab = self.current_tab()
            if tab and tab.page():
                tab.page().triggerAction(QWebEnginePage.WebAction.ExitFullScreen)
            self._exit_html_fullscreen()
        elif not self.find_bar.isHidden():
            self.find_bar.close_bar()

    def _persist_stats(self) -> None:
        self.settings.set("total_blocked", self.engine.total_blocked)

    def closeEvent(self, event) -> None:
        self._persist_stats()
        self._stats_timer.stop()
        for dt in self._devtools_windows:
            dt.close()
            dt.deleteLater()
        self._devtools_windows.clear()
        # Delete all WebEngine pages/tabs before profile teardown
        while self.tabs.count() > 0:
            w = self.tabs.widget(0)
            self.tabs.removeTab(0)
            if w is not None:
                if isinstance(w, BrowserTab):
                    page = w.page()
                    w.setPage(None)
                    if page is not None:
                        page.deleteLater()
                w.deleteLater()
        super().closeEvent(event)
