"""Persistent JSON storage for browser settings, bookmarks, and history."""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List


SEARCH_ENGINES = {
    "duckduckgo": ("DuckDuckGo", "https://duckduckgo.com/?q={query}"),
    "google": ("Google", "https://www.google.com/search?q={query}"),
    "bing": ("Bing", "https://www.bing.com/search?q={query}"),
}


def _read_json(path: Path, default: Any) -> Any:
    if not path.exists():
        return default
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return default


def _write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
    tmp.replace(path)


@dataclass
class BookmarkItem:
    title: str
    url: str
    created_at: str = ""


@dataclass
class HistoryItem:
    title: str
    url: str
    visited_at: str = ""


class SettingsManager:
    """Manages browser & adblock configuration persisted to settings.json."""

    DEFAULT_SETTINGS: Dict[str, Any] = {
        "search_engine": "google",
        "homepage": "adblock://home",
        "adblock_enabled": True,
        "cosmetic_enabled": True,
        "show_bookmarks_bar": True,
        "password_manager_enabled": True,
        "https_only_enabled": True,
        "doh_provider": "cloudflare_security",
        "http_allowed_domains": [],
        "whitelisted_domains": [],
        "total_blocked": 0,
    }

    def __init__(self, data_dir: Path) -> None:
        self.path = Path(data_dir) / "settings.json"
        self.data: Dict[str, Any] = dict(self.DEFAULT_SETTINGS)
        self.load()

    def load(self) -> None:
        loaded = _read_json(self.path, {})
        if isinstance(loaded, dict):
            self.data.update(loaded)

    def save(self) -> None:
        _write_json(self.path, self.data)

    def get(self, key: str, default: Any = None) -> Any:
        return self.data.get(key, default)

    def set(self, key: str, value: Any) -> None:
        self.data[key] = value
        self.save()

    def get_search_url(self, query: str) -> str:
        from urllib.parse import quote_plus

        engine_key = self.data.get("search_engine", "google")
        _, template = SEARCH_ENGINES.get(engine_key, SEARCH_ENGINES["google"])
        return template.format(query=quote_plus(query))


class BookmarksManager:
    """Manages user bookmarks persisted to bookmarks.json."""

    DEFAULT_BOOKMARKS = [
        {"title": "Google", "url": "https://www.google.com"},
        {"title": "DuckDuckGo", "url": "https://duckduckgo.com"},
        {"title": "YouTube", "url": "https://www.youtube.com"},
        {"title": "Wikipedia", "url": "https://tr.wikipedia.org"},
        {"title": "GitHub", "url": "https://github.com"},
    ]

    def __init__(self, data_dir: Path) -> None:
        self.path = Path(data_dir) / "bookmarks.json"
        self.items: List[BookmarkItem] = []
        self.load()

    def load(self) -> None:
        if not self.path.exists():
            now = datetime.now().strftime("%Y-%m-%d %H:%M")
            self.items = [
                BookmarkItem(title=b["title"], url=b["url"], created_at=now)
                for b in self.DEFAULT_BOOKMARKS
            ]
            self.save()
            return
        raw = _read_json(self.path, [])
        self.items = [
            BookmarkItem(
                title=str(item.get("title", "")),
                url=str(item.get("url", "")),
                created_at=str(item.get("created_at", "")),
            )
            for item in raw
            if isinstance(item, dict) and item.get("url")
        ]

    def save(self) -> None:
        _write_json(self.path, [asdict(item) for item in self.items])

    def is_bookmarked(self, url: str) -> bool:
        norm = url.rstrip("/")
        return any(item.url.rstrip("/") == norm for item in self.items)

    def add(self, title: str, url: str) -> None:
        if not url or url.startswith("adblock://") or url.startswith("data:"):
            return
        norm = url.rstrip("/")
        if self.is_bookmarked(norm):
            return
        clean_title = (title or url).strip()
        self.items.append(
            BookmarkItem(
                title=clean_title,
                url=url,
                created_at=datetime.now().strftime("%Y-%m-%d %H:%M"),
            )
        )
        self.save()

    def remove(self, url: str) -> None:
        norm = url.rstrip("/")
        self.items = [item for item in self.items if item.url.rstrip("/") != norm]
        self.save()

    def toggle(self, title: str, url: str) -> bool:
        """Toggle bookmark state. Returns True if now bookmarked."""
        if self.is_bookmarked(url):
            self.remove(url)
            return False
        self.add(title, url)
        return True


class HistoryManager:
    """Manages browsing history persisted to history.json."""

    MAX_ITEMS = 2000

    def __init__(self, data_dir: Path) -> None:
        self.path = Path(data_dir) / "history.json"
        self.items: List[HistoryItem] = []
        self.load()

    def load(self) -> None:
        raw = _read_json(self.path, [])
        self.items = [
            HistoryItem(
                title=str(item.get("title", "")),
                url=str(item.get("url", "")),
                visited_at=str(item.get("visited_at", "")),
            )
            for item in raw
            if isinstance(item, dict) and item.get("url")
        ]

    def save(self) -> None:
        _write_json(self.path, [asdict(item) for item in self.items[: self.MAX_ITEMS]])

    def record(self, title: str, url: str) -> None:
        if not url or not (url.startswith("http://") or url.startswith("https://")):
            return
        now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        clean_title = (title or url).strip()

        # Avoid duplicate consecutive entry for the exact same URL
        if self.items and self.items[0].url == url:
            if clean_title and clean_title != url:
                self.items[0].title = clean_title
            self.items[0].visited_at = now
            self.save()
            return

        self.items.insert(0, HistoryItem(title=clean_title, url=url, visited_at=now))
        if len(self.items) > self.MAX_ITEMS:
            self.items = self.items[: self.MAX_ITEMS]
        self.save()

    def search(self, query: str) -> List[HistoryItem]:
        q = query.strip().lower()
        if not q:
            return list(self.items)
        return [
            item
            for item in self.items
            if q in item.title.lower() or q in item.url.lower()
        ]

    def remove_at(self, index: int) -> None:
        if 0 <= index < len(self.items):
            self.items.pop(index)
            self.save()

    def clear(self) -> None:
        self.items.clear()
        self.save()
