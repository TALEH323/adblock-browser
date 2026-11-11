import html
import socket
import ssl
from typing import Callable, Optional
from urllib.parse import quote

from PyQt6.QtCore import Qt, QThread, pyqtSignal
from PyQt6.QtWidgets import (
    QCheckBox,
    QDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QVBoxLayout,
)

from adblock.dns_guard import DnsCheckResult, DnsGuard
from browser.icons import svg_pixmap


class _CertFetchWorker(QThread):
    """Fetches TLS certificate metadata for an HTTPS host without blocking UI."""

    fetched = pyqtSignal(dict)

    def __init__(self, host: str, port: int = 443, parent=None):
        super().__init__(parent)
        self.host = host
        self.port = port

    def run(self):
        info: dict = {"ok": False, "error": ""}
        try:
            ctx = ssl.create_default_context()
            with socket.create_connection((self.host, self.port), timeout=2.5) as sock:
                with ctx.wrap_socket(sock, server_hostname=self.host) as ssock:
                    cert = ssock.getpeercert() or {}
                    cipher = ssock.cipher()
                    proto = ssock.version()
                    subject = dict(x[0] for x in cert.get("subject", ()))
                    issuer = dict(x[0] for x in cert.get("issuer", ()))
                    info = {
                        "ok": True,
                        "common_name": subject.get("commonName", self.host),
                        "issuer_org": issuer.get("organizationName")
                        or issuer.get("commonName")
                        or "Bilinmiyor",
                        "not_before": cert.get("notBefore", ""),
                        "not_after": cert.get("notAfter", ""),
                        "protocol": proto or "TLS",
                        "cipher": cipher[0] if cipher else "",
                    }
        except Exception as exc:
            info = {"ok": False, "error": str(exc)}
        self.fetched.emit(info)


_SECURITY_DIALOG_QSS = """
QDialog {
    background-color: #202124;
    color: #e8eaed;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}
QLabel#Title {
    color: #e8eaed;
    font-size: 15px;
    font-weight: 600;
}
QLabel#Host {
    color: #9aa0a6;
    font-size: 12px;
}
QFrame#Card {
    background-color: #292a2d;
    border: 1px solid #3c4043;
    border-radius: 10px;
}
QLabel#CardTitle {
    color: #e8eaed;
    font-size: 13px;
    font-weight: 600;
}
QLabel#CardSub {
    color: #9aa0a6;
    font-size: 12px;
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
QPushButton {
    background-color: #303134;
    color: #e8eaed;
    border: 1px solid #3c4043;
    border-radius: 8px;
    padding: 7px 14px;
    font-size: 12.5px;
}
QPushButton:hover {
    background-color: #3c4043;
}
QPushButton#Primary {
    background-color: #8ab4f8;
    color: #202124;
    border: none;
    font-weight: 600;
}
QPushButton#Primary:hover {
    background-color: #aecbfa;
}
"""


class SiteSecurityDialog(QDialog):
    """Chrome-style site security & DoH DNS inspection popup."""

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
        parent=None,
    ):
        super().__init__(parent)
        self.host = host
        self.scheme = scheme
        self.on_toggle_http_allowed = on_toggle_http_allowed
        self.on_toggle_adblock_whitelist = on_toggle_adblock_whitelist
        self._worker: Optional[_CertFetchWorker] = None

        self.setWindowTitle("Site Güvenliği ve DNS Koruması")
        self.setMinimumWidth(420)
        self.setStyleSheet(_SECURITY_DIALOG_QSS)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(12)

        # Header
        header = QHBoxLayout()
        header.setSpacing(10)
        icon_label = QLabel()

        is_https = scheme == "https"
        if cert_error_desc:
            icon_name, icon_color = "warning", "#f28b82"
            status_text = "Sertifika Hatası (Güvensiz Bağlantı)"
        elif is_https:
            icon_name, icon_color = "lock", "#81c995"
            status_text = "Bağlantı Güvenli (HTTPS)"
        elif scheme == "adblock":
            icon_name, icon_color = "shield", "#8ab4f8"
            status_text = "Dahili Güvenli Sayfa"
        else:
            icon_name, icon_color = "warning", "#fdd663"
            status_text = "Bağlantı Şifrelenmemiş (HTTP)"

        icon_label.setPixmap(svg_pixmap(icon_name, icon_color, 22))
        header.addWidget(icon_label)

        title_col = QVBoxLayout()
        title_col.setSpacing(2)
        title_lbl = QLabel(status_text)
        title_lbl.setObjectName("Title")
        host_lbl = QLabel(host or url_str)
        host_lbl.setObjectName("Host")
        host_lbl.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        title_col.addWidget(title_lbl)
        title_col.addWidget(host_lbl)
        header.addLayout(title_col, 1)
        layout.addLayout(header)

        # TLS / Certificate Card
        tls_card = QFrame()
        tls_card.setObjectName("Card")
        tls_layout = QVBoxLayout(tls_card)
        tls_layout.setContentsMargins(12, 10, 12, 10)
        tls_layout.setSpacing(4)
        tls_title = QLabel("TLS / SSL Sertifikası")
        tls_title.setObjectName("CardTitle")
        tls_layout.addWidget(tls_title)

        self.cert_detail_lbl = QLabel()
        self.cert_detail_lbl.setObjectName("CardSub")
        self.cert_detail_lbl.setWordWrap(True)
        tls_layout.addWidget(self.cert_detail_lbl)

        if cert_error_desc:
            self.cert_detail_lbl.setText(f"Hata: {cert_error_desc}")
        elif is_https and host:
            self.cert_detail_lbl.setText("Sertifika bilgileri doğrulanıyor...")
            self._worker = _CertFetchWorker(host, 443, self)
            self._worker.fetched.connect(self._on_cert_fetched)
            self._worker.start()
        elif scheme == "adblock":
            self.cert_detail_lbl.setText("Yerel tarayıcı arayüzü; harici ağ trafiği yok.")
        else:
            note = (
                "HTTP üzerinden bağlanıldı. Parola veya kart bilgisi girmeyin."
                if not https_only_enabled
                else "HTTP istisnası etkin veya site HTTPS desteklemiyor."
            )
            self.cert_detail_lbl.setText(note)
        layout.addWidget(tls_card)

        # DNS-over-HTTPS Card
        dns_card = QFrame()
        dns_card.setObjectName("Card")
        dns_layout = QVBoxLayout(dns_card)
        dns_layout.setContentsMargins(12, 10, 12, 10)
        dns_layout.setSpacing(4)
        dns_title = QLabel(f"DNS Koruması (DoH): {dns_guard.get_provider_name()}")
        dns_title.setObjectName("CardTitle")
        dns_layout.addWidget(dns_title)

        if dns_guard.provider_id == "off":
            dns_text = "DNS-over-HTTPS koruması devre dışı. Ayarlardan etkinleştirebilirsiniz."
        elif dns_result is None:
            dns_text = "Bu alan adı için henüz harici DoH sorgusu yapılmadı (yerel veya önbelleksiz)."
        elif dns_result.blocked:
            dns_text = f"ENGELLENDİ: {dns_result.reason}"
        else:
            dns_text = f"Temiz: {dns_result.reason}"
        dns_sub = QLabel(dns_text)
        dns_sub.setObjectName("CardSub")
        dns_sub.setWordWrap(True)
        dns_layout.addWidget(dns_sub)
        layout.addWidget(dns_card)

        # Site Controls
        if host:
            ctrl_card = QFrame()
            ctrl_card.setObjectName("Card")
            ctrl_layout = QVBoxLayout(ctrl_card)
            ctrl_layout.setContentsMargins(12, 10, 12, 10)
            ctrl_layout.setSpacing(8)

            self.http_allow_cb = QCheckBox("Bu site için güvensiz HTTP bağlantısına izin ver")
            self.http_allow_cb.setChecked(is_http_allowed)
            self.http_allow_cb.setEnabled(https_only_enabled)
            self.http_allow_cb.toggled.connect(
                lambda checked: self.on_toggle_http_allowed(self.host, checked)
            )
            ctrl_layout.addWidget(self.http_allow_cb)

            self.adblock_wl_cb = QCheckBox("Bu sitede AdBlock korumasını duraklat")
            self.adblock_wl_cb.setChecked(is_adblock_whitelisted)
            self.adblock_wl_cb.toggled.connect(
                lambda checked: self.on_toggle_adblock_whitelist(self.host, checked)
            )
            ctrl_layout.addWidget(self.adblock_wl_cb)
            layout.addWidget(ctrl_card)

        btn_row = QHBoxLayout()
        btn_row.addStretch()
        close_btn = QPushButton("Kapat")
        close_btn.setObjectName("Primary")
        close_btn.clicked.connect(self.accept)
        btn_row.addWidget(close_btn)
        layout.addLayout(btn_row)

    def _on_cert_fetched(self, info: dict) -> None:
        if info.get("ok"):
            self.cert_detail_lbl.setText(
                f"Verilen: {info.get('common_name')}\n"
                f"Sağlayıcı: {info.get('issuer_org')}\n"
                f"Protokol: {info.get('protocol')} ({info.get('cipher')})\n"
                f"Geçerlilik: {info.get('not_before')} -> {info.get('not_after')}"
            )
        else:
            err = info.get("error", "Doğrulanamadı")
            self.cert_detail_lbl.setText(f"Sertifika özeti alınamadı: {err}")

    def closeEvent(self, event) -> None:
        if self._worker and self._worker.isRunning():
            self._worker.wait(500)
        super().closeEvent(event)


def build_dns_blocked_html(target_url: str, host: str, provider_name: str, reason: str) -> str:
    """Renders Chrome-like dark warning page when DoH blocks a malicious/sinkholed host."""
    safe_url = html.escape(target_url)
    safe_host = html.escape(host)
    safe_provider = html.escape(provider_name)
    safe_reason = html.escape(reason)
    override_href = f"adblock://allow-dns?url={quote(target_url, safe='')}"
    return f"""<!DOCTYPE html>
<html lang="tr">
<head>
<meta charset="utf-8">
<title>DNS Koruması Tarafından Engellendi</title>
<style>
  body {{
    background: #202124; color: #e8eaed; margin: 0; padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    display: flex; align-items: center; justify-content: center; min-height: 100vh;
  }}
  .card {{
    max-width: 560px; background: #292a2d; border: 1px solid #3c4043;
    border-radius: 14px; padding: 32px; box-shadow: 0 8px 28px rgba(0,0,0,0.45);
  }}
  .icon {{
    width: 44px; height: 44px; border-radius: 10px;
    background: rgba(242,139,130,0.14); display: flex; align-items: center;
    justify-content: center; margin-bottom: 18px;
  }}
  h1 {{ font-size: 20px; margin: 0 0 10px 0; color: #f28b82; font-weight: 600; }}
  p {{ font-size: 14px; line-height: 1.55; color: #bdc1c6; margin: 0 0 14px 0; }}
  .meta {{
    background: #202124; border: 1px solid #3c4043; border-radius: 8px;
    padding: 12px; font-size: 12.5px; color: #9aa0a6; margin-bottom: 20px;
    word-break: break-all;
  }}
  .actions {{ display: flex; gap: 10px; flex-wrap: wrap; }}
  a.btn {{
    text-decoration: none; padding: 9px 16px; border-radius: 8px;
    font-size: 13px; font-weight: 600; display: inline-block;
  }}
  a.primary {{ background: #8ab4f8; color: #202124; }}
  a.secondary {{ background: #303134; color: #e8eaed; border: 1px solid #3c4043; }}
</style>
</head>
<body>
  <div class="card">
    <div class="icon">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#f28b82" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
        <line x1="12" y1="8" x2="12" y2="12"/>
        <line x1="12" y1="16" x2="12.01" y2="16"/>
      </svg>
    </div>
    <h1>Bu site DNS Koruması tarafından engellendi</h1>
    <p><strong>{safe_host}</strong> alan adı seçili güvenli DNS sağlayıcısı (<strong>{safe_provider}</strong>) tarafından zararlı, kimlik avı veya izleyici olarak işaretlendi.</p>
    <div class="meta">
      <div><strong>Adres:</strong> {safe_url}</div>
      <div><strong>Neden:</strong> {safe_reason}</div>
    </div>
    <div class="actions">
      <a class="btn primary" href="adblock://home">Güvenli Sayfaya Dön</a>
      <a class="btn secondary" href="{override_href}">Riski Kabul Et ve Yine de Aç</a>
    </div>
  </div>
</body>
</html>"""


def build_cert_error_html(target_url: str, host: str, description: str, is_overridable: bool) -> str:
    """Renders Chrome-like dark TLS certificate error page."""
    safe_url = html.escape(target_url)
    safe_host = html.escape(host)
    safe_desc = html.escape(description)
    override_btn = ""
    if is_overridable:
        override_href = f"adblock://accept-cert?url={quote(target_url, safe='')}"
        override_btn = f'<a class="btn secondary" href="{override_href}">Güvensiz Olduğunu Biliyorum, Devam Et</a>'
    return f"""<!DOCTYPE html>
<html lang="tr">
<head>
<meta charset="utf-8">
<title>Bağlantınız Gizli Değil</title>
<style>
  body {{
    background: #202124; color: #e8eaed; margin: 0; padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    display: flex; align-items: center; justify-content: center; min-height: 100vh;
  }}
  .card {{
    max-width: 560px; background: #292a2d; border: 1px solid #3c4043;
    border-radius: 14px; padding: 32px; box-shadow: 0 8px 28px rgba(0,0,0,0.45);
  }}
  .icon {{
    width: 44px; height: 44px; border-radius: 10px;
    background: rgba(253,214,99,0.14); display: flex; align-items: center;
    justify-content: center; margin-bottom: 18px;
  }}
  h1 {{ font-size: 20px; margin: 0 0 10px 0; color: #fdd663; font-weight: 600; }}
  p {{ font-size: 14px; line-height: 1.55; color: #bdc1c6; margin: 0 0 14px 0; }}
  .meta {{
    background: #202124; border: 1px solid #3c4043; border-radius: 8px;
    padding: 12px; font-size: 12.5px; color: #9aa0a6; margin-bottom: 20px;
    word-break: break-all;
  }}
  .actions {{ display: flex; gap: 10px; flex-wrap: wrap; }}
  a.btn {{
    text-decoration: none; padding: 9px 16px; border-radius: 8px;
    font-size: 13px; font-weight: 600; display: inline-block;
  }}
  a.primary {{ background: #8ab4f8; color: #202124; }}
  a.secondary {{ background: #303134; color: #e8eaed; border: 1px solid #3c4043; }}
</style>
</head>
<body>
  <div class="card">
    <div class="icon">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#fdd663" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>
        <line x1="12" y1="9" x2="12" y2="13"/>
        <line x1="12" y1="17" x2="12.01" y2="17"/>
      </svg>
    </div>
    <h1>Bağlantınız gizli değil (SSL/TLS Hatası)</h1>
    <p><strong>{safe_host}</strong> sunucusunun güvenlik sertifikası doğrulanamadı. Saldırganlar bilgilerinizi çalmaya çalışıyor olabilir.</p>
    <div class="meta">
      <div><strong>Adres:</strong> {safe_url}</div>
      <div><strong>Sertifika Hatası:</strong> {safe_desc}</div>
    </div>
    <div class="actions">
      <a class="btn primary" href="adblock://home">Güvenli Sayfaya Dön</a>
      {override_btn}
    </div>
  </div>
</body>
</html>"""


def build_https_failed_html(https_url: str, http_url: str, host: str) -> str:
    """Renders Chrome-like HTTPS-Only fallback page when a site fails to load over HTTPS."""
    safe_https = html.escape(https_url)
    safe_host = html.escape(host)
    allow_http_href = f"adblock://allow-http?url={quote(http_url, safe='')}"
    return f"""<!DOCTYPE html>
<html lang="tr">
<head>
<meta charset="utf-8">
<title>HTTPS Bağlantısı Kurulamadı</title>
<style>
  body {{
    background: #202124; color: #e8eaed; margin: 0; padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    display: flex; align-items: center; justify-content: center; min-height: 100vh;
  }}
  .card {{
    max-width: 560px; background: #292a2d; border: 1px solid #3c4043;
    border-radius: 14px; padding: 32px; box-shadow: 0 8px 28px rgba(0,0,0,0.45);
  }}
  h1 {{ font-size: 20px; margin: 0 0 10px 0; color: #8ab4f8; font-weight: 600; }}
  p {{ font-size: 14px; line-height: 1.55; color: #bdc1c6; margin: 0 0 14px 0; }}
  .meta {{
    background: #202124; border: 1px solid #3c4043; border-radius: 8px;
    padding: 12px; font-size: 12.5px; color: #9aa0a6; margin-bottom: 20px;
    word-break: break-all;
  }}
  .actions {{ display: flex; gap: 10px; flex-wrap: wrap; }}
  a.btn {{
    text-decoration: none; padding: 9px 16px; border-radius: 8px;
    font-size: 13px; font-weight: 600; display: inline-block;
  }}
  a.primary {{ background: #8ab4f8; color: #202124; }}
  a.secondary {{ background: #303134; color: #e8eaed; border: 1px solid #3c4043; }}
</style>
</head>
<body>
  <div class="card">
    <h1>HTTPS-Only Modu: Güvenli bağlantı kurulamadı</h1>
    <p><strong>{safe_host}</strong> HTTPS üzerinden yüklenemedi. Site HTTPS desteklemiyor veya sunucu yanıt vermiyor olabilir.</p>
    <div class="meta">Denenen adres: {safe_https}</div>
    <div class="actions">
      <a class="btn primary" href="adblock://home">Güvenli Sayfaya Dön</a>
      <a class="btn secondary" href="{allow_http_href}">Şifrelenmemiş HTTP Olarak Yükle</a>
    </div>
  </div>
</body>
</html>"""
