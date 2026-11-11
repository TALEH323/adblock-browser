"""Dialogs for Bookmarks, History, and Browser Settings."""

from __future__ import annotations

from typing import Callable

from PyQt6.QtCore import Qt
from PyQt6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDialog,
    QFormLayout,
    QHBoxLayout,
    QHeaderView,
    QLineEdit,
    QMessageBox,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
)

from adblock.dns_guard import DOH_PROVIDERS
from .icons import svg_icon
from .storage import BookmarksManager, HistoryManager, SEARCH_ENGINES, SettingsManager

_DIALOG_QSS = """
QDialog { background-color: #202124; color: #e8eaed; }
QLabel { color: #e8eaed; }
QLineEdit, QComboBox {
    background-color: #292a2d;
    color: #e8eaed;
    border: 1px solid #3c4043;
    border-radius: 8px;
    padding: 6px 12px;
}
QLineEdit:focus, QComboBox:focus { border-color: #8ab4f8; }
QTableWidget {
    background-color: #292a2d;
    color: #e8eaed;
    border: 1px solid #3c4043;
    border-radius: 8px;
    gridline-color: #35363a;
}
QHeaderView::section {
    background-color: #202124;
    color: #9aa0a6;
    border: none;
    border-bottom: 1px solid #3c4043;
    padding: 7px;
}
QPushButton {
    background-color: #303134;
    color: #e8eaed;
    border: 1px solid #3c4043;
    border-radius: 16px;
    padding: 6px 14px;
    font-weight: 600;
}
QPushButton:hover { background-color: #3c4043; }
QPushButton#DangerBtn { background-color: rgba(242, 139, 130, 0.16); border-color: #f28b82; color: #f28b82; }
QPushButton#PrimaryBtn { background-color: #8ab4f8; border-color: #8ab4f8; color: #202124; }
QCheckBox { color: #e8eaed; }
"""


class BookmarksDialog(QDialog):
    """Bookmarks manager dialog."""

    def __init__(
        self,
        bookmarks: BookmarksManager,
        open_url_cb: Callable[[str], None],
        on_changed_cb: Callable[[], None],
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.bookmarks = bookmarks
        self.open_url_cb = open_url_cb
        self.on_changed_cb = on_changed_cb

        self.setWindowTitle("Yer İmleri")
        self.setWindowIcon(svg_icon("bookmarks", "#8ab4f8", 18))
        self.resize(640, 440)
        self.setStyleSheet(_DIALOG_QSS)

        layout = QVBoxLayout(self)

        top = QHBoxLayout()
        self.search_input = QLineEdit()
        self.search_input.setPlaceholderText("Yer imlerinde ara...")
        self.search_input.textChanged.connect(self._populate)
        top.addWidget(self.search_input)
        layout.addLayout(top)

        self.table = QTableWidget(0, 3)
        self.table.setHorizontalHeaderLabels(["Başlık", "URL", "Eklenme"])
        hdr = self.table.horizontalHeader()
        hdr.setSectionResizeMode(0, QHeaderView.ResizeMode.Interactive)
        hdr.setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        hdr.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        self.table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        self.table.cellDoubleClicked.connect(self._open_selected)
        layout.addWidget(self.table, 1)

        btns = QHBoxLayout()
        open_btn = QPushButton("Yeni Sekmede Aç")
        open_btn.setObjectName("PrimaryBtn")
        open_btn.clicked.connect(self._open_selected)

        del_btn = QPushButton("Seçiliyi Sil")
        del_btn.setObjectName("DangerBtn")
        del_btn.clicked.connect(self._delete_selected)

        close_btn = QPushButton("Kapat")
        close_btn.clicked.connect(self.accept)

        btns.addWidget(open_btn)
        btns.addWidget(del_btn)
        btns.addStretch()
        btns.addWidget(close_btn)
        layout.addLayout(btns)

        self._populate()

    def _populate(self) -> None:
        q = self.search_input.text().strip().lower()
        items = [
            b
            for b in self.bookmarks.items
            if not q or q in b.title.lower() or q in b.url.lower()
        ]
        self.table.setRowCount(len(items))
        for i, b in enumerate(items):
            self.table.setItem(i, 0, QTableWidgetItem(b.title))
            self.table.setItem(i, 1, QTableWidgetItem(b.url))
            self.table.setItem(i, 2, QTableWidgetItem(b.created_at))

    def _open_selected(self) -> None:
        row = self.table.currentRow()
        if row < 0:
            return
        url_item = self.table.item(row, 1)
        if url_item:
            self.open_url_cb(url_item.text())
            self.accept()

    def _delete_selected(self) -> None:
        row = self.table.currentRow()
        if row < 0:
            return
        url_item = self.table.item(row, 1)
        if url_item:
            self.bookmarks.remove(url_item.text())
            self.on_changed_cb()
            self._populate()


class HistoryDialog(QDialog):
    """Searchable browsing history viewer dialog."""

    def __init__(
        self,
        history: HistoryManager,
        open_url_cb: Callable[[str], None],
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.history = history
        self.open_url_cb = open_url_cb

        self.setWindowTitle("Tarama Geçmişi")
        self.setWindowIcon(svg_icon("history", "#8ab4f8", 18))
        self.resize(700, 480)
        self.setStyleSheet(_DIALOG_QSS)

        layout = QVBoxLayout(self)

        self.search_input = QLineEdit()
        self.search_input.setPlaceholderText("Geçmişte ara (başlık veya URL)...")
        self.search_input.textChanged.connect(self._populate)
        layout.addWidget(self.search_input)

        self.table = QTableWidget(0, 3)
        self.table.setHorizontalHeaderLabels(["Başlık", "URL", "Ziyaret Zamanı"])
        hdr = self.table.horizontalHeader()
        hdr.setSectionResizeMode(0, QHeaderView.ResizeMode.Interactive)
        hdr.setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        hdr.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        self.table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        self.table.cellDoubleClicked.connect(self._open_selected)
        layout.addWidget(self.table, 1)

        btns = QHBoxLayout()
        open_btn = QPushButton("Yeni Sekmede Aç")
        open_btn.setObjectName("PrimaryBtn")
        open_btn.clicked.connect(self._open_selected)

        clear_btn = QPushButton("Tüm Geçmişi Temizle")
        clear_btn.setObjectName("DangerBtn")
        clear_btn.clicked.connect(self._clear_all)

        close_btn = QPushButton("Kapat")
        close_btn.clicked.connect(self.accept)

        btns.addWidget(open_btn)
        btns.addWidget(clear_btn)
        btns.addStretch()
        btns.addWidget(close_btn)
        layout.addLayout(btns)

        self._populate()

    def _populate(self) -> None:
        items = self.history.search(self.search_input.text())
        self.table.setRowCount(len(items))
        for i, h in enumerate(items):
            self.table.setItem(i, 0, QTableWidgetItem(h.title))
            self.table.setItem(i, 1, QTableWidgetItem(h.url))
            self.table.setItem(i, 2, QTableWidgetItem(h.visited_at))

    def _open_selected(self) -> None:
        row = self.table.currentRow()
        if row < 0:
            return
        url_item = self.table.item(row, 1)
        if url_item:
            self.open_url_cb(url_item.text())
            self.accept()

    def _clear_all(self) -> None:
        reply = QMessageBox.question(
            self,
            "Geçmişi Temizle",
            "Tüm tarama geçmişini silmek istediğinize emin misiniz?",
        )
        if reply == QMessageBox.StandardButton.Yes:
            self.history.clear()
            self._populate()


class SettingsDialog(QDialog):
    """Browser general settings dialog."""

    def __init__(self, settings: SettingsManager, parent=None) -> None:
        super().__init__(parent)
        self.settings = settings
        self.setWindowTitle("Tarayıcı Ayarları")
        self.setWindowIcon(svg_icon("settings", "#8ab4f8", 18))
        self.resize(480, 310)
        self.setStyleSheet(_DIALOG_QSS)

        layout = QVBoxLayout(self)
        form = QFormLayout()

        self.engine_combo = QComboBox()
        current_engine = self.settings.get("search_engine", "google")
        selected_idx = 0
        for idx, (key, (name, _)) in enumerate(SEARCH_ENGINES.items()):
            self.engine_combo.addItem(name, key)
            if key == current_engine:
                selected_idx = idx
        self.engine_combo.setCurrentIndex(selected_idx)

        self.doh_combo = QComboBox()
        current_doh = self.settings.get("doh_provider", "cloudflare_security")
        doh_idx = 0
        for idx, (key, (name, _)) in enumerate(DOH_PROVIDERS.items()):
            self.doh_combo.addItem(name, key)
            if key == current_doh:
                doh_idx = idx
        self.doh_combo.setCurrentIndex(doh_idx)

        self.homepage_edit = QLineEdit(self.settings.get("homepage", "adblock://home"))
        self.bm_bar_cb = QCheckBox("Yer İmleri Çubuğunu Göster")
        self.bm_bar_cb.setChecked(bool(self.settings.get("show_bookmarks_bar", True)))
        self.pw_mgr_cb = QCheckBox("Şifre Kaydetme ve Otomatik Doldurma Aktif")
        self.pw_mgr_cb.setChecked(bool(self.settings.get("password_manager_enabled", True)))
        self.https_only_cb = QCheckBox("HTTPS-Only Modu (HTTP siteleri otomatik HTTPS'e yükselt)")
        self.https_only_cb.setChecked(bool(self.settings.get("https_only_enabled", True)))

        form.addRow("Varsayılan Arama Motoru:", self.engine_combo)
        form.addRow("Güvenli DNS (DoH):", self.doh_combo)
        form.addRow("Ana Sayfa URL:", self.homepage_edit)
        form.addRow("", self.https_only_cb)
        form.addRow("", self.bm_bar_cb)
        form.addRow("", self.pw_mgr_cb)
        layout.addLayout(form)

        btns = QHBoxLayout()
        save_btn = QPushButton("Kaydet")
        save_btn.setObjectName("PrimaryBtn")
        save_btn.clicked.connect(self._save)
        cancel_btn = QPushButton("İptal")
        cancel_btn.clicked.connect(self.reject)
        btns.addStretch()
        btns.addWidget(cancel_btn)
        btns.addWidget(save_btn)
        layout.addLayout(btns)

    def _save(self) -> None:
        self.settings.set("search_engine", self.engine_combo.currentData())
        self.settings.set("doh_provider", self.doh_combo.currentData())
        hp = self.homepage_edit.text().strip() or "adblock://home"
        self.settings.set("homepage", hp)
        self.settings.set("https_only_enabled", self.https_only_cb.isChecked())
        self.settings.set("show_bookmarks_bar", self.bm_bar_cb.isChecked())
        self.settings.set("password_manager_enabled", self.pw_mgr_cb.isChecked())
        self.accept()
