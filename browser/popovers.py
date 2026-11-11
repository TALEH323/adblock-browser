"""Chrome-inspired anchored popovers and right docked Side Panel."""

from __future__ import annotations

import os
import subprocess
import sys
from typing import Callable, Optional

from PyQt6.QtCore import QPoint, QSize, Qt, QUrl
from PyQt6.QtGui import QDesktopServices, QGuiApplication
from PyQt6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QScrollArea,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from adblock.dns_guard import DnsCheckResult, DnsGuard
from adblock.engine import AdBlockEngine
from .downloads import DownloadsManager, format_bytes
from .icons import svg_icon, svg_pixmap
from .security_dialog import _CertFetchWorker
from .storage import BookmarksManager, HistoryManager, SettingsManager

_POPOVER_QSS = """
QFrame#ChromePopover {
    background-color: #202124;
    border: 1px solid #3c4043;
    border-radius: 14px;
    color: #e8eaed;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}
QLabel#PopTitle {
    color: #e8eaed;
    font-size: 15px;
    font-weight: 600;
}
QLabel#PopSub {
    color: #9aa0a6;
    font-size: 12px;
}
QPushButton#PopIconBtn {
    background-color: transparent;
    border: none;
    border-radius: 14px;
    min-width: 28px;
    max-width: 28px;
    min-height: 28px;
    max-height: 28px;
}
QPushButton#PopIconBtn:hover {
    background-color: #35363a;
}
QPushButton#PopRowBtn {
    background-color: transparent;
    color: #e8eaed;
    border: none;
    border-radius: 10px;
    text-align: left;
    padding: 8px 10px;
}
QPushButton#PopRowBtn:hover {
    background-color: #2d2e31;
}
QFrame#PopSep {
    background-color: #3c4043;
    min-height: 1px;
    max-height: 1px;
    margin: 4px 6px;
}
QFrame#PopCard {
    background-color: #292a2d;
    border: 1px solid #3c4043;
    border-radius: 10px;
}
QCheckBox {
    color: #e8eaed;
    font-size: 12.5px;
    spacing: 8px;
}
QCheckBox::indicator {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    border: 1px solid #5f6368;
    background-color: #202124;
}
QCheckBox::indicator:checked {
    background-color: #8ab4f8;
    border-color: #8ab4f8;
}
QScrollArea {
    background: transparent;
    border: none;
}
"""


def _open_local_path(path: str) -> None:
    if path and os.path.exists(path):
        QDesktopServices.openUrl(QUrl.fromLocalFile(path))


def _reveal_in_folder(path: str) -> None:
    if not path:
        return
    folder = os.path.dirname(path)
    if sys.platform == "darwin" and os.path.exists(path):
        subprocess.Popen(["open", "-R", path])
    elif sys.platform.startswith("win") and os.path.exists(path):
        subprocess.Popen(["explorer", "/select,", os.path.normpath(path)])
    elif folder and os.path.exists(folder):
        QDesktopServices.openUrl(QUrl.fromLocalFile(folder))


def _anchor_below(popup: QWidget, anchor: QWidget, align_right: bool = False) -> None:
    popup.adjustSize()
    global_pt = anchor.mapToGlobal(QPoint(0, anchor.height() + 6))
    x = global_pt.x()
    if align_right:
        x = global_pt.x() + anchor.width() - popup.width()
    y = global_pt.y()
    screen = QGuiApplication.screenAt(global_pt) or QGuiApplication.primaryScreen()
    if screen is not None:
        avail = screen.availableGeometry()
        x = max(avail.left() + 8, min(x, avail.right() - popup.width() - 8))
        y = max(avail.top() + 8, min(y, avail.bottom() - popup.height() - 8))
    popup.move(x, y)


def _make_menu_row(
    icon_name: str,
    icon_color: str,
    title: str,
    subtitle: str,
    right_icon: str,
    on_click: Callable[[], None],
) -> QPushButton:
    btn = QPushButton()
    btn.setObjectName("PopRowBtn")
    btn.setCursor(Qt.CursorShape.PointingHandCursor)
    btn.clicked.connect(on_click)

    lay = QHBoxLayout(btn)
    lay.setContentsMargins(10, 8, 10, 8)
    lay.setSpacing(12)

    left_ico = QLabel()
    left_ico.setPixmap(svg_pixmap(icon_name, icon_color, 18))
    left_ico.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
    lay.addWidget(left_ico)

    text_col = QVBoxLayout()
    text_col.setSpacing(1)
    t_lbl = QLabel(title)
    t_lbl.setStyleSheet("color: #e8eaed; font-size: 13.5px; font-weight: 500; background: transparent;")
    t_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
    text_col.addWidget(t_lbl)
    if subtitle:
        s_lbl = QLabel(subtitle)
        s_lbl.setStyleSheet("color: #9aa0a6; font-size: 11.5px; background: transparent;")
        s_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        text_col.addWidget(s_lbl)
    lay.addLayout(text_col, 1)

    right_ico = QLabel()
    right_ico.setPixmap(svg_pixmap(right_icon, "#9aa0a6", 15))
    right_ico.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
    lay.addWidget(right_ico)

    btn.setMinimumHeight(46 if subtitle else 40)
    return btn


class SiteSecurityPopover(QFrame):
    """Chrome-style site security & DoH popover anchored beneath the omnibox chip."""

    def __init__(
        self,
        url_str: str,
        host: str,
        scheme: str,
        dns_guard: DnsGuard,
        dns_result: Optional[DnsCheckResult],
        cert_error_desc: str,
        https_only_enabled: bool,
        is_http_allowed: bool,
        is_adblock_whitelisted: bool,
        on_toggle_http_allowed: Callable[[str, bool], None],
        on_toggle_adblock_whitelist: Callable[[str, bool], None],
        on_open_settings_tab: Callable[[], None],
        parent=None,
    ) -> None:
        super().__init__(parent, Qt.WindowType.Popup | Qt.WindowType.FramelessWindowHint)
        self.setObjectName("ChromePopover")
        self.setFixedWidth(340)
        self.setStyleSheet(_POPOVER_QSS)
        self.host = host
        self._worker: Optional[_CertFetchWorker] = None

        outer = QVBoxLayout(self)
        outer.setContentsMargins(10, 10, 10, 10)
        outer.setSpacing(0)

        self.stack = QStackedWidget()
        outer.addWidget(self.stack)

        is_https = scheme == "https"
        if cert_error_desc:
            conn_ico, conn_col, conn_title = "warning", "#f28b82", "Sertifika hatası"
        elif is_https:
            conn_ico, conn_col, conn_title = "lock", "#81c995", "Bağlantı güvenli"
        elif scheme == "adblock":
            conn_ico, conn_col, conn_title = "shield", "#8ab4f8", "Dahili güvenli sayfa"
        else:
            conn_ico, conn_col, conn_title = "warning", "#fdd663", "Bağlantı güvenli değil (HTTP)"

        # Page 0: Chrome Site Info Main Menu
        p0 = QWidget()
        l0 = QVBoxLayout(p0)
        l0.setContentsMargins(2, 2, 2, 2)
        l0.setSpacing(4)

        hdr0 = QHBoxLayout()
        hdr0.setContentsMargins(8, 2, 2, 6)
        title0 = QLabel(host or "AdBlock Tarayıcı")
        title0.setObjectName("PopTitle")
        close0 = QPushButton()
        close0.setObjectName("PopIconBtn")
        close0.setIcon(svg_icon("close", "#9aa0a6", 15))
        close0.clicked.connect(self.close)
        hdr0.addWidget(title0, 1)
        hdr0.addWidget(close0)
        l0.addLayout(hdr0)

        l0.addWidget(
            _make_menu_row(
                conn_ico,
                conn_col,
                conn_title,
                "",
                "chevron_right",
                lambda: self.stack.setCurrentIndex(1),
            )
        )

        doh_short = dns_guard.get_provider_name().split("(")[0].strip()
        l0.addWidget(
            _make_menu_row(
                "shield",
                "#8ab4f8",
                "DNS Koruması ve İzinler",
                f"DoH: {doh_short}",
                "chevron_right",
                lambda: self.stack.setCurrentIndex(2),
            )
        )

        def _open_settings() -> None:
            self.close()
            on_open_settings_tab()

        l0.addWidget(
            _make_menu_row(
                "settings",
                "#bdc1c6",
                "Site ayarları",
                "",
                "external",
                _open_settings,
            )
        )

        sep = QFrame()
        sep.setObjectName("PopSep")
        l0.addWidget(sep)

        l0.addWidget(
            _make_menu_row(
                "info",
                "#bdc1c6",
                "Bu sayfa hakkında",
                "Sertifika, şifreleme ve DNS özeti",
                "external",
                lambda: self.stack.setCurrentIndex(1),
            )
        )
        self.stack.addWidget(p0)

        # Page 1: Certificate Details Subpage
        p1 = QWidget()
        l1 = QVBoxLayout(p1)
        l1.setContentsMargins(2, 2, 2, 4)
        l1.setSpacing(10)
        hdr1 = QHBoxLayout()
        back1 = QPushButton()
        back1.setObjectName("PopIconBtn")
        back1.setIcon(svg_icon("chevron_left", "#e8eaed", 16))
        back1.clicked.connect(lambda: self.stack.setCurrentIndex(0))
        t1 = QLabel(conn_title)
        t1.setObjectName("PopTitle")
        close1 = QPushButton()
        close1.setObjectName("PopIconBtn")
        close1.setIcon(svg_icon("close", "#9aa0a6", 15))
        close1.clicked.connect(self.close)
        hdr1.addWidget(back1)
        hdr1.addWidget(t1, 1)
        hdr1.addWidget(close1)
        l1.addLayout(hdr1)

        cert_card = QFrame()
        cert_card.setObjectName("PopCard")
        cc_lay = QVBoxLayout(cert_card)
        cc_lay.setContentsMargins(12, 10, 12, 10)
        self.cert_detail_lbl = QLabel()
        self.cert_detail_lbl.setObjectName("PopSub")
        self.cert_detail_lbl.setWordWrap(True)
        cc_lay.addWidget(self.cert_detail_lbl)
        l1.addWidget(cert_card)
        l1.addStretch()

        if cert_error_desc:
            self.cert_detail_lbl.setText(f"Sertifika Hatası:\n{cert_error_desc}")
        elif is_https and host:
            self.cert_detail_lbl.setText("TLS sertifikası doğrulanıyor...")
            self._worker = _CertFetchWorker(host, 443, self)
            self._worker.fetched.connect(self._on_cert_fetched)
            self._worker.start()
        elif scheme == "adblock":
            self.cert_detail_lbl.setText("Yerel tarayıcı sayfası; ağ trafiği şifresiz dış sunucuya gitmez.")
        else:
            self.cert_detail_lbl.setText(
                "Bağlantı HTTP üzerinden (şifresiz) kuruldu. Parola veya kart bilgisi girmeyin."
            )
        self.stack.addWidget(p1)

        # Page 2: DNS & Site Toggles Subpage
        p2 = QWidget()
        l2 = QVBoxLayout(p2)
        l2.setContentsMargins(2, 2, 2, 4)
        l2.setSpacing(10)
        hdr2 = QHBoxLayout()
        back2 = QPushButton()
        back2.setObjectName("PopIconBtn")
        back2.setIcon(svg_icon("chevron_left", "#e8eaed", 16))
        back2.clicked.connect(lambda: self.stack.setCurrentIndex(0))
        t2 = QLabel("DNS ve Site Kontrolleri")
        t2.setObjectName("PopTitle")
        close2 = QPushButton()
        close2.setObjectName("PopIconBtn")
        close2.setIcon(svg_icon("close", "#9aa0a6", 15))
        close2.clicked.connect(self.close)
        hdr2.addWidget(back2)
        hdr2.addWidget(t2, 1)
        hdr2.addWidget(close2)
        l2.addLayout(hdr2)

        dns_card = QFrame()
        dns_card.setObjectName("PopCard")
        dc_lay = QVBoxLayout(dns_card)
        dc_lay.setContentsMargins(12, 10, 12, 10)
        dc_lay.setSpacing(4)
        dc_title = QLabel(dns_guard.get_provider_name())
        dc_title.setStyleSheet("color: #e8eaed; font-size: 12.5px; font-weight: 600;")
        dc_lay.addWidget(dc_title)
        if dns_guard.provider_id == "off":
            dns_txt = "DoH DNS koruması kapalı."
        elif dns_result is None:
            dns_txt = "Bu alan adı için henüz harici DoH kaydı yok."
        elif dns_result.blocked:
            dns_txt = f"ENGELLENDİ: {dns_result.reason}"
        else:
            dns_txt = f"Temiz: {dns_result.reason}"
        dc_sub = QLabel(dns_txt)
        dc_sub.setObjectName("PopSub")
        dc_sub.setWordWrap(True)
        dc_lay.addWidget(dc_sub)
        l2.addWidget(dns_card)

        if host:
            toggles = QFrame()
            toggles.setObjectName("PopCard")
            tg_lay = QVBoxLayout(toggles)
            tg_lay.setContentsMargins(12, 10, 12, 10)
            tg_lay.setSpacing(8)
            http_cb = QCheckBox("Bu sitede güvensiz HTTP'ye izin ver")
            http_cb.setChecked(is_http_allowed)
            http_cb.setEnabled(https_only_enabled)
            http_cb.toggled.connect(lambda c: on_toggle_http_allowed(self.host, c))
            tg_lay.addWidget(http_cb)

            ab_cb = QCheckBox("Bu sitede AdBlock korumasını duraklat")
            ab_cb.setChecked(is_adblock_whitelisted)
            ab_cb.toggled.connect(lambda c: on_toggle_adblock_whitelist(self.host, c))
            tg_lay.addWidget(ab_cb)
            l2.addWidget(toggles)

        l2.addWidget(
            _make_menu_row(
                "settings",
                "#8ab4f8",
                "Tüm güvenlik ve DNS ayarları",
                "",
                "external",
                _open_settings,
            )
        )
        self.stack.addWidget(p2)

    def show_below(self, anchor: QWidget) -> None:
        _anchor_below(self, anchor, align_right=False)
        self.show()

    def _on_cert_fetched(self, info: dict) -> None:
        if info.get("ok"):
            self.cert_detail_lbl.setText(
                f"Verilen: {info.get('common_name')}\n"
                f"Sağlayıcı: {info.get('issuer_org')}\n"
                f"Protokol: {info.get('protocol')} ({info.get('cipher')})\n"
                f"Geçerlilik: {info.get('not_before')} -> {info.get('not_after')}"
            )
        else:
            self.cert_detail_lbl.setText(f"Sertifika özeti alınamadı: {info.get('error', '')}")

    def closeEvent(self, event) -> None:
        if self._worker and self._worker.isRunning():
            self._worker.wait(400)
        super().closeEvent(event)


class DownloadsPopover(QFrame):
    """Chrome 'Son İndirme Geçmişi' popover bubble opened on first toolbar click."""

    def __init__(
        self,
        manager: DownloadsManager,
        on_open_full_tab: Callable[[], None],
        parent=None,
    ) -> None:
        super().__init__(parent, Qt.WindowType.Popup | Qt.WindowType.FramelessWindowHint)
        self.setObjectName("ChromePopover")
        self.setFixedWidth(365)
        self.setStyleSheet(_POPOVER_QSS)
        self.manager = manager
        self.on_open_full_tab = on_open_full_tab

        layout = QVBoxLayout(self)
        layout.setContentsMargins(12, 10, 12, 10)
        layout.setSpacing(8)

        hdr = QHBoxLayout()
        title = QLabel("Son İndirme Geçmişi")
        title.setObjectName("PopTitle")
        close_btn = QPushButton()
        close_btn.setObjectName("PopIconBtn")
        close_btn.setIcon(svg_icon("close", "#9aa0a6", 15))
        close_btn.clicked.connect(self.close)
        hdr.addWidget(title, 1)
        hdr.addWidget(close_btn)
        layout.addLayout(hdr)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setMaximumHeight(290)
        list_host = QWidget()
        list_host.setStyleSheet("background: transparent;")
        self.list_layout = QVBoxLayout(list_host)
        self.list_layout.setContentsMargins(0, 0, 0, 0)
        self.list_layout.setSpacing(4)
        scroll.setWidget(list_host)
        layout.addWidget(scroll)

        self._populate()
        manager.downloadsChanged.connect(self._populate)

        sep = QFrame()
        sep.setObjectName("PopSep")
        layout.addWidget(sep)

        def _open_full() -> None:
            self.close()
            self.on_open_full_tab()

        layout.addWidget(
            _make_menu_row(
                "download",
                "#8ab4f8",
                "Tüm İndirme Geçmişi",
                "",
                "external",
                _open_full,
            )
        )

    def _populate(self) -> None:
        while self.list_layout.count():
            item = self.list_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()

        records = self.manager.records[:8]
        if not records:
            empty = QLabel("Henüz indirilen dosya yok.")
            empty.setObjectName("PopSub")
            empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
            empty.setContentsMargins(8, 22, 8, 22)
            self.list_layout.addWidget(empty)
            return

        for rec in records:
            row = QFrame()
            row.setStyleSheet(
                "QFrame { background-color: transparent; border-radius: 10px; }"
                "QFrame:hover { background-color: #2d2e31; }"
            )
            rl = QHBoxLayout(row)
            rl.setContentsMargins(8, 7, 6, 7)
            rl.setSpacing(10)

            ico = QLabel()
            ico.setPixmap(svg_pixmap("file", "#bdc1c6", 18))
            rl.addWidget(ico)

            col = QVBoxLayout()
            col.setSpacing(1)
            short_name = (rec.file_name[:34] + "…") if len(rec.file_name) > 35 else rec.file_name
            name_lbl = QLabel(short_name)
            name_lbl.setStyleSheet("color: #e8eaed; font-size: 12.5px; font-weight: 500; background: transparent;")
            if rec.total_bytes > 0:
                size_str = f"{format_bytes(rec.received_bytes)} / {format_bytes(rec.total_bytes)}"
            else:
                size_str = format_bytes(rec.received_bytes)
            sub_lbl = QLabel(f"{size_str} • {rec.state_label}")
            sub_lbl.setStyleSheet("color: #9aa0a6; font-size: 11px; background: transparent;")
            col.addWidget(name_lbl)
            col.addWidget(sub_lbl)
            rl.addLayout(col, 1)

            folder_btn = QPushButton()
            folder_btn.setObjectName("PopIconBtn")
            folder_btn.setIcon(svg_icon("folder", "#bdc1c6", 14))
            folder_btn.setToolTip("Klasörde Göster")
            folder_btn.clicked.connect(lambda _=False, p=rec.save_path: _reveal_in_folder(p))
            rl.addWidget(folder_btn)

            open_btn = QPushButton()
            open_btn.setObjectName("PopIconBtn")
            open_btn.setIcon(svg_icon("external", "#8ab4f8", 14))
            open_btn.setToolTip("Dosyayı Aç")
            open_btn.clicked.connect(lambda _=False, p=rec.save_path: _open_local_path(p))
            rl.addWidget(open_btn)

            self.list_layout.addWidget(row)

    def show_below(self, anchor: QWidget) -> None:
        _anchor_below(self, anchor, align_right=True)
        self.show()


class ChromeSidePanel(QFrame):
    """Right-docked Chrome Side Panel for History and Bookmarks (narrows left viewport)."""

    def __init__(
        self,
        history: HistoryManager,
        bookmarks: BookmarksManager,
        on_open_url: Callable[[str], None],
        on_open_in_tab: Callable[[str], None],
        on_bookmarks_changed: Callable[[], None],
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.history = history
        self.bookmarks = bookmarks
        self.on_open_url = on_open_url
        self.on_open_in_tab = on_open_in_tab
        self.on_bookmarks_changed = on_bookmarks_changed
        self.mode: str = "history"

        self.setFixedWidth(335)
        self.setStyleSheet(
            """
            ChromeSidePanel {
                background-color: #202124;
                border-left: 1px solid #3c4043;
                color: #e8eaed;
            }
            QComboBox#SideModeCombo {
                background-color: transparent;
                color: #e8eaed;
                border: none;
                border-radius: 8px;
                padding: 4px 8px;
                font-size: 14px;
                font-weight: 600;
            }
            QComboBox#SideModeCombo:hover {
                background-color: #2d2e31;
            }
            QComboBox#SideModeCombo QAbstractItemView {
                background-color: #292a2d;
                color: #e8eaed;
                border: 1px solid #3c4043;
                selection-background-color: #35363a;
            }
            QLineEdit#SideSearch {
                background-color: #292a2d;
                color: #e8eaed;
                border: 1px solid #3c4043;
                border-radius: 16px;
                padding: 6px 12px;
                font-size: 12.5px;
            }
            QLineEdit#SideSearch:focus {
                border-color: #8ab4f8;
            }
            QPushButton#PopIconBtn {
                background-color: transparent;
                border: none;
                border-radius: 14px;
                min-width: 28px;
                max-width: 28px;
                min-height: 28px;
                max-height: 28px;
            }
            QPushButton#PopIconBtn:hover {
                background-color: #35363a;
            }
            QScrollArea {
                background: transparent;
                border: none;
            }
            """
        )

        root = QVBoxLayout(self)
        root.setContentsMargins(12, 10, 12, 10)
        root.setSpacing(10)

        hdr = QHBoxLayout()
        hdr.setSpacing(4)
        self.mode_combo = QComboBox()
        self.mode_combo.setObjectName("SideModeCombo")
        self.mode_combo.addItem("Geçmiş", "history")
        self.mode_combo.addItem("Yer İmleri", "bookmarks")
        self.mode_combo.currentIndexChanged.connect(self._on_mode_combo_changed)

        self.open_tab_btn = QPushButton()
        self.open_tab_btn.setObjectName("PopIconBtn")
        self.open_tab_btn.setIcon(svg_icon("external", "#bdc1c6", 15))
        self.open_tab_btn.setToolTip("Tam Sayfayı Sekmede Aç")
        self.open_tab_btn.clicked.connect(
            lambda: self.on_open_in_tab(f"adblock://{self.mode}")
        )

        close_btn = QPushButton()
        close_btn.setObjectName("PopIconBtn")
        close_btn.setIcon(svg_icon("close", "#bdc1c6", 15))
        close_btn.setToolTip("Yan Paneli Kapat")
        close_btn.clicked.connect(self.hide)

        hdr.addWidget(self.mode_combo, 1)
        hdr.addWidget(self.open_tab_btn)
        hdr.addWidget(close_btn)
        root.addLayout(hdr)

        self.search_edit = QLineEdit()
        self.search_edit.setObjectName("SideSearch")
        self.search_edit.setPlaceholderText("Ara...")
        self.search_edit.textChanged.connect(self.refresh)
        root.addWidget(self.search_edit)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        host = QWidget()
        host.setStyleSheet("background: transparent;")
        self.items_layout = QVBoxLayout(host)
        self.items_layout.setContentsMargins(0, 0, 2, 0)
        self.items_layout.setSpacing(4)
        scroll.setWidget(host)
        root.addWidget(scroll, 1)

        self.hide()

    def toggle_mode(self, mode: str) -> None:
        if self.isVisible() and self.mode == mode:
            self.hide()
            return
        self.mode = mode
        idx = self.mode_combo.findData(mode)
        if idx >= 0:
            self.mode_combo.blockSignals(True)
            self.mode_combo.setCurrentIndex(idx)
            self.mode_combo.blockSignals(False)
        self.search_edit.clear()
        self.refresh()
        self.show()

    def _on_mode_combo_changed(self) -> None:
        self.mode = self.mode_combo.currentData() or "history"
        self.refresh()

    def refresh(self) -> None:
        while self.items_layout.count():
            item = self.items_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()

        query = self.search_edit.text().strip().lower()
        if self.mode == "history":
            entries = self.history.search(query)[:80]
            if not entries:
                self._add_empty("Kayıtlı geçmiş bulunamadı.")
                return
            for entry in entries:
                self._add_row(
                    title=entry.title or entry.url,
                    subtitle=f"{entry.url} • {entry.visited_at[11:16]}",
                    url=entry.url,
                    on_delete=None,
                )
        else:
            bms = [
                b
                for b in self.bookmarks.items
                if not query or query in b.title.lower() or query in b.url.lower()
            ]
            if not bms:
                self._add_empty("Yer imi bulunamadı.")
                return
            for bm in bms:
                self._add_row(
                    title=bm.title or bm.url,
                    subtitle=bm.url,
                    url=bm.url,
                    on_delete=lambda u=bm.url: (
                        self.bookmarks.remove(u),
                        self.on_bookmarks_changed(),
                        self.refresh(),
                    ),
                )
        self.items_layout.addStretch()

    def _add_empty(self, text: str) -> None:
        lbl = QLabel(text)
        lbl.setStyleSheet("color: #9aa0a6; font-size: 12.5px; padding: 20px;")
        lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.items_layout.addWidget(lbl)
        self.items_layout.addStretch()

    def _add_row(
        self,
        title: str,
        subtitle: str,
        url: str,
        on_delete: Optional[Callable[[], None]],
    ) -> None:
        row = QFrame()
        row.setMinimumHeight(54)
        row.setStyleSheet(
            "QFrame { background-color: #292a2d; border-radius: 10px; border: 1px solid transparent; }"
            "QFrame:hover { background-color: #323438; border-color: #3c4043; }"
        )
        rl = QHBoxLayout(row)
        rl.setContentsMargins(10, 7, 8, 7)
        rl.setSpacing(6)

        open_btn = QPushButton()
        open_btn.setMinimumHeight(40)
        open_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        open_btn.setStyleSheet("background: transparent; border: none; text-align: left; padding: 0;")
        open_btn.clicked.connect(lambda _=False, u=url: self.on_open_url(u))
        bl = QVBoxLayout(open_btn)
        bl.setContentsMargins(0, 2, 0, 2)
        bl.setSpacing(2)
        t_lbl = QLabel((title[:32] + "…") if len(title) > 33 else title)
        t_lbl.setStyleSheet("color: #e8eaed; font-size: 12.5px; font-weight: 500; background: transparent;")
        t_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        s_lbl = QLabel((subtitle[:40] + "…") if len(subtitle) > 41 else subtitle)
        s_lbl.setStyleSheet("color: #9aa0a6; font-size: 11px; background: transparent;")
        s_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        bl.addWidget(t_lbl)
        bl.addWidget(s_lbl)
        rl.addWidget(open_btn, 1)

        if on_delete is not None:
            del_btn = QPushButton()
            del_btn.setObjectName("PopIconBtn")
            del_btn.setIcon(svg_icon("trash", "#9aa0a6", 13))
            del_btn.setToolTip("Kaldır")
            del_btn.clicked.connect(lambda _=False, fn=on_delete: fn())
            rl.addWidget(del_btn)

        self.items_layout.addWidget(row)


class AdBlockPopover(QFrame):
    """Chrome-style anchored AdBlock Shield popover bubble with blocked-request drilldown."""

    def __init__(
        self,
        engine: AdBlockEngine,
        settings: SettingsManager,
        current_tab,
        on_state_changed: Callable[[], None],
        on_reload_tab: Callable[[], None],
        on_open_full_tab: Callable[[], None],
        parent=None,
    ) -> None:
        super().__init__(parent, Qt.WindowType.Popup | Qt.WindowType.FramelessWindowHint)
        self.setObjectName("ChromePopover")
        self.setFixedWidth(365)
        self.setStyleSheet(_POPOVER_QSS)
        self.engine = engine
        self.settings = settings
        self.current_tab = current_tab
        self.on_state_changed = on_state_changed
        self.on_reload_tab = on_reload_tab
        self.on_open_full_tab = on_open_full_tab

        outer = QVBoxLayout(self)
        outer.setContentsMargins(12, 10, 12, 10)
        outer.setSpacing(0)

        self.stack = QStackedWidget()
        outer.addWidget(self.stack)

        # Page 0: Main Shield Overview Bubble
        p0 = QWidget()
        l0 = QVBoxLayout(p0)
        l0.setContentsMargins(0, 0, 0, 0)
        l0.setSpacing(10)

        hdr = QHBoxLayout()
        hdr_ico = QLabel()
        self._hdr_ico = hdr_ico
        title_lbl = QLabel("AdBlock Kalkanı")
        title_lbl.setObjectName("PopTitle")
        reload_btn = QPushButton()
        reload_btn.setObjectName("PopIconBtn")
        reload_btn.setIcon(svg_icon("reload", "#bdc1c6", 14))
        reload_btn.setToolTip("Sayfayı Yenile")
        reload_btn.clicked.connect(lambda: (self.close(), self.on_reload_tab()))
        close_btn = QPushButton()
        close_btn.setObjectName("PopIconBtn")
        close_btn.setIcon(svg_icon("close", "#9aa0a6", 15))
        close_btn.clicked.connect(self.close)
        hdr.addWidget(hdr_ico)
        hdr.addSpacing(4)
        hdr.addWidget(title_lbl, 1)
        hdr.addWidget(reload_btn)
        hdr.addWidget(close_btn)
        l0.addLayout(hdr)

        # Site Status Card
        site_card = QFrame()
        site_card.setObjectName("PopCard")
        sc_lay = QHBoxLayout(site_card)
        sc_lay.setContentsMargins(12, 10, 10, 10)
        sc_lay.setSpacing(8)
        sc_col = QVBoxLayout()
        sc_col.setSpacing(2)
        self.site_lbl = QLabel("Site: -")
        self.site_lbl.setStyleSheet("color: #e8eaed; font-size: 13px; font-weight: 600;")
        self.site_sub_lbl = QLabel("")
        self.site_sub_lbl.setObjectName("PopSub")
        self.site_sub_lbl.setWordWrap(True)
        sc_col.addWidget(self.site_lbl)
        sc_col.addWidget(self.site_sub_lbl)
        self.site_toggle_btn = QPushButton("Duraklat")
        self.site_toggle_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        self.site_toggle_btn.clicked.connect(self._on_toggle_site)
        sc_lay.addLayout(sc_col, 1)
        sc_lay.addWidget(self.site_toggle_btn)
        l0.addWidget(site_card)

        # Stats Row
        stats_card = QFrame()
        stats_card.setObjectName("PopCard")
        st_lay = QHBoxLayout(stats_card)
        st_lay.setContentsMargins(8, 10, 8, 10)
        st_lay.setSpacing(6)
        self.tab_blocked_lbl = self._make_stat_col(st_lay, "Bu Sekmede", "#81c995")
        self.total_blocked_lbl = self._make_stat_col(st_lay, "Toplam", "#8ab4f8")
        self.rules_count_lbl = self._make_stat_col(st_lay, "Kurallar", "#d2a8ff")
        l0.addWidget(stats_card)

        # Quick Global Toggles Card
        toggles_card = QFrame()
        toggles_card.setObjectName("PopCard")
        tg_lay = QVBoxLayout(toggles_card)
        tg_lay.setContentsMargins(12, 10, 12, 10)
        tg_lay.setSpacing(8)
        self.global_cb = QCheckBox("Genel Reklam Engelleme Aktif")
        self.global_cb.setChecked(self.engine.enabled)
        self.global_cb.toggled.connect(self._on_global_toggled)
        self.cosmetic_cb = QCheckBox("Kozmetik && YouTube Filtreleme Aktif")
        self.cosmetic_cb.setChecked(self.engine.cosmetic_enabled)
        self.cosmetic_cb.toggled.connect(self._on_cosmetic_toggled)
        tg_lay.addWidget(self.global_cb)
        tg_lay.addWidget(self.cosmetic_cb)
        l0.addWidget(toggles_card)

        # Drill-down & Full Tab Links
        tab_count = len(self.current_tab.blocked_items) if self.current_tab else 0
        self.blocked_row_btn = _make_menu_row(
            "shield",
            "#81c995",
            "Engellenen İstekler",
            f"Bu sekmede {tab_count} istek engellendi",
            "chevron_right",
            lambda: self.stack.setCurrentIndex(1),
        )
        l0.addWidget(self.blocked_row_btn)

        sep = QFrame()
        sep.setObjectName("PopSep")
        l0.addWidget(sep)

        l0.addWidget(
            _make_menu_row(
                "settings",
                "#8ab4f8",
                "Özel Kurallar, Listeler ve Beyaz Liste",
                "Tarayıcı sekmesinde aç",
                "external",
                lambda: (self.close(), self.on_open_full_tab()),
            )
        )
        self.stack.addWidget(p0)

        # Page 1: Blocked Requests Log Subpage
        p1 = QWidget()
        l1 = QVBoxLayout(p1)
        l1.setContentsMargins(0, 0, 0, 0)
        l1.setSpacing(8)
        hdr1 = QHBoxLayout()
        back1 = QPushButton()
        back1.setObjectName("PopIconBtn")
        back1.setIcon(svg_icon("chevron_left", "#e8eaed", 16))
        back1.clicked.connect(lambda: self.stack.setCurrentIndex(0))
        t1 = QLabel("Engellenen İstekler")
        t1.setObjectName("PopTitle")
        close1 = QPushButton()
        close1.setObjectName("PopIconBtn")
        close1.setIcon(svg_icon("close", "#9aa0a6", 15))
        close1.clicked.connect(self.close)
        hdr1.addWidget(back1)
        hdr1.addWidget(t1, 1)
        hdr1.addWidget(close1)
        l1.addLayout(hdr1)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setMaximumHeight(320)
        logs_host = QWidget()
        logs_host.setStyleSheet("background: transparent;")
        self.logs_layout = QVBoxLayout(logs_host)
        self.logs_layout.setContentsMargins(0, 0, 0, 0)
        self.logs_layout.setSpacing(4)
        scroll.setWidget(logs_host)
        l1.addWidget(scroll)
        self.stack.addWidget(p1)

        self._refresh_ui()

    def _make_stat_col(self, parent_layout: QHBoxLayout, label: str, color: str) -> QLabel:
        col = QVBoxLayout()
        col.setSpacing(1)
        val_lbl = QLabel("0")
        val_lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)
        val_lbl.setStyleSheet(f"font-size: 16px; font-weight: 700; color: {color};")
        sub_lbl = QLabel(label)
        sub_lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)
        sub_lbl.setStyleSheet("font-size: 11px; color: #9aa0a6;")
        col.addWidget(val_lbl)
        col.addWidget(sub_lbl)
        parent_layout.addLayout(col, 1)
        return val_lbl

    def _refresh_ui(self) -> None:
        host = self.current_tab.current_host() if self.current_tab else ""
        if host:
            self.site_lbl.setText(host)
            whitelisted = self.engine.is_site_whitelisted(host)
            self.site_toggle_btn.setEnabled(True)
            if whitelisted:
                self.site_sub_lbl.setText("Bu sitede koruma duraklatıldı")
                self.site_toggle_btn.setText("Etkinleştir")
                self.site_toggle_btn.setStyleSheet(
                    "background-color: #81c995; color: #202124; border: none; border-radius: 14px; "
                    "padding: 5px 12px; font-size: 12px; font-weight: 600;"
                )
            else:
                self.site_sub_lbl.setText("Reklam & izleyici engelleme aktif")
                self.site_toggle_btn.setText("Duraklat")
                self.site_toggle_btn.setStyleSheet(
                    "background-color: rgba(242, 139, 130, 0.16); color: #f28b82; "
                    "border: 1px solid rgba(242, 139, 130, 0.4); border-radius: 14px; "
                    "padding: 5px 12px; font-size: 12px; font-weight: 600;"
                )
        else:
            self.site_lbl.setText("Dahili Güvenli Sayfa")
            self.site_sub_lbl.setText("Yerel sayfa — filtrelemeye gerek yok")
            self.site_toggle_btn.setEnabled(False)
            self.site_toggle_btn.setText("Aktif")
            self.site_toggle_btn.setStyleSheet(
                "background-color: #303134; color: #9aa0a6; border: none; border-radius: 14px; "
                "padding: 5px 12px; font-size: 12px;"
            )

        self._hdr_ico.setPixmap(
            svg_pixmap(
                "shield" if self.engine.enabled else "shield_off",
                "#81c995" if self.engine.enabled else "#f28b82",
                18,
            )
        )

        tab_count = self.current_tab.blocked_count if self.current_tab else 0
        self.tab_blocked_lbl.setText(f"{tab_count:,}")
        self.total_blocked_lbl.setText(f"{self.engine.total_blocked:,}")
        self.rules_count_lbl.setText(f"{self.engine.total_rules_count:,}")

        while self.logs_layout.count():
            item = self.logs_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()

        items = self.current_tab.blocked_items[:45] if self.current_tab else []
        if not items:
            empty = QLabel("Bu sekmede henüz engellenen istek yok.")
            empty.setObjectName("PopSub")
            empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
            empty.setContentsMargins(10, 24, 10, 24)
            self.logs_layout.addWidget(empty)
        else:
            for entry in items:
                row = QFrame()
                row.setObjectName("PopCard")
                rl = QVBoxLayout(row)
                rl.setContentsMargins(10, 7, 10, 7)
                rl.setSpacing(2)
                top = QLabel(f"{entry.resource_type.upper()}  •  {entry.rule}")
                top.setStyleSheet("color: #f28b82; font-size: 11px; font-weight: 600;")
                url_lbl = QLabel(entry.request_url[:90] + ("…" if len(entry.request_url) > 90 else ""))
                url_lbl.setStyleSheet("color: #bdc1c6; font-size: 11.5px;")
                url_lbl.setWordWrap(True)
                rl.addWidget(top)
                rl.addWidget(url_lbl)
                self.logs_layout.addWidget(row)
        self.logs_layout.addStretch()

    def _on_toggle_site(self) -> None:
        if not self.current_tab:
            return
        host = self.current_tab.current_host()
        if not host:
            return
        if self.engine.is_site_whitelisted(host):
            self.engine.set_site_whitelisted(host, False)
        else:
            self.engine.set_site_whitelisted(host, True)
        self.settings.set("whitelisted_domains", sorted(self.engine.whitelisted_domains))
        self._refresh_ui()
        self.on_state_changed()

    def _on_global_toggled(self, checked: bool) -> None:
        self.engine.enabled = checked
        self.settings.set("adblock_enabled", checked)
        self._refresh_ui()
        self.on_state_changed()

    def _on_cosmetic_toggled(self, checked: bool) -> None:
        self.engine.cosmetic_enabled = checked
        self.settings.set("cosmetic_enabled", checked)
        if self.current_tab:
            self.current_tab.apply_cosmetic_filtering()
        self.on_state_changed()

    def show_below(self, anchor: QWidget) -> None:
        _anchor_below(self, anchor, align_right=True)
        self.show()
