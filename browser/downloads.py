"""Downloads manager and dialog inspired by Chrome's Downloads manager."""

from __future__ import annotations

import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional

from PyQt6.QtCore import QObject, QUrl, pyqtSignal
from PyQt6.QtGui import QDesktopServices
from PyQt6.QtWebEngineCore import QWebEngineDownloadRequest
from PyQt6.QtWidgets import (
    QDialog,
    QHBoxLayout,
    QHeaderView,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
)

from .icons import svg_icon
from .library_dialogs import _DIALOG_QSS

_DS = QWebEngineDownloadRequest.DownloadState


def format_bytes(size: int) -> str:
    if size <= 0:
        return "0 B"
    for unit in ("B", "KB", "MB", "GB"):
        if size < 1024 or unit == "GB":
            return f"{size:.1f} {unit}" if unit != "B" else f"{size} B"
        size /= 1024.0
    return f"{size:.1f} GB"


@dataclass
class DownloadRecord:
    """Tracks state and progress of a single file download."""

    file_name: str
    save_path: str
    url: str
    received_bytes: int = 0
    total_bytes: int = 0
    state_label: str = "İndiriliyor"
    request: Optional[QWebEngineDownloadRequest] = None

    @property
    def is_finished(self) -> bool:
        return self.state_label in {"Tamamlandı", "İptal Edildi", "Kesildi"}


class DownloadsManager(QObject):
    """Stores active and completed downloads for the current session."""

    downloadsChanged = pyqtSignal()

    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self.records: List[DownloadRecord] = []

    def track(self, download: QWebEngineDownloadRequest, save_path: str) -> DownloadRecord:
        rec = DownloadRecord(
            file_name=os.path.basename(save_path),
            save_path=save_path,
            url=download.url().toString(),
            total_bytes=max(0, download.totalBytes()),
            request=download,
        )
        self.records.insert(0, rec)

        def on_received(received: int, total: int) -> None:
            rec.received_bytes = max(0, received)
            rec.total_bytes = max(0, total)
            self.downloadsChanged.emit()

        def on_state(state: QWebEngineDownloadRequest.DownloadState) -> None:
            if state == _DS.DownloadCompleted:
                rec.state_label = "Tamamlandı"
                rec.request = None
            elif state == _DS.DownloadCancelled:
                rec.state_label = "İptal Edildi"
                rec.request = None
            elif state == _DS.DownloadInterrupted:
                rec.state_label = "Kesildi"
                rec.request = None
            elif state == _DS.DownloadInProgress:
                rec.state_label = "İndiriliyor"
            self.downloadsChanged.emit()

        download.receivedBytesChanged.connect(
            lambda: on_received(download.receivedBytes(), download.totalBytes())
        )
        download.stateChanged.connect(on_state)
        self.downloadsChanged.emit()
        return rec

    def clear_finished(self) -> None:
        self.records = [r for r in self.records if not r.is_finished]
        self.downloadsChanged.emit()


class DownloadsDialog(QDialog):
    """Downloads manager window with progress, cancel, open, and reveal-in-folder."""

    def __init__(self, manager: DownloadsManager, parent=None) -> None:
        super().__init__(parent)
        self.manager = manager
        self.setWindowTitle("İndirilenler")
        self.setWindowIcon(svg_icon("download", "#8ab4f8", 18))
        self.resize(700, 420)
        self.setStyleSheet(_DIALOG_QSS)

        layout = QVBoxLayout(self)

        self.table = QTableWidget(0, 4)
        self.table.setHorizontalHeaderLabels(["Dosya", "Boyut / İlerleme", "Durum", "Konum"])
        hdr = self.table.horizontalHeader()
        hdr.setSectionResizeMode(0, QHeaderView.ResizeMode.Interactive)
        hdr.setSectionResizeMode(1, QHeaderView.ResizeMode.ResizeToContents)
        hdr.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        hdr.setSectionResizeMode(3, QHeaderView.ResizeMode.Stretch)
        self.table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        layout.addWidget(self.table, 1)

        btns = QHBoxLayout()
        open_btn = QPushButton("Dosyayı Aç")
        open_btn.setObjectName("PrimaryBtn")
        open_btn.clicked.connect(self._open_file)

        reveal_btn = QPushButton("Klasörde Göster")
        reveal_btn.clicked.connect(self._reveal_in_folder)

        cancel_btn = QPushButton("İndirmeyi İptal Et")
        cancel_btn.setObjectName("DangerBtn")
        cancel_btn.clicked.connect(self._cancel_selected)

        clear_btn = QPushButton("Tamamlananları Temizle")
        clear_btn.clicked.connect(self.manager.clear_finished)

        close_btn = QPushButton("Kapat")
        close_btn.clicked.connect(self.accept)

        btns.addWidget(open_btn)
        btns.addWidget(reveal_btn)
        btns.addWidget(cancel_btn)
        btns.addStretch()
        btns.addWidget(clear_btn)
        btns.addWidget(close_btn)
        layout.addLayout(btns)

        self.manager.downloadsChanged.connect(self._refresh)
        self._refresh()

    def _refresh(self) -> None:
        self.table.setRowCount(len(self.manager.records))
        for i, rec in enumerate(self.manager.records):
            if rec.total_bytes > 0:
                pct = int(min(100, (rec.received_bytes * 100) // rec.total_bytes))
                size_str = f"{format_bytes(rec.received_bytes)} / {format_bytes(rec.total_bytes)} ({pct}%)"
            else:
                size_str = format_bytes(rec.received_bytes)
            self.table.setItem(i, 0, QTableWidgetItem(rec.file_name))
            self.table.setItem(i, 1, QTableWidgetItem(size_str))
            self.table.setItem(i, 2, QTableWidgetItem(rec.state_label))
            self.table.setItem(i, 3, QTableWidgetItem(rec.save_path))

    def _selected_record(self) -> Optional[DownloadRecord]:
        row = self.table.currentRow()
        if 0 <= row < len(self.manager.records):
            return self.manager.records[row]
        return None

    def _open_file(self) -> None:
        rec = self._selected_record()
        if rec and Path(rec.save_path).exists():
            QDesktopServices.openUrl(QUrl.fromLocalFile(rec.save_path))

    def _reveal_in_folder(self) -> None:
        rec = self._selected_record()
        if not rec:
            return
        path = Path(rec.save_path)
        if not path.exists():
            QDesktopServices.openUrl(QUrl.fromLocalFile(str(path.parent)))
            return
        if sys.platform == "darwin":
            subprocess.Popen(["open", "-R", str(path)])
        elif sys.platform.startswith("win"):
            subprocess.Popen(["explorer", f"/select,{path}"])
        else:
            QDesktopServices.openUrl(QUrl.fromLocalFile(str(path.parent)))

    def _cancel_selected(self) -> None:
        rec = self._selected_record()
        if rec and rec.request is not None:
            rec.request.cancel()
