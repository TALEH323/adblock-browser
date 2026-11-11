"""AdBlock Shield Control Panel dialog for per-site controls, logs, custom rules, and list updates."""

from __future__ import annotations

from typing import Callable, Dict, Optional

from PyQt6.QtCore import Qt, QThread, pyqtSignal
from PyQt6.QtWidgets import (
    QCheckBox,
    QDialog,
    QFrame,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QListWidget,
    QMessageBox,
    QPlainTextEdit,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)

from adblock.default_filters import BUILTIN_FILTER_LISTS
from adblock.engine import AdBlockEngine
from .icons import svg_icon
from .storage import SettingsManager
from .web_view import BrowserTab


class FilterUpdateThread(QThread):
    """Background worker for downloading external filter lists."""

    finished_with_results = pyqtSignal(dict)
    failed = pyqtSignal(str)

    def __init__(self, engine: AdBlockEngine, parent=None) -> None:
        super().__init__(parent)
        self.engine = engine

    def run(self) -> None:
        try:
            res = self.engine.update_filter_lists()
            self.finished_with_results.emit(res)
        except Exception as exc:
            self.failed.emit(str(exc))


class AdBlockPanel(QDialog):
    """Interactive AdBlock Shield panel showing stats, toggles, logs, and custom rules."""

    stateChanged = pyqtSignal()

    def __init__(
        self,
        engine: AdBlockEngine,
        settings: SettingsManager,
        current_tab: Optional[BrowserTab],
        reload_current_cb: Callable[[], None],
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.engine = engine
        self.settings = settings
        self.current_tab = current_tab
        self.reload_current_cb = reload_current_cb
        self._update_thread: Optional[FilterUpdateThread] = None

        self.setWindowTitle("AdBlock Kalkanı ve Kontrol Paneli")
        self.setWindowIcon(svg_icon("shield", "#81c995", 18))
        self.resize(680, 560)
        self.setStyleSheet(
            """
            QDialog { background-color: #202124; color: #e8eaed; }
            QLabel { color: #e8eaed; }
            QFrame#Card {
                background-color: #292a2d;
                border: 1px solid #3c4043;
                border-radius: 12px;
                padding: 10px;
            }
            QPushButton {
                background-color: #303134;
                color: #e8eaed;
                border: 1px solid #3c4043;
                border-radius: 16px;
                padding: 7px 14px;
                font-weight: 600;
            }
            QPushButton:hover { background-color: #3c4043; }
            QPushButton#PrimaryBtn {
                background-color: #81c995;
                border-color: #81c995;
                color: #202124;
            }
            QPushButton#PrimaryBtn:hover { background-color: #9be0ae; }
            QPushButton#DangerBtn {
                background-color: rgba(242, 139, 130, 0.16);
                border-color: #f28b82;
                color: #f28b82;
            }
            QTabWidget::pane {
                border: 1px solid #3c4043;
                background: #292a2d;
                border-radius: 8px;
            }
            QTabBar::tab {
                background: #202124;
                color: #9aa0a6;
                padding: 8px 14px;
                border-top-left-radius: 8px;
                border-top-right-radius: 8px;
                margin-right: 4px;
            }
            QTabBar::tab:selected {
                background: #292a2d;
                color: #e8eaed;
                border: 1px solid #3c4043;
                border-bottom: none;
            }
            QTableWidget, QPlainTextEdit, QListWidget {
                background-color: #202124;
                color: #e8eaed;
                border: 1px solid #3c4043;
                border-radius: 8px;
                gridline-color: #35363a;
            }
            QHeaderView::section {
                background-color: #292a2d;
                color: #9aa0a6;
                border: none;
                border-bottom: 1px solid #3c4043;
                padding: 6px;
            }
            QCheckBox { color: #e8eaed; spacing: 8px; font-size: 13px; }
            """
        )

        self._build_ui()
        self._refresh_ui()

    def _build_ui(self) -> None:
        root = QVBoxLayout(self)
        root.setSpacing(12)

        # Header Card: Site status + per-site toggle
        header_card = QFrame()
        header_card.setObjectName("Card")
        header_layout = QHBoxLayout(header_card)

        site_info = QVBoxLayout()
        self.site_label = QLabel("Geçerli Site: -")
        self.site_label.setStyleSheet("font-size: 15px; font-weight: 700;")
        self.status_sub = QLabel("")
        self.status_sub.setStyleSheet("font-size: 12px; color: #8b949e;")
        site_info.addWidget(self.site_label)
        site_info.addWidget(self.status_sub)

        self.site_toggle_btn = QPushButton("Bu Sitede Devre Dışı Bırak")
        self.site_toggle_btn.clicked.connect(self._on_toggle_site)

        header_layout.addLayout(site_info, 1)
        header_layout.addWidget(self.site_toggle_btn)
        root.addWidget(header_card)

        # Stats Row
        stats_row = QHBoxLayout()
        self.tab_blocked_lbl = self._make_stat_card(stats_row, "0", "Bu Sekmede Engellenen", "#56d364")
        self.total_blocked_lbl = self._make_stat_card(stats_row, "0", "Toplam Engellenen", "#58a6ff")
        self.rules_count_lbl = self._make_stat_card(stats_row, "0", "Aktif Kural Sayısı", "#d2a8ff")
        root.addLayout(stats_row)

        # Global toggles
        toggles_row = QHBoxLayout()
        self.global_cb = QCheckBox("Genel Reklam Engelleme Aktif")
        self.global_cb.setChecked(self.engine.enabled)
        self.global_cb.toggled.connect(self._on_global_toggled)

        self.cosmetic_cb = QCheckBox("Kozmetik Filtreleme (Reklam kutularını CSS ile gizle)")
        self.cosmetic_cb.setChecked(self.engine.cosmetic_enabled)
        self.cosmetic_cb.toggled.connect(self._on_cosmetic_toggled)

        toggles_row.addWidget(self.global_cb)
        toggles_row.addWidget(self.cosmetic_cb)
        toggles_row.addStretch()
        root.addLayout(toggles_row)

        # Tabs
        self.tabs = QTabWidget()
        self.tabs.addTab(self._build_blocked_tab(), "Engellenen İstekler")
        self.tabs.addTab(self._build_custom_rules_tab(), "Özel Kurallar")
        self.tabs.addTab(self._build_lists_tab(), "Filtre Listeleri")
        self.tabs.addTab(self._build_whitelist_tab(), "Beyaz Liste")
        root.addWidget(self.tabs, 1)

        # Footer
        footer = QHBoxLayout()
        reload_btn = QPushButton("Sayfayı Yenile")
        reload_btn.clicked.connect(self.reload_current_cb)
        close_btn = QPushButton("Kapat")
        close_btn.clicked.connect(self.accept)
        footer.addWidget(reload_btn)
        footer.addStretch()
        footer.addWidget(close_btn)
        root.addLayout(footer)

    def _make_stat_card(self, parent_layout: QHBoxLayout, val: str, title: str, color: str) -> QLabel:
        card = QFrame()
        card.setObjectName("Card")
        v = QVBoxLayout(card)
        v.setSpacing(2)
        val_lbl = QLabel(val)
        val_lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)
        val_lbl.setStyleSheet(f"font-size: 20px; font-weight: 700; color: {color};")
        title_lbl = QLabel(title)
        title_lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title_lbl.setStyleSheet("font-size: 11px; color: #8b949e;")
        v.addWidget(val_lbl)
        v.addWidget(title_lbl)
        parent_layout.addWidget(card)
        return val_lbl

    def _build_blocked_tab(self) -> QWidget:
        w = QWidget()
        layout = QVBoxLayout(w)
        self.blocked_table = QTableWidget(0, 3)
        self.blocked_table.setHorizontalHeaderLabels(["Tür", "Engellenen URL", "Eşleşen Kural"])
        header = self.blocked_table.horizontalHeader()
        header.setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        header.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        self.blocked_table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.blocked_table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        layout.addWidget(self.blocked_table)
        return w

    def _build_custom_rules_tab(self) -> QWidget:
        w = QWidget()
        layout = QVBoxLayout(w)
        hint = QLabel(
            "Her satıra bir EasyList/ABP kuralı yazabilirsiniz (ör: ||reklam-sitesi.com^ , @@||izinli.com^ , example.com##.reklam-kutusu):"
        )
        hint.setWordWrap(True)
        hint.setStyleSheet("color: #8b949e; font-size: 12px;")
        self.custom_editor = QPlainTextEdit()
        self.custom_editor.setPlainText(self.engine.custom_rules_text)
        save_btn = QPushButton("Özel Kuralları Kaydet ve Uygula")
        save_btn.setObjectName("PrimaryBtn")
        save_btn.clicked.connect(self._on_save_custom_rules)

        layout.addWidget(hint)
        layout.addWidget(self.custom_editor, 1)
        layout.addWidget(save_btn, alignment=Qt.AlignmentFlag.AlignRight)
        return w

    def _build_lists_tab(self) -> QWidget:
        w = QWidget()
        layout = QVBoxLayout(w)
        desc = QLabel(
            "Tarayıcı yerleşik reklam/izleyici kurallarıyla gelir. İsterseniz açık kaynaklı filtre listelerini internetten indirip güncelleyebilirsiniz:"
        )
        desc.setWordWrap(True)
        desc.setStyleSheet("color: #8b949e; font-size: 12px;")
        layout.addWidget(desc)

        self.lists_widget = QListWidget()
        for item in BUILTIN_FILTER_LISTS:
            cached_path = self.engine.filter_dir / f"{item['id']}.txt" if self.engine.filter_dir else None
            status = "İndirildi" if (cached_path and cached_path.exists()) else "Henüz indirilmedi"
            self.lists_widget.addItem(f"• {item['name']}  [{status}]")
        layout.addWidget(self.lists_widget, 1)

        row = QHBoxLayout()
        self.update_status_lbl = QLabel("")
        self.update_status_lbl.setStyleSheet("color: #8b949e; font-size: 12px;")
        self.update_lists_btn = QPushButton("Filtre Listelerini İnternetten Güncelle")
        self.update_lists_btn.setObjectName("PrimaryBtn")
        self.update_lists_btn.clicked.connect(self._on_update_lists)
        row.addWidget(self.update_status_lbl, 1)
        row.addWidget(self.update_lists_btn)
        layout.addLayout(row)
        return w

    def _build_whitelist_tab(self) -> QWidget:
        w = QWidget()
        layout = QVBoxLayout(w)
        self.whitelist_widget = QListWidget()
        remove_btn = QPushButton("Seçili Siteyi Beyaz Listeden Çıkar")
        remove_btn.clicked.connect(self._on_remove_whitelist)
        layout.addWidget(self.whitelist_widget, 1)
        layout.addWidget(remove_btn, alignment=Qt.AlignmentFlag.AlignRight)
        return w

    def _refresh_ui(self) -> None:
        host = self.current_tab.current_host() if self.current_tab else ""
        whitelisted = self.engine.is_site_whitelisted(host) if host else False

        if host:
            self.site_label.setText(f"Site: {host}")
            self.site_toggle_btn.setEnabled(True)
            if not self.engine.enabled:
                self.status_sub.setText("Genel reklam engelleme şu an kapalı.")
                self.site_toggle_btn.setText("Bu Sitede Devre Dışı Bırak")
                self.site_toggle_btn.setObjectName("")
            elif whitelisted:
                self.status_sub.setText("Bu site beyaz listede — reklamlar engellenmiyor.")
                self.site_toggle_btn.setText("Bu Sitede Korumayı Aç")
                self.site_toggle_btn.setObjectName("PrimaryBtn")
            else:
                self.status_sub.setText("Reklam ve izleyici engelleme bu sitede aktif.")
                self.site_toggle_btn.setText("Bu Sitede Devre Dışı Bırak")
                self.site_toggle_btn.setObjectName("DangerBtn")
            self.site_toggle_btn.style().unpolish(self.site_toggle_btn)
            self.site_toggle_btn.style().polish(self.site_toggle_btn)
        else:
            self.site_label.setText("Yeni Sekme / Dahili Sayfa")
            self.status_sub.setText("Site bazlı kontrol için bir web sitesini ziyaret edin.")
            self.site_toggle_btn.setEnabled(False)

        tab_blocked = self.current_tab.blocked_count if self.current_tab else 0
        self.tab_blocked_lbl.setText(f"{tab_blocked:,}")
        self.total_blocked_lbl.setText(f"{self.engine.total_blocked:,}")
        self.rules_count_lbl.setText(f"{self.engine.total_rules_count:,}")

        # Populate blocked table
        items = self.current_tab.blocked_items if self.current_tab else []
        self.blocked_table.setRowCount(len(items))
        for row_idx, entry in enumerate(items):
            self.blocked_table.setItem(row_idx, 0, QTableWidgetItem(entry.resource_type))
            self.blocked_table.setItem(row_idx, 1, QTableWidgetItem(entry.request_url))
            self.blocked_table.setItem(row_idx, 2, QTableWidgetItem(entry.rule))

        # Populate whitelist
        self.whitelist_widget.clear()
        for dom in sorted(self.engine.whitelisted_domains):
            self.whitelist_widget.addItem(dom)

    def _on_toggle_site(self) -> None:
        if not self.current_tab:
            return
        host = self.current_tab.current_host()
        if not host:
            return
        self.engine.toggle_site_whitelist(host)
        self.settings.set("whitelisted_domains", sorted(self.engine.whitelisted_domains))
        self.current_tab.apply_cosmetic_filtering()
        self.reload_current_cb()
        self._refresh_ui()
        self.stateChanged.emit()

    def _on_global_toggled(self, checked: bool) -> None:
        self.engine.enabled = checked
        self.settings.set("adblock_enabled", checked)
        if self.current_tab:
            self.current_tab.apply_cosmetic_filtering()
        self._refresh_ui()
        self.stateChanged.emit()

    def _on_cosmetic_toggled(self, checked: bool) -> None:
        self.engine.cosmetic_enabled = checked
        self.settings.set("cosmetic_enabled", checked)
        if self.current_tab:
            self.current_tab.apply_cosmetic_filtering()
        self._refresh_ui()
        self.stateChanged.emit()

    def _on_save_custom_rules(self) -> None:
        count = self.engine.set_custom_rules(self.custom_editor.toPlainText())
        if self.current_tab:
            self.current_tab.apply_cosmetic_filtering()
        self._refresh_ui()
        self.stateChanged.emit()
        QMessageBox.information(
            self,
            "Kurallar Kaydedildi",
            f"Özel kurallar uygulandı. Toplam aktif kural sayısı: {count:,}",
        )

    def _on_remove_whitelist(self) -> None:
        item = self.whitelist_widget.currentItem()
        if not item:
            return
        dom = item.text().strip()
        self.engine.set_site_whitelisted(dom, False)
        self.settings.set("whitelisted_domains", sorted(self.engine.whitelisted_domains))
        self._refresh_ui()
        self.stateChanged.emit()

    def _on_update_lists(self) -> None:
        self.update_lists_btn.setEnabled(False)
        self.update_status_lbl.setText("Listeler indiriliyor...")
        self._update_thread = FilterUpdateThread(self.engine, self)
        self._update_thread.finished_with_results.connect(self._on_lists_updated)
        self._update_thread.failed.connect(self._on_lists_failed)
        self._update_thread.start()

    def _on_lists_updated(self, results: Dict[str, str]) -> None:
        self.update_lists_btn.setEnabled(True)
        summary = " | ".join(f"{k}: {v}" for k, v in results.items())
        self.update_status_lbl.setText(summary)
        self.lists_widget.clear()
        for item in BUILTIN_FILTER_LISTS:
            cached_path = self.engine.filter_dir / f"{item['id']}.txt" if self.engine.filter_dir else None
            status = "İndirildi" if (cached_path and cached_path.exists()) else "Henüz indirilmedi"
            self.lists_widget.addItem(f"• {item['name']}  [{status}]")
        self._refresh_ui()
        self.stateChanged.emit()

    def _on_lists_failed(self, err: str) -> None:
        self.update_lists_btn.setEnabled(True)
        self.update_status_lbl.setText(f"Güncelleme başarısız: {err}")
