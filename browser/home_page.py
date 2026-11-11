"""Built-in Chrome-inspired New Tab / Start Page HTML generator with inline vector SVGs."""

from __future__ import annotations

import html
from typing import List

from .storage import BookmarksManager, SEARCH_ENGINES, SettingsManager

_SHIELD_SVG = """<svg xmlns="http://www.w3.org/2000/svg" width="38" height="38" viewBox="0 0 24 24" fill="none">
  <path d="M12 2.2l8 3.4v6.2c0 5.3-3.4 9.7-8 11.2-4.6-1.5-8-5.9-8-11.2V5.6l8-3.4z"
        fill="rgba(138, 180, 248, 0.14)" stroke="#8ab4f8" stroke-width="1.8" stroke-linejoin="round"/>
  <path d="M9 12.2l2.2 2.2 4-4.4" stroke="#81c995" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"""

_SEARCH_SVG = """<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none">
  <circle cx="11" cy="11" r="6.8" stroke="#9aa0a6" stroke-width="2"/>
  <path d="M16.2 16.2L20.5 20.5" stroke="#9aa0a6" stroke-width="2" stroke-linecap="round"/>
</svg>"""


def render_home_html(
    total_blocked: int,
    rules_count: int,
    adblock_enabled: bool,
    settings: SettingsManager,
    bookmarks: BookmarksManager,
) -> str:
    """Return HTML for the Chrome-inspired built-in New Tab page."""
    engine_key = settings.get("search_engine", "google")
    engine_name, engine_template = SEARCH_ENGINES.get(engine_key, SEARCH_ENGINES["google"])
    search_action = engine_template.split("?")[0]

    status_badge = (
        '<span class="badge active"><span class="dot"></span>Kalkan Aktif</span>'
        if adblock_enabled
        else '<span class="badge paused"><span class="dot"></span>Koruma Duraklatıldı</span>'
    )

    cards_html: List[str] = []
    for item in bookmarks.items[:10]:
        safe_title = html.escape(item.title[:22] or item.url)
        safe_url = html.escape(item.url)
        initial = html.escape((item.title[:1] or "W").upper())
        cards_html.append(
            f'<a class="bm-tile" href="{safe_url}">'
            f'<div class="bm-circle">{initial}</div>'
            f'<div class="bm-title">{safe_title}</div>'
            f"</a>"
        )

    bookmarks_block = "".join(cards_html) or '<p class="empty-bm">Henüz yer imi yok.</p>'

    return f"""<!DOCTYPE html>
<html lang="tr">
<head>
<meta charset="utf-8">
<title>Yeni Sekme</title>
<style>
  :root {{
    color-scheme: dark;
    --bg: #202124;
    --surface: #292a2d;
    --surface-hover: #35363a;
    --border: #3c4043;
    --text: #e8eaed;
    --muted: #9aa0a6;
    --blue: #8ab4f8;
    --green: #81c995;
    --red: #f28b82;
  }}
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{
    background: var(--bg);
    color: var(--text);
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", Roboto, sans-serif;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 36px 20px;
  }}
  .container {{
    width: 100%;
    max-width: 680px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 26px;
  }}
  .hero {{
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }}
  .brand {{
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 28px;
    font-weight: 600;
    letter-spacing: -0.4px;
  }}
  .brand span.accent {{
    color: var(--blue);
  }}
  .badge {{
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
    font-weight: 500;
    padding: 4px 12px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--surface);
  }}
  .badge .dot {{
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }}
  .badge.active {{ color: var(--green); border-color: rgba(129, 201, 149, 0.35); }}
  .badge.active .dot {{ background: var(--green); }}
  .badge.paused {{ color: var(--red); border-color: rgba(242, 139, 130, 0.35); }}
  .badge.paused .dot {{ background: var(--red); }}

  /* Chrome Omnibox-style Pill Search */
  .search-form {{
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 26px;
    padding: 6px 8px 6px 18px;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.28);
    transition: border-color 0.15s, background 0.15s;
  }}
  .search-form:hover {{
    background: #303134;
  }}
  .search-form:focus-within {{
    border-color: var(--blue);
    background: #303134;
  }}
  .search-icon {{
    display: flex;
    align-items: center;
    opacity: 0.9;
  }}
  .search-input {{
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text);
    font-size: 15px;
    padding: 9px 0;
  }}
  .search-input::placeholder {{
    color: var(--muted);
  }}
  .search-btn {{
    background: var(--blue);
    color: #202124;
    border: none;
    border-radius: 20px;
    padding: 9px 18px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }}
  .search-btn:hover {{
    filter: brightness(1.06);
  }}

  /* Compact Telemetry Strip */
  .stats {{
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 10px;
    width: 100%;
  }}
  .stat-pill {{
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 10px 18px;
    display: flex;
    align-items: baseline;
    gap: 8px;
  }}
  .stat-val {{
    font-size: 16px;
    font-weight: 600;
    color: var(--green);
  }}
  .stat-val.blue {{ color: var(--blue); }}
  .stat-label {{
    font-size: 12px;
    color: var(--muted);
  }}

  /* Chrome Shortcut Grid */
  .bm-grid {{
    width: 100%;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(104px, 1fr));
    gap: 8px;
    margin-top: 4px;
  }}
  .bm-tile {{
    text-decoration: none;
    color: var(--text);
    border-radius: 12px;
    padding: 14px 8px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    transition: background 0.15s;
  }}
  .bm-tile:hover {{
    background: var(--surface-hover);
  }}
  .bm-circle {{
    width: 46px;
    height: 46px;
    border-radius: 50%;
    background: #303134;
    border: 1px solid var(--border);
    color: var(--blue);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 18px;
    font-weight: 600;
  }}
  .bm-title {{
    font-size: 12px;
    color: var(--text);
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 96px;
  }}
  .empty-bm {{ color: var(--muted); font-size: 13px; }}
</style>
</head>
<body>
  <div class="container">
    <div class="hero">
      <div class="brand">
        {_SHIELD_SVG}
        <span>AdBlock <span class="accent">Tarayıcı</span></span>
      </div>
      {status_badge}
    </div>

    <form class="search-form" action="{html.escape(search_action)}" method="GET">
      <span class="search-icon">{_SEARCH_SVG}</span>
      <input class="search-input" type="text" name="q" placeholder="{html.escape(engine_name)} ile ara veya adres çubuğuna URL yaz" autofocus />
      <button class="search-btn" type="submit">Ara</button>
    </form>

    <div class="stats">
      <div class="stat-pill">
        <span class="stat-val">{total_blocked:,}</span>
        <span class="stat-label">Engellenen Reklam</span>
      </div>
      <div class="stat-pill">
        <span class="stat-val blue">{rules_count:,}</span>
        <span class="stat-label">Aktif Kural</span>
      </div>
      <div class="stat-pill">
        <span class="stat-val blue">Ağ + CSS</span>
        <span class="stat-label">Çift Katmanlı Koruma</span>
      </div>
    </div>

    <div class="bm-grid">{bookmarks_block}</div>
  </div>
</body>
</html>
"""
