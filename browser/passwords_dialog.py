"""Saved Passwords manager dialog."""

from __future__ import annotations

from typing import Callable, List, Optional

from PyQt6.QtWidgets import (
    QApplication,
    QDialog,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QListWidget,
    QMessageBox,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)

from .icons import svg_icon, svg_pixmap
from .library_dialogs import _DIALOG_QSS
from .passwords import PasswordEntry, PasswordManager


class PasswordsDialog(QDialog):
    """Dialog for inspecting, copying, filling, and deleting saved website passwords."""

    def __init__(
        self,
        pw_manager: PasswordManager,
        fill_current_tab_cb: Optional[Callable[[str, str], None]] = None,
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.pw_manager = pw_manager
        self.fill_current_tab_cb = fill_current_tab_cb
        self._revealed = False
        self._filtered_entries: List[PasswordEntry] = []

        self.setWindowTitle("Kayıtlı Şifreler")
        self.setWindowIcon(svg_icon("key", "#8ab4f8", 18))
        self.resize(740, 480)
        self.setStyleSheet(_DIALOG_QSS)

        layout = QVBoxLayout(self)

        tabs = QTabWidget()
        tabs.setStyleSheet(
            """
            QTabWidget::pane { border: 1px solid #3c4043; background: #292a2d; border-radius: 8px; }
            QTabBar::tab { background: #202124; color: #9aa0a6; padding: 7px 14px; border-top-left-radius: 8px; border-top-right-radius: 8px; margin-right: 4px; }
            QTabBar::tab:selected { background: #292a2d; color: #e8eaed; border: 1px solid #3c4043; border-bottom: none; }
            """
        )

        # Tab 1: Saved credentials
        cred_tab = QWidget()
        cred_layout = QVBoxLayout(cred_tab)

        top = QHBoxLayout()
        self.search_input = QLineEdit()
        self.search_input.setPlaceholderText("Site veya kullanıcı adı ara...")
        self.search_input.textChanged.connect(self._populate_table)

        self.reveal_btn = QPushButton("Şifreleri Göster")
        self.reveal_btn.setIcon(svg_icon("eye", "#e8eaed", 15))
        self.reveal_btn.clicked.connect(self._toggle_reveal)

        top.addWidget(self.search_input, 1)
        top.addWidget(self.reveal_btn)
        cred_layout.addLayout(top)

        self.table = QTableWidget(0, 4)
        self.table.setHorizontalHeaderLabels(["Site (Origin)", "Kullanıcı Adı", "Şifre", "Güncelleme"])
        hdr = self.table.horizontalHeader()
        hdr.setSectionResizeMode(0, QHeaderView.ResizeMode.Stretch)
        hdr.setSectionResizeMode(1, QHeaderView.ResizeMode.Interactive)
        hdr.setSectionResizeMode(2, QHeaderView.ResizeMode.Interactive)
        hdr.setSectionResizeMode(3, QHeaderView.ResizeMode.ResizeToContents)
        self.table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        cred_layout.addWidget(self.table, 1)

        btns = QHBoxLayout()
        copy_user_btn = QPushButton("Kullanıcıyı Kopyala")
        copy_user_btn.clicked.connect(self._copy_username)

        copy_pw_btn = QPushButton("Şifreyi Kopyala")
        copy_pw_btn.clicked.connect(self._copy_password)

        fill_btn = QPushButton("Aktif Sekmede Doldur")
        fill_btn.setObjectName("PrimaryBtn")
        fill_btn.clicked.connect(self._fill_in_tab)

        del_btn = QPushButton("Seçiliyi Sil")
        del_btn.setObjectName("DangerBtn")
        del_btn.clicked.connect(self._delete_selected)

        clear_btn = QPushButton("Tümünü Sil")
        clear_btn.clicked.connect(self._clear_all)

        btns.addWidget(copy_user_btn)
        btns.addWidget(copy_pw_btn)
        if self.fill_current_tab_cb is not None:
            btns.addWidget(fill_btn)
        btns.addStretch()
        btns.addWidget(del_btn)
        btns.addWidget(clear_btn)
        cred_layout.addLayout(btns)

        # Tab 2: Never-save sites
        never_tab = QWidget()
        never_layout = QVBoxLayout(never_tab)
        info = QLabel("Şifre kaydetmeyi reddettiğiniz siteler (isterseniz listeden çıkarabilirsiniz):")
        info.setStyleSheet("color: #8b949e; font-size: 12px;")
        self.never_list = QListWidget()
        self.never_list.setStyleSheet("background-color: #0d1117; color: #e6edf3; border: 1px solid #30363d; border-radius: 6px;")
        remove_never_btn = QPushButton("Seçili Siteyi Kaldır")
        remove_never_btn.clicked.connect(self._remove_never_save)
        never_layout.addWidget(info)
        never_layout.addWidget(self.never_list, 1)
        never_layout.addWidget(remove_never_btn)

        tabs.addTab(cred_tab, "Kayıtlı Şifreler")
        tabs.addTab(never_tab, "Asla Kaydedilmeyen Siteler")
        layout.addWidget(tabs, 1)

        footer = QHBoxLayout()
        lock_lbl = QLabel()
        lock_lbl.setPixmap(svg_pixmap("lock", "#81c995", 14))
        enc_note = QLabel("Şifreler yerel cihazınızda Fernet (AES) ile şifrelenerek saklanır.")
        enc_note.setStyleSheet("color: #9aa0a6; font-size: 11px;")
        close_btn = QPushButton("Kapat")
        close_btn.clicked.connect(self.accept)
        footer.addWidget(lock_lbl)
        footer.addWidget(enc_note)
        footer.addStretch()
        footer.addWidget(close_btn)
        layout.addLayout(footer)

        self._populate_table()
        self._populate_never_list()

    def _populate_table(self) -> None:
        q = self.search_input.text().strip().lower()
        self._filtered_entries = [
            e
            for e in self.pw_manager.entries
            if not q or q in e.origin.lower() or q in e.username.lower()
        ]
        self.table.setRowCount(len(self._filtered_entries))
        for i, e in enumerate(self._filtered_entries):
            self.table.setItem(i, 0, QTableWidgetItem(e.origin))
            self.table.setItem(i, 1, QTableWidgetItem(e.username or "-"))
            masked = e.password if self._revealed else "•" * min(12, max(6, len(e.password)))
            self.table.setItem(i, 2, QTableWidgetItem(masked))
            self.table.setItem(i, 3, QTableWidgetItem(e.updated_at))

    def _populate_never_list(self) -> None:
        self.never_list.clear()
        for h in sorted(self.pw_manager.never_save_hosts):
            self.never_list.addItem(h)

    def _selected_entry(self) -> Optional[PasswordEntry]:
        row = self.table.currentRow()
        if 0 <= row < len(self._filtered_entries):
            return self._filtered_entries[row]
        return None

    def _toggle_reveal(self) -> None:
        self._revealed = not self._revealed
        self.reveal_btn.setText("Şifreleri Gizle" if self._revealed else "Şifreleri Göster")
        self.reveal_btn.setIcon(svg_icon("eye_off" if self._revealed else "eye", "#e8eaed", 15))
        self._populate_table()

    def _copy_username(self) -> None:
        entry = self._selected_entry()
        if entry and entry.username:
            QApplication.clipboard().setText(entry.username)

    def _copy_password(self) -> None:
        entry = self._selected_entry()
        if entry:
            QApplication.clipboard().setText(entry.password)

    def _fill_in_tab(self) -> None:
        entry = self._selected_entry()
        if entry and self.fill_current_tab_cb:
            self.fill_current_tab_cb(entry.username, entry.password)
            self.accept()

    def _delete_selected(self) -> None:
        entry = self._selected_entry()
        if not entry:
            return
        self.pw_manager.delete_credential(entry.origin, entry.username)
        self._populate_table()

    def _clear_all(self) -> None:
        if not self.pw_manager.entries:
            return
        reply = QMessageBox.question(
            self,
            "Tüm Şifreleri Sil",
            "Kayıtlı tüm şifreleri kalıcı olarak silmek istediğinize emin misiniz?",
        )
        if reply == QMessageBox.StandardButton.Yes:
            self.pw_manager.clear_all()
            self._populate_table()

    def _remove_never_save(self) -> None:
        item = self.never_list.currentItem()
        if item:
            self.pw_manager.remove_never_save(item.text())
            self._populate_never_list()
