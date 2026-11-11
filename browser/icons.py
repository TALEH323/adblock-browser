"""Crisp vector SVG icon factory for Chrome-inspired browser UI."""

from __future__ import annotations

from functools import lru_cache
from typing import Dict

from PyQt6.QtCore import QByteArray, QSize, Qt
from PyQt6.QtGui import QIcon, QPainter, QPixmap
from PyQt6.QtSvg import QSvgRenderer

_SVG_PATHS: Dict[str, str] = {
    "back": (
        '<path d="M19 12H5M12 19l-7-7 7-7" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "forward": (
        '<path d="M5 12h14M12 5l7 7-7 7" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "reload": (
        '<path d="M21 12a9 9 0 1 1-2.64-6.36" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round"/>'
        '<polyline points="21 3 21 9 15 9" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "stop": (
        '<path d="M18 6L6 18M6 6l12 12" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round"/>'
    ),
    "home": (
        '<path d="M3 10.5L12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z" '
        'fill="none" stroke="{c}" stroke-width="1.9" stroke-linejoin="round"/>'
    ),
    "plus": (
        '<path d="M12 5v14M5 12h14" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round"/>'
    ),
    "close": (
        '<path d="M16 8l-8 8M8 8l8 8" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round"/>'
    ),
    "star_outline": (
        '<polygon points="12 3 14.9 8.8 21.3 9.7 16.6 14.2 17.7 20.6 12 17.6 '
        '6.3 20.6 7.4 14.2 2.7 9.7 9.1 8.8" fill="none" stroke="{c}" '
        'stroke-width="1.8" stroke-linejoin="round"/>'
    ),
    "star_filled": (
        '<polygon points="12 3 14.9 8.8 21.3 9.7 16.6 14.2 17.7 20.6 12 17.6 '
        '6.3 20.6 7.4 14.2 2.7 9.7 9.1 8.8" fill="{c}" stroke="{c}" '
        'stroke-width="1.5" stroke-linejoin="round"/>'
    ),
    "shield": (
        '<path d="M12 2.5l7.5 3.2v5.8c0 4.9-3.2 9.1-7.5 10.5-4.3-1.4-7.5-5.6-7.5-10.5V5.7z" '
        'fill="none" stroke="{c}" stroke-width="1.9" stroke-linejoin="round"/>'
        '<path d="M9.2 12.2l2 2 3.8-4.1" fill="none" stroke="{c}" '
        'stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "shield_off": (
        '<path d="M12 2.5l7.5 3.2v5.8c0 4.9-3.2 9.1-7.5 10.5-4.3-1.4-7.5-5.6-7.5-10.5V5.7z" '
        'fill="none" stroke="{c}" stroke-width="1.9" stroke-linejoin="round"/>'
        '<path d="M9.5 9.5l5 5M14.5 9.5l-5 5" fill="none" stroke="{c}" '
        'stroke-width="1.9" stroke-linecap="round"/>'
    ),
    "lock": (
        '<rect x="5" y="11" width="14" height="10" rx="2" fill="none" stroke="{c}" stroke-width="1.9"/>'
        '<path d="M8 11V7.5a4 4 0 0 1 8 0V11" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round"/>'
    ),
    "warning": (
        '<path d="M12 3.5L2.8 19.5h18.4z" fill="none" stroke="{c}" stroke-width="1.9" stroke-linejoin="round"/>'
        '<path d="M12 9.5v4.5" fill="none" stroke="{c}" stroke-width="2" stroke-linecap="round"/>'
        '<circle cx="12" cy="16.8" r="1" fill="{c}"/>'
    ),
    "info": (
        '<circle cx="12" cy="12" r="9" fill="none" stroke="{c}" stroke-width="1.9"/>'
        '<path d="M12 11v5" fill="none" stroke="{c}" stroke-width="2" stroke-linecap="round"/>'
        '<circle cx="12" cy="7.8" r="1" fill="{c}"/>'
    ),
    "search": (
        '<circle cx="11" cy="11" r="6.8" fill="none" stroke="{c}" stroke-width="1.9"/>'
        '<path d="M16.2 16.2L20.5 20.5" fill="none" stroke="{c}" stroke-width="2" stroke-linecap="round"/>'
    ),
    "history": (
        '<path d="M3.5 12a8.5 8.5 0 1 0 2.5-6" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round"/>'
        '<polyline points="3.5 4.5 3.5 8.8 7.8 8.8" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/>'
        '<path d="M12 7.5V12l3 2" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "bookmarks": (
        '<path d="M6 4h12a1 1 0 0 1 1 1v16l-7-4-7 4V5a1 1 0 0 1 1-1z" '
        'fill="none" stroke="{c}" stroke-width="1.9" stroke-linejoin="round"/>'
    ),
    "key": (
        '<circle cx="8" cy="14" r="4" fill="none" stroke="{c}" stroke-width="1.9"/>'
        '<path d="M11 11.2L19.5 3.5M17 5.8l2.5 2.5M14.8 8l2 2" fill="none" stroke="{c}" '
        'stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "download": (
        '<path d="M12 4v11M7.5 11.5L12 16l4.5-4.5" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
        '<path d="M5 19.5h14" fill="none" stroke="{c}" stroke-width="2" stroke-linecap="round"/>'
    ),
    "settings": (
        '<circle cx="12" cy="12" r="3" fill="none" stroke="{c}" stroke-width="1.9"/>'
        '<path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 '
        '1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 '
        '0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 '
        '0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3h.1a1.6 '
        '1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 '
        '1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8v.1a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 '
        '0 0 0-1.5 1z" fill="none" stroke="{c}" stroke-width="1.6" stroke-linejoin="round"/>'
    ),
    "more_vert": (
        '<circle cx="12" cy="5.5" r="1.6" fill="{c}"/>'
        '<circle cx="12" cy="12" r="1.6" fill="{c}"/>'
        '<circle cx="12" cy="18.5" r="1.6" fill="{c}"/>'
    ),
    "incognito": (
        '<path d="M3 12h18" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round"/>'
        '<path d="M6 12l1.8-6.2A1.5 1.5 0 0 1 9.2 4.8h5.6a1.5 1.5 0 0 1 1.4 1L18 12" '
        'fill="none" stroke="{c}" stroke-width="1.8" stroke-linejoin="round"/>'
        '<circle cx="8" cy="16.5" r="2.6" fill="none" stroke="{c}" stroke-width="1.8"/>'
        '<circle cx="16" cy="16.5" r="2.6" fill="none" stroke="{c}" stroke-width="1.8"/>'
        '<path d="M10.6 16.5h2.8" fill="none" stroke="{c}" stroke-width="1.8"/>'
    ),
    "volume_on": (
        '<polygon points="4 9.5 8 9.5 13 5 13 19 8 14.5 4 14.5" fill="none" stroke="{c}" '
        'stroke-width="1.8" stroke-linejoin="round"/>'
        '<path d="M16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12" fill="none" stroke="{c}" '
        'stroke-width="1.8" stroke-linecap="round"/>'
    ),
    "volume_off": (
        '<polygon points="4 9.5 8 9.5 13 5 13 19 8 14.5 4 14.5" fill="none" stroke="{c}" '
        'stroke-width="1.8" stroke-linejoin="round"/>'
        '<path d="M16.5 9.5l5 5M21.5 9.5l-5 5" fill="none" stroke="{c}" stroke-width="1.8" stroke-linecap="round"/>'
    ),
    "eye": (
        '<path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6z" '
        'fill="none" stroke="{c}" stroke-width="1.8"/>'
        '<circle cx="12" cy="12" r="2.5" fill="none" stroke="{c}" stroke-width="1.8"/>'
    ),
    "eye_off": (
        '<path d="M3 3l18 18M10.6 6.2A9.8 9.8 0 0 1 12 6c6 0 9.5 6 9.5 6a16.8 16.8 0 0 1-3.3 3.9M6.6 '
        '8.2A16.7 16.7 0 0 0 2.5 12s3.5 6 9.5 6c1.5 0 2.9-.3 4.1-.9" '
        'fill="none" stroke="{c}" stroke-width="1.8" stroke-linecap="round"/>'
    ),
    "arrow_up": (
        '<path d="M12 18V6M6 12l6-6 6 6" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "arrow_down": (
        '<path d="M12 6v12M6 12l6 6 6-6" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "reopen": (
        '<path d="M9 14l-4-4 4-4" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/>'
        '<path d="M5 10h9a5 5 0 1 1 0 10h-3" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round"/>'
    ),
    "pdf": (
        '<path d="M6 3h8l5 5v12a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" '
        'fill="none" stroke="{c}" stroke-width="1.8" stroke-linejoin="round"/>'
        '<polyline points="14 3 14 8 19 8" fill="none" stroke="{c}" stroke-width="1.8" stroke-linejoin="round"/>'
    ),
    "code": (
        '<polyline points="8 7 3 12 8 17" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/>'
        '<polyline points="16 7 21 12 16 17" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/>'
        '<path d="M14 5l-4 14" fill="none" stroke="{c}" stroke-width="1.8" stroke-linecap="round"/>'
    ),
    "devtools": (
        '<path d="M14.7 6.3a4.5 4.5 0 0 0-5.4 6.6L4.5 17.7a1.5 1.5 0 0 0 2.1 2.1l4.8-4.8a4.5 4.5 0 0 0 '
        '6.6-5.4l-2.6 2.6-2.2-2.2z" fill="none" stroke="{c}" stroke-width="1.8" stroke-linejoin="round"/>'
    ),
    "chevron_down": (
        '<polyline points="6 9 12 15 18 9" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "chevron_right": (
        '<polyline points="9 6 15 12 9 18" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "chevron_left": (
        '<polyline points="15 6 9 12 15 18" fill="none" stroke="{c}" '
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "external": (
        '<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" '
        'fill="none" stroke="{c}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>'
        '<polyline points="15 3 21 3 21 9" fill="none" stroke="{c}" '
        'stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>'
        '<line x1="10" y1="14" x2="21" y2="3" stroke="{c}" stroke-width="1.8" stroke-linecap="round"/>'
    ),
    "folder": (
        '<path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" '
        'fill="none" stroke="{c}" stroke-width="1.8" stroke-linejoin="round"/>'
    ),
    "copy": (
        '<rect x="9" y="9" width="11" height="11" rx="2" fill="none" stroke="{c}" stroke-width="1.8"/>'
        '<path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" '
        'fill="none" stroke="{c}" stroke-width="1.8" stroke-linecap="round"/>'
    ),
    "trash": (
        '<polyline points="3 6 5 6 21 6" fill="none" stroke="{c}" stroke-width="1.8" stroke-linecap="round"/>'
        '<path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6m3 0V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2" '
        'fill="none" stroke="{c}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>'
    ),
    "file": (
        '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" '
        'fill="none" stroke="{c}" stroke-width="1.8" stroke-linejoin="round"/>'
        '<polyline points="14 2 14 8 20 8" fill="none" stroke="{c}" stroke-width="1.8" stroke-linejoin="round"/>'
    ),
}


@lru_cache(maxsize=256)
def svg_icon(name: str, color: str = "#e8eaed", size: int = 18) -> QIcon:
    """Render an inline vector SVG into a crisp HiDPI QIcon."""
    body = _SVG_PATHS.get(name, _SVG_PATHS["info"]).format(c=color)
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">{body}</svg>'
    renderer = QSvgRenderer(QByteArray(svg.encode("utf-8")))

    scale = 2
    pixmap = QPixmap(size * scale, size * scale)
    pixmap.fill(Qt.GlobalColor.transparent)
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    renderer.render(painter)
    painter.end()
    pixmap.setDevicePixelRatio(scale)
    return QIcon(pixmap)


def svg_pixmap(name: str, color: str = "#e8eaed", size: int = 16) -> QPixmap:
    """Return a crisp HiDPI QPixmap for inline QLabel icons."""
    return svg_icon(name, color=color, size=size).pixmap(QSize(size, size))
