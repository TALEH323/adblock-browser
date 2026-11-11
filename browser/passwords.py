"""Encrypted password storage and QWebChannel bridge for autofill and capture."""

from __future__ import annotations

import json
import os
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import Path
from typing import List, Optional
from urllib.parse import urlparse

from cryptography.fernet import Fernet, InvalidToken
from PyQt6.QtCore import QObject, pyqtSignal, pyqtSlot

from adblock.engine import extract_host


@dataclass
class PasswordEntry:
    """Single saved website credential (decrypted in memory, encrypted on disk)."""

    origin: str
    username: str
    password: str
    updated_at: str = ""


class PasswordManager:
    """Manages Fernet-encrypted website passwords persisted to passwords.json."""

    def __init__(self, data_dir: Path) -> None:
        self.data_dir = Path(data_dir)
        self.data_dir.mkdir(parents=True, exist_ok=True)
        self.key_path = self.data_dir / ".pw_key"
        self.store_path = self.data_dir / "passwords.json"
        self._fernet = Fernet(self._load_or_create_key())
        self.entries: List[PasswordEntry] = []
        self.never_save_hosts: List[str] = []
        self.load()

    def _load_or_create_key(self) -> bytes:
        if self.key_path.exists():
            raw = self.key_path.read_bytes().strip()
            if raw:
                return raw
        key = Fernet.generate_key()
        self.key_path.write_bytes(key)
        try:
            os.chmod(self.key_path, 0o600)
        except OSError:
            pass
        return key

    def _encrypt(self, plaintext: str) -> str:
        return self._fernet.encrypt(plaintext.encode("utf-8")).decode("ascii")

    def _decrypt(self, token: str) -> Optional[str]:
        try:
            return self._fernet.decrypt(token.encode("ascii")).decode("utf-8")
        except (InvalidToken, ValueError, TypeError):
            return None

    def load(self) -> None:
        self.entries.clear()
        self.never_save_hosts.clear()
        if not self.store_path.exists():
            return
        try:
            raw = json.loads(self.store_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return

        if not isinstance(raw, dict):
            return

        self.never_save_hosts = [
            str(h).lower() for h in raw.get("never_save_hosts", []) if h
        ]

        for item in raw.get("credentials", []):
            if not isinstance(item, dict):
                continue
            enc_pw = str(item.get("password_enc", ""))
            pw = self._decrypt(enc_pw)
            if pw is None:
                continue
            origin = str(item.get("origin", ""))
            username = str(item.get("username", ""))
            updated_at = str(item.get("updated_at", ""))
            if origin and pw:
                self.entries.append(
                    PasswordEntry(
                        origin=origin,
                        username=username,
                        password=pw,
                        updated_at=updated_at,
                    )
                )

    def save(self) -> None:
        payload = {
            "never_save_hosts": sorted(set(self.never_save_hosts)),
            "credentials": [
                {
                    "origin": e.origin,
                    "username": e.username,
                    "password_enc": self._encrypt(e.password),
                    "updated_at": e.updated_at,
                }
                for e in self.entries
            ],
        }
        tmp = self.store_path.with_suffix(".tmp")
        tmp.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
        tmp.replace(self.store_path)

    @staticmethod
    def normalize_origin(url: str) -> str:
        parsed = urlparse(url if "://" in url else f"https://{url}")
        if not parsed.scheme or not parsed.netloc:
            return ""
        return f"{parsed.scheme.lower()}://{parsed.netloc.lower()}"

    def is_never_saved(self, url_or_origin: str) -> bool:
        host = extract_host(url_or_origin)
        return bool(host and host in self.never_save_hosts)

    def add_never_save(self, url_or_origin: str) -> None:
        host = extract_host(url_or_origin)
        if host and host not in self.never_save_hosts:
            self.never_save_hosts.append(host)
            self.save()

    def remove_never_save(self, host: str) -> None:
        h = host.lower().strip()
        if h in self.never_save_hosts:
            self.never_save_hosts.remove(h)
            self.save()

    def has_exact_credential(self, origin: str, username: str, password: str) -> bool:
        host = extract_host(origin)
        if not host:
            return False
        for e in self.entries:
            if extract_host(e.origin) == host and e.username == username and e.password == password:
                return True
        return False

    def has_username_for_host(self, origin: str, username: str) -> bool:
        host = extract_host(origin)
        if not host:
            return False
        return any(
            extract_host(e.origin) == host and e.username == username
            for e in self.entries
        )

    def save_credential(self, origin: str, username: str, password: str) -> None:
        norm_origin = self.normalize_origin(origin)
        host = extract_host(norm_origin)
        if not norm_origin or not host or not password:
            return

        now = datetime.now().strftime("%Y-%m-%d %H:%M")
        for e in self.entries:
            if extract_host(e.origin) == host and e.username == username:
                e.origin = norm_origin
                e.password = password
                e.updated_at = now
                self.save()
                return

        self.entries.insert(
            0,
            PasswordEntry(
                origin=norm_origin,
                username=username,
                password=password,
                updated_at=now,
            ),
        )
        self.save()

    def get_credentials_for_url(self, page_url: str) -> List[PasswordEntry]:
        parsed = urlparse(page_url)
        if parsed.scheme not in {"http", "https"}:
            return []
        page_host = extract_host(page_url)
        if not page_host:
            return []

        matches: List[PasswordEntry] = []
        for e in self.entries:
            saved_parsed = urlparse(e.origin)
            # Never auto-fill HTTPS credentials onto an insecure HTTP page
            if saved_parsed.scheme == "https" and parsed.scheme != "https":
                continue
            saved_host = extract_host(e.origin)
            if saved_host == page_host:
                matches.append(e)
        return matches

    def delete_credential(self, origin: str, username: str) -> None:
        self.entries = [
            e
            for e in self.entries
            if not (e.origin == origin and e.username == username)
        ]
        self.save()

    def clear_all(self) -> None:
        self.entries.clear()
        self.save()


class PasswordBridge(QObject):
    """QWebChannel bridge exposed ONLY to ApplicationWorld (isolated from webpage JS)."""

    savePromptRequested = pyqtSignal(str, str, str)

    def __init__(self, manager: PasswordManager, is_enabled_fn, parent=None) -> None:
        super().__init__(parent)
        self.manager = manager
        self._is_enabled_fn = is_enabled_fn

    @pyqtSlot(str, str, str)
    def onCredentialsSubmitted(self, origin: str, username: str, password: str) -> None:
        if not self._is_enabled_fn():
            return
        if not origin or not password:
            return
        if not (origin.startswith("https://") or origin.startswith("http://")):
            return
        if self.manager.is_never_saved(origin):
            return
        if self.manager.has_exact_credential(origin, username, password):
            return
        self.savePromptRequested.emit(origin, username, password)

    @pyqtSlot(str, result=str)
    def getSavedCredentialsJson(self, page_url: str) -> str:
        if not self._is_enabled_fn():
            return "[]"
        creds = self.manager.get_credentials_for_url(page_url)
        return json.dumps(
            [{"username": c.username, "password": c.password} for c in creds],
            ensure_ascii=False,
        )
