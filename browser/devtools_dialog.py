"""Find-in-Page bar and Chromium DevTools inspector window with SVG icons."""

from __future__ import annotations

from typing import Callable, Optional

from PyQt6.QtCore import QSize
from PyQt6.QtWebEngineCore import QWebEnginePage
from PyQt6.QtWebEngineWidgets import QWebEngineView
from PyQt6.QtWidgets import (
    QDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from .icons import svg_icon, svg_pixmap


class FindBar(QWidget):
    """Chrome-style inline Find-in-Page bar (Cmd/Ctrl+F)."""

    def __init__(self, get_tab_fn: Callable[[], Optional[QWebEngineView]], parent=None) -> None:
        super().__init__(parent)
        self._get_tab = get_tab_fn
        self.setStyleSheet(
            """
            QWidget { background-color: #292a2d; border-bottom: 1px solid #3c4043; color: #e8eaed; }
            QLineEdit {
                background-color: #202124;
                color: #e8eaed;
                border: 1px solid #3c4043;
                border-radius: 14px;
                padding: 4px 12px;
                min-width: 240px;
            }
            QLineEdit:focus { border-color: #8ab4f8; }
            QPushButton {
                background-color: transparent;
                color: #e8eaed;
                border: 1px solid transparent;
                border-radius: 14px;
                padding: 4px 8px;
                font-size: 12px;
            }
            QPushButton:hover { background-color: #35363a; }
            QPushButton:checked { background-color: rgba(138, 180, 248, 0.2); border-color: #8ab4f8; color: #8ab4f8; }
            """
        )

        layout = QHBoxLayout(self)
        layout.setContentsMargins(12, 5, 12, 5)
        layout.setSpacing(6)

        icon_lbl = QLabel()
        icon_lbl.setStyleSheet("border: none;")
        icon_lbl.setPixmap(svg_pixmap("search", "#9aa0a6", 15))

        lbl = QLabel("Sayfada Bul:")
        lbl.setStyleSheet("border: none; font-size: 12px; color: #9aa0a6;")

        self.input = QLineEdit()
        self.input.setPlaceholderText("Aranacak metni yazın...")
        self.input.textChanged.connect(lambda: self._run_find(backward=False))
        self.input.returnPressed.connect(lambda: self._run_find(backward=False))

        self.case_btn = QPushButton("Aa")
        self.case_btn.setCheckable(True)
        self.case_btn.setToolTip("Büyük/Küçük Harf Duyarlı")
        self.case_btn.toggled.connect(lambda: self._run_find(backward=False))

        prev_btn = QPushButton()
        prev_btn.setIcon(svg_icon("arrow_up", "#e8eaed", 15))
        prev_btn.setIconSize(QSize(15, 15))
        prev_btn.setToolTip("Önceki Eşleşme")
        prev_btn.clicked.connect(lambda: self._run_find(backward=True))

        next_btn = QPushButton()
        next_btn.setIcon(svg_icon("arrow_down", "#e8eaed", 15))
        next_btn.setIconSize(QSize(15, 15))
        next_btn.setToolTip("Sonraki Eşleşme")
        next_btn.clicked.connect(lambda: self._run_find(backward=False))

        self.status_lbl = QLabel("")
        self.status_lbl.setStyleSheet("border: none; font-size: 12px; color: #9aa0a6;")

        close_btn = QPushButton()
        close_btn.setIcon(svg_icon("close", "#9aa0a6", 15))
        close_btn.setIconSize(QSize(15, 15))
        close_btn.setToolTip("Kapat (Esc)")
        close_btn.clicked.connect(self.close_bar)

        layout.addWidget(icon_lbl)
        layout.addWidget(lbl)
        layout.addWidget(self.input)
        layout.addWidget(self.case_btn)
        layout.addWidget(prev_btn)
        layout.addWidget(next_btn)
        layout.addWidget(self.status_lbl)
        layout.addStretch()
        layout.addWidget(close_btn)
        self.hide()

    def open_and_focus(self) -> None:
        self.show()
        self.input.setFocus()
        self.input.selectAll()

    def close_bar(self) -> None:
        tab = self._get_tab()
        if tab:
            tab.findText("")
        self.status_lbl.setText("")
        self.hide()

    def _run_find(self, backward: bool = False) -> None:
        tab = self._get_tab()
        if not tab:
            return
        query = self.input.text()
        if not query:
            tab.findText("")
            self.status_lbl.setText("")
            return

        flags = QWebEnginePage.FindFlag(0)
        if backward:
            flags |= QWebEnginePage.FindFlag.FindBackward
        if self.case_btn.isChecked():
            flags |= QWebEnginePage.FindFlag.FindCaseSensitively

        def on_result(found) -> None:
            if isinstance(found, bool):
                self.status_lbl.setText("Bulundu" if found else "Eşleşme yok")
            elif hasattr(found, "numberOfMatches"):
                total = found.numberOfMatches()
                active = found.activeMatch()
                self.status_lbl.setText(f"{active}/{total}" if total > 0 else "0/0")

        tab.findText(query, flags, on_result)


class DevToolsWindow(QDialog):
    """Non-modal Chromium DevTools inspector window for a BrowserTab."""

    def __init__(self, target_page: QWebEnginePage, parent=None) -> None:
        super().__init__(parent)
        self.setWindowTitle("Geliştirici Araçları (DevTools)")
        self.setWindowIcon(svg_icon("devtools", "#8ab4f8", 18))
        self.resize(980, 620)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self.view = QWebEngineView(self)
        dt_page = QWebEnginePage(target_page.profile(), self.view)
        self.view.setPage(dt_page)
        target_page.setDevToolsPage(dt_page)
        layout.addWidget(self.view)

    def closeEvent(self, event) -> None:
        self.view.setPage(None)
        super().closeEvent(event)
