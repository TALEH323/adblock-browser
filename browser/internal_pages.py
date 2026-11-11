"""Chrome-inspired in-tab HTML renderers for adblock://settings, downloads, passwords, history, bookmarks."""

from __future__ import annotations

import html
from typing import List

from adblock.default_filters import BUILTIN_FILTER_LISTS
from adblock.dns_guard import DOH_PROVIDERS
from adblock.engine import AdBlockEngine, extract_host
from .downloads import DownloadsManager, format_bytes
from .passwords import PasswordManager
from .storage import SEARCH_ENGINES, BookmarksManager, HistoryManager, SettingsManager

_NAV_ITEMS = (
    ("settings", "Ayarlar", "adblock://settings"),
    ("adblock", "AdBlock Yönetimi", "adblock://adblock"),
    ("passwords", "Şifre Yöneticisi", "adblock://passwords"),
    ("downloads", "İndirilenler", "adblock://downloads"),
    ("history", "Geçmiş", "adblock://history"),
    ("bookmarks", "Yer İmleri", "adblock://bookmarks"),
)

_COMMON_CSS = """
  :root {
    color-scheme: dark;
    --bg: #202124;
    --surface: #292a2d;
    --surface2: #303134;
    --border: #3c4043;
    --text: #e8eaed;
    --muted: #9aa0a6;
    --blue: #8ab4f8;
    --green: #81c995;
    --red: #f28b82;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; background: var(--bg); color: var(--text);
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", Roboto, sans-serif;
    display: flex; min-height: 100vh;
  }
  .sidebar {
    width: 240px; flex-shrink: 0; padding: 24px 12px;
    border-right: 1px solid var(--border); background: #1c1d1f;
    display: flex; flex-direction: column; gap: 4px;
  }
  .brand {
    font-size: 16px; font-weight: 600; color: var(--text);
    padding: 6px 14px 16px 14px; display: flex; align-items: center; gap: 10px;
  }
  .nav-link {
    text-decoration: none; color: #bdc1c6; padding: 10px 14px;
    border-radius: 20px; font-size: 13.5px; font-weight: 500;
    transition: background 0.15s;
  }
  .nav-link:hover { background: #2c2d30; color: var(--text); }
  .nav-link.active {
    background: rgba(138, 180, 248, 0.16); color: var(--blue); font-weight: 600;
  }
  .main {
    flex: 1; padding: 28px 40px; max-width: 880px;
  }
  .header-row {
    display: flex; align-items: center; justify-content: space-between;
    gap: 16px; margin-bottom: 22px; flex-wrap: wrap;
  }
  h1 { margin: 0; font-size: 22px; font-weight: 600; }
  .search-input, input[type="text"], select {
    background: var(--surface); color: var(--text);
    border: 1px solid var(--border); border-radius: 10px;
    padding: 8px 12px; font-size: 13.5px; outline: none;
  }
  .search-input { width: 280px; border-radius: 20px; padding: 8px 16px; }
  input[type="text"]:focus, select:focus, .search-input:focus {
    border-color: var(--blue);
  }
  .card {
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 12px; overflow: hidden; margin-bottom: 18px;
  }
  .card-title {
    font-size: 13px; font-weight: 600; color: var(--blue);
    padding: 12px 18px; border-bottom: 1px solid var(--border);
    background: rgba(255,255,255,0.02);
  }
  .row {
    display: flex; align-items: center; justify-content: space-between;
    gap: 16px; padding: 13px 18px; border-bottom: 1px solid #303134;
  }
  .row:last-child { border-bottom: none; }
  .row-info { min-width: 0; flex: 1; }
  .row-title {
    font-size: 14px; font-weight: 500; color: var(--text);
    word-break: break-word;
  }
  .row-sub {
    font-size: 12px; color: var(--muted); margin-top: 2px;
    word-break: break-all;
  }
  .btn, button {
    background: var(--surface2); color: var(--text);
    border: 1px solid var(--border); border-radius: 8px;
    padding: 7px 14px; font-size: 12.5px; font-weight: 500;
    cursor: pointer; text-decoration: none; display: inline-block;
  }
  .btn:hover, button:hover { background: #3c4043; }
  .btn-primary {
    background: var(--blue); color: #202124; border: none; font-weight: 600;
  }
  .btn-primary:hover { background: #aecbfa; }
  .btn-danger {
    background: rgba(242, 139, 130, 0.14); color: var(--red);
    border: 1px solid rgba(242, 139, 130, 0.35);
  }
  .btn-danger:hover { background: rgba(242, 139, 130, 0.25); }
  .actions { display: flex; gap: 8px; flex-shrink: 0; align-items: center; }
  .empty {
    padding: 36px 20px; text-align: center; color: var(--muted); font-size: 13.5px;
  }
  label.toggle {
    display: flex; align-items: center; gap: 10px; cursor: pointer;
  }
  input[type="checkbox"] {
    width: 17px; height: 17px; accent-color: var(--blue); cursor: pointer;
  }
  a.ext-link { color: var(--blue); text-decoration: none; }
  a.ext-link:hover { text-decoration: underline; }
"""


def _wrap_shell(active_key: str, page_title: str, body_html: str) -> str:
    links = []
    for key, label, href in _NAV_ITEMS:
        cls = "nav-link active" if key == active_key else "nav-link"
        links.append(f'<a class="{cls}" href="{href}">{html.escape(label)}</a>')
    nav_html = "\n".join(links)
    return f"""<!DOCTYPE html>
<html lang="tr">
<head>
<meta charset="utf-8">
<title>{html.escape(page_title)}</title>
<style>{_COMMON_CSS}</style>
</head>
<body>
  <aside class="sidebar">
    <div class="brand">
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none">
        <path d="M12 2.5l7.5 3.2v5.8c0 4.9-3.2 9.1-7.5 10.5-4.3-1.4-7.5-5.6-7.5-10.5V5.7z"
              stroke="#8ab4f8" stroke-width="2" stroke-linejoin="round"/>
        <path d="M9.2 12.2l2 2 3.8-4.1" stroke="#81c995" stroke-width="2" stroke-linecap="round"/>
      </svg>
      <span>AdBlock Tarayıcı</span>
    </div>
    {nav_html}
  </aside>
  <main class="main">
    {body_html}
  </main>
  <script>
    function filterRows(q) {{
      var term = (q || '').toLowerCase();
      var rows = document.querySelectorAll('[data-filter-item]');
      for (var i = 0; i < rows.length; i++) {{
        var txt = (rows[i].getAttribute('data-search') || '').toLowerCase();
        rows[i].style.display = txt.indexOf(term) !== -1 ? 'flex' : 'none';
      }}
    }}
  </script>
</body>
</html>"""


def render_settings_page(settings: SettingsManager, engine: AdBlockEngine) -> str:
    cur_engine = settings.get("search_engine", "google")
    engine_opts = "\n".join(
        f'<option value="{html.escape(k)}" {"selected" if k == cur_engine else ""}>{html.escape(name)}</option>'
        for k, (name, _) in SEARCH_ENGINES.items()
    )

    cur_doh = settings.get("doh_provider", "cloudflare_security")
    doh_opts = "\n".join(
        f'<option value="{html.escape(k)}" {"selected" if k == cur_doh else ""}>{html.escape(name)}</option>'
        for k, (name, _) in DOH_PROVIDERS.items()
    )

    hp = html.escape(str(settings.get("homepage", "adblock://home")))
    https_chk = "checked" if bool(settings.get("https_only_enabled", True)) else ""
    ab_chk = "checked" if engine.enabled else ""
    cos_chk = "checked" if engine.cosmetic_enabled else ""
    bm_chk = "checked" if bool(settings.get("show_bookmarks_bar", True)) else ""
    pw_chk = "checked" if bool(settings.get("password_manager_enabled", True)) else ""

    body = f"""
    <div class="header-row">
      <h1>Tarayıcı Ayarları</h1>
    </div>
    <form action="adblock-cmd://save-settings" method="get">
      <div class="card">
        <div class="card-title">Güvenlik, HTTPS ve DNS Koruması</div>
        <div class="row">
          <div class="row-info">
            <div class="row-title">Güvenli DNS (DNS-over-HTTPS)</div>
            <div class="row-sub">Zararlı alan adlarını ve izleyicileri DNS seviyesinde kontrol eder</div>
          </div>
          <select name="doh_provider">{doh_opts}</select>
        </div>
        <div class="row">
          <div class="row-info">
            <div class="row-title">HTTPS-Only Modu</div>
            <div class="row-sub">HTTP siteleri otomatik olarak şifreli HTTPS bağlantısına yükseltir</div>
          </div>
          <input type="checkbox" name="https_only_enabled" value="1" {https_chk}>
        </div>
        <div class="row">
          <div class="row-info">
            <div class="row-title">AdBlock Ağ Koruması</div>
            <div class="row-sub">Reklam ve izleyici isteklerini yüklenmeden engeller</div>
          </div>
          <input type="checkbox" name="adblock_enabled" value="1" {ab_chk}>
        </div>
        <div class="row">
          <div class="row-info">
            <div class="row-title">Kozmetik & YouTube Reklam Filtreleme</div>
            <div class="row-sub">Sayfa içi reklam boşluklarını gizler</div>
          </div>
          <input type="checkbox" name="cosmetic_enabled" value="1" {cos_chk}>
        </div>
      </div>

      <div class="card">
        <div class="card-title">Arama, Görünüm ve Şifreler</div>
        <div class="row">
          <div class="row-info">
            <div class="row-title">Varsayılan Arama Motoru</div>
          </div>
          <select name="search_engine">{engine_opts}</select>
        </div>
        <div class="row">
          <div class="row-info">
            <div class="row-title">Ana Sayfa Adresi</div>
          </div>
          <input type="text" name="homepage" value="{hp}" style="width: 260px;">
        </div>
        <div class="row">
          <div class="row-info">
            <div class="row-title">Yer İmleri Çubuğunu Göster</div>
          </div>
          <input type="checkbox" name="show_bookmarks_bar" value="1" {bm_chk}>
        </div>
        <div class="row">
          <div class="row-info">
            <div class="row-title">Şifre Kaydetme ve Otomatik Doldurma</div>
          </div>
          <input type="checkbox" name="password_manager_enabled" value="1" {pw_chk}>
        </div>
      </div>

      <div class="actions" style="justify-content: flex-end;">
        <button type="submit" class="btn-primary">Ayarları Kaydet</button>
      </div>
    </form>
    """
    return _wrap_shell("settings", "Ayarlar — AdBlock Tarayıcı", body)


def render_downloads_page(manager: DownloadsManager) -> str:
    rows: List[str] = []
    for idx, rec in enumerate(manager.records):
        if rec.total_bytes > 0:
            sz = f"{format_bytes(rec.received_bytes)} / {format_bytes(rec.total_bytes)}"
        else:
            sz = format_bytes(rec.received_bytes)
        btns = (
            f'<a class="btn" href="adblock-cmd://dl-reveal?idx={idx}">Klasörde Göster</a>'
            f'<a class="btn btn-primary" href="adblock-cmd://dl-open?idx={idx}">Aç</a>'
        )
        if not rec.is_finished:
            btns += f'<a class="btn btn-danger" href="adblock-cmd://dl-cancel?idx={idx}">İptal</a>'
        rows.append(
            f'<div class="row" data-filter-item="1" data-search="{html.escape(rec.file_name + " " + rec.url)}">'
            f'<div class="row-info">'
            f'<div class="row-title">{html.escape(rec.file_name)}</div>'
            f'<div class="row-sub">{html.escape(sz)} • {html.escape(rec.state_label)} • {html.escape(rec.url[:90])}</div>'
            f"</div>"
            f'<div class="actions">{btns}</div>'
            f"</div>"
        )
    content = "".join(rows) or '<div class="empty">Henüz indirilen dosya yok.</div>'
    body = f"""
    <div class="header-row">
      <h1>İndirilenler</h1>
      <div class="actions">
        <input class="search-input" type="text" placeholder="İndirilenlerde ara..." oninput="filterRows(this.value)">
        <a class="btn" href="adblock-cmd://dl-clear">Tamamlananları Temizle</a>
      </div>
    </div>
    <div class="card">{content}</div>
    """
    return _wrap_shell("downloads", "İndirilenler — AdBlock Tarayıcı", body)


def render_passwords_page(pw_manager: PasswordManager) -> str:
    rows: List[str] = []
    for idx, cred in enumerate(pw_manager.entries):
        host = extract_host(cred.origin) or cred.origin
        host_disp = html.escape(host)
        user_disp = html.escape(cred.username or "(Kullanıcı adı yok)")
        pass_esc = html.escape(cred.password)
        rows.append(
            f'<div class="row" data-filter-item="1" data-search="{html.escape(host + " " + cred.username)}">'
            f'<div class="row-info">'
            f'<div class="row-title">{host_disp}</div>'
            f'<div class="row-sub">Kullanıcı: {user_disp} &nbsp;|&nbsp; '
            f'Şifre: <span id="pw-{idx}" data-shown="0" data-val="{pass_esc}">••••••••</span></div>'
            f"</div>"
            f'<div class="actions">'
            f'<button type="button" onclick="togglePw({idx})">Göster/Gizle</button>'
            f'<a class="btn" href="adblock-cmd://pw-copy?idx={idx}">Kopyala</a>'
            f'<a class="btn btn-danger" href="adblock-cmd://pw-del?idx={idx}">Sil</a>'
            f"</div>"
            f"</div>"
        )
    content = "".join(rows) or '<div class="empty">Kayıtlı şifre bulunmuyor.</div>'
    body = f"""
    <div class="header-row">
      <h1>Şifre Yöneticisi</h1>
      <input class="search-input" type="text" placeholder="Site veya kullanıcı adı ara..." oninput="filterRows(this.value)">
    </div>
    <div class="card">{content}</div>
    <script>
      function togglePw(idx) {{
        var el = document.getElementById('pw-' + idx);
        if (!el) return;
        if (el.getAttribute('data-shown') === '1') {{
          el.textContent = '••••••••';
          el.setAttribute('data-shown', '0');
        }} else {{
          el.textContent = el.getAttribute('data-val') || '';
          el.setAttribute('data-shown', '1');
        }}
      }}
    </script>
    """
    return _wrap_shell("passwords", "Şifre Yöneticisi — AdBlock Tarayıcı", body)


def render_history_page(history: HistoryManager) -> str:
    rows: List[str] = []
    for entry in history.items[:300]:
        safe_title = html.escape(entry.title or entry.url)
        safe_url = html.escape(entry.url)
        ts = html.escape(entry.visited_at.replace("T", " ")[:16])
        rows.append(
            f'<div class="row" data-filter-item="1" data-search="{html.escape((entry.title or "") + " " + entry.url)}">'
            f'<div class="row-info">'
            f'<div class="row-title"><a class="ext-link" href="{safe_url}">{safe_title}</a></div>'
            f'<div class="row-sub">{safe_url} • {ts}</div>'
            f"</div>"
            f"</div>"
        )
    content = "".join(rows) or '<div class="empty">Tarama geçmişi boş.</div>'
    body = f"""
    <div class="header-row">
      <h1>Geçmiş</h1>
      <div class="actions">
        <input class="search-input" type="text" placeholder="Geçmişte ara..." oninput="filterRows(this.value)">
        <a class="btn btn-danger" href="adblock-cmd://hist-clear">Tüm Geçmişi Temizle</a>
      </div>
    </div>
    <div class="card">{content}</div>
    """
    return _wrap_shell("history", "Geçmiş — AdBlock Tarayıcı", body)


def render_bookmarks_page(bookmarks: BookmarksManager) -> str:
    rows: List[str] = []
    for idx, bm in enumerate(bookmarks.items):
        safe_title = html.escape(bm.title or bm.url)
        safe_url = html.escape(bm.url)
        rows.append(
            f'<div class="row" data-filter-item="1" data-search="{html.escape((bm.title or "") + " " + bm.url)}">'
            f'<div class="row-info">'
            f'<div class="row-title"><a class="ext-link" href="{safe_url}">{safe_title}</a></div>'
            f'<div class="row-sub">{safe_url}</div>'
            f"</div>"
            f'<div class="actions"><a class="btn btn-danger" href="adblock-cmd://bm-del?idx={idx}">Kaldır</a></div>'
            f"</div>"
        )
    content = "".join(rows) or '<div class="empty">Henüz kayıtlı yer imi yok.</div>'
    body = f"""
    <div class="header-row">
      <h1>Yer İmleri</h1>
      <input class="search-input" type="text" placeholder="Yer imlerinde ara..." oninput="filterRows(this.value)">
    </div>
    <div class="card">
      <div class="card-title">Yeni Yer İmi Ekle</div>
      <form class="row" action="adblock-cmd://bm-add" method="get">
        <input type="text" name="title" placeholder="Başlık" style="width: 210px;">
        <input type="text" name="url" placeholder="https://..." style="flex: 1;">
        <button type="submit" class="btn-primary">Ekle</button>
      </form>
    </div>
    <div class="card">{content}</div>
    """
    return _wrap_shell("bookmarks", "Yer İmleri — AdBlock Tarayıcı", body)


def render_adblock_page(engine: AdBlockEngine) -> str:
    wl_rows = "".join(
        f'<div class="row">'
        f'<div class="row-info"><div class="row-title">{html.escape(d)}</div></div>'
        f'<div class="actions"><a class="btn btn-danger" href="adblock-cmd://ab-wl-del?host={html.escape(d)}">Kaldır</a></div>'
        f"</div>"
        for d in sorted(engine.whitelisted_domains)
    ) or '<div class="empty">Beyaz listede domain bulunmuyor.</div>'

    list_rows_parts: List[str] = []
    for item in BUILTIN_FILTER_LISTS:
        cached_path = engine.filter_dir / f"{item['id']}.txt" if engine.filter_dir else None
        status_txt = "İndirildi" if (cached_path and cached_path.exists()) else "Dahili/Hazır"
        status_col = "#81c995" if (cached_path and cached_path.exists()) else "#9aa0a6"
        list_rows_parts.append(
            f'<div class="row">'
            f'<div class="row-info">'
            f'<div class="row-title">{html.escape(item["name"])}</div>'
            f'<div class="row-sub">{html.escape(item["url"])}</div>'
            f"</div>"
            f'<div class="actions"><span style="color:{status_col};font-size:12.5px;font-weight:600;">{status_txt}</span></div>'
            f"</div>"
        )
    list_rows = "".join(list_rows_parts)

    rules_text = html.escape(engine.custom_rules_text or "")
    body = f"""
    <div class="header-row">
      <h1>AdBlock Kalkanı ve Filtre Yönetimi</h1>
      <a class="btn btn-primary" href="adblock-cmd://ab-update-lists">Filtre Listelerini Güncelle</a>
    </div>

    <div class="card">
      <div class="card-title">Özel Filtre Kuralları (custom_filters.txt)</div>
      <form action="adblock-cmd://ab-save-rules" method="get" style="padding: 14px 18px; display: flex; flex-direction: column; gap: 12px;">
        <textarea name="rules" rows="7" style="width:100%; background:#202124; color:#e8eaed; border:1px solid #3c4043; border-radius:10px; padding:10px; font-family:monospace; font-size:12.5px;">{rules_text}</textarea>
        <div class="actions" style="justify-content: flex-end;">
          <button type="submit" class="btn-primary">Özel Kuralları Kaydet</button>
        </div>
      </form>
    </div>

    <div class="card">
      <div class="card-title">Beyaz Liste (Reklam Engelleme Duraklatılan Siteler)</div>
      <form class="row" action="adblock-cmd://ab-wl-add" method="get">
        <input type="text" name="host" placeholder="Örnek: youtube.com" style="flex: 1;">
        <button type="submit" class="btn-primary">Beyaz Listeye Ekle</button>
      </form>
      {wl_rows}
    </div>

    <div class="card">
      <div class="card-title">Dahili Filtre Listeleri ({engine.total_rules_count:,} aktif kural)</div>
      {list_rows}
    </div>
    """
    return _wrap_shell("adblock", "AdBlock Yönetimi — AdBlock Tarayıcı", body)
