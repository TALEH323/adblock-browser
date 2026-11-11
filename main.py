"""Entry point for AdBlock Desktop Browser."""

from __future__ import annotations

import os
import sys
from pathlib import Path

from PyQt6.QtGui import QIcon
from PyQt6.QtWidgets import QApplication

from browser.main_window import MainWindow


def get_data_dir() -> Path:
    """Return a writable data directory for both source runs and frozen Windows/macOS builds."""
    if getattr(sys, "frozen", False):
        if sys.platform.startswith("win"):
            base = Path(os.environ.get("LOCALAPPDATA") or Path.home() / "AppData" / "Local")
            return base / "AdBlockTarayici"
        return Path.home() / ".adblock_tarayici"
    return Path(__file__).resolve().parent / "user_data"


def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName("AdBlock Tarayıcı")
    app.setOrganizationName("AdBlockBrowser")
    icon_file = Path(__file__).resolve().parent / "rust-browser" / "assets" / "icon.png"
    if icon_file.exists():
        app.setWindowIcon(QIcon(str(icon_file)))

    window = MainWindow(data_dir=get_data_dir())
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
