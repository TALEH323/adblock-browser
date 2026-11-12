use crate::adblock_engine::{AdBlockService, BlockedItem};
use crate::dns_guard::DnsCheckResult;
use crate::passwords::SavedCredential;
use crate::storage::{BookmarkEntry, BrowserStorage, DownloadEntry};
use html_escape::{encode_double_quoted_attribute as attr, encode_text as txt};
use serde::Serialize;

#[derive(Serialize)]
pub struct TabUiItem {
    pub id: usize,
    pub title: String,
    pub active: bool,
    pub blocked_count: usize,
    pub favicon: String,
    pub host: String,
    pub is_internal: bool,
}

#[derive(Serialize)]
pub struct BarUiState {
    pub tabs: Vec<TabUiItem>,
    pub url: String,
    pub host: String,
    pub is_secure: bool,
    pub is_internal: bool,
    pub is_bookmarked: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub adblock_enabled: bool,
    pub site_whitelisted: bool,
    pub tab_blocked: usize,
    pub show_bookmarks_bar: bool,
    pub bookmarks: Vec<BookmarkEntry>,
    pub has_saved_pw: bool,
    pub side_panel_open: bool,
}

pub fn chrome_bar_html() -> &'static str {
    r###"<!DOCTYPE html>
<html lang="tr">
<head>
<meta charset="utf-8">
<style>
  :root {
    color-scheme: dark;
    --bg: #202124;
    --surface: #292a2d;
    --bar: #35363a;
    --omni: #202124;
    --border: #3c4043;
    --text: #e8eaed;
    --sub: #9aa0a6;
    --blue: #8ab4f8;
    --green: #81c995;
    --red: #f28b82;
  }
  * { box-sizing: border-box; user-select: none; }
  html, body {
    margin: 0; padding: 0;
    background: var(--bg);
    color: var(--text);
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    overflow: hidden;
  }
  .tabs-strip {
    height: 36px;
    display: flex;
    align-items: flex-end;
    padding: 4px 10px 0 10px;
    gap: 4px;
    background: #17181a;
  }
  .tabs-list {
    display: flex; align-items: flex-end; gap: 3px; flex: 0 1 auto; max-width: calc(100% - 34px); min-width: 0; overflow-x: auto;
  }
  .tabs-list::-webkit-scrollbar { display: none; }
  .tab {
    height: 31px; width: 195px; min-width: 90px; max-width: 215px; flex: 0 1 auto;
    display: flex; align-items: center; gap: 7px;
    padding: 0 10px; border-radius: 9px 9px 0 0;
    background: transparent; color: var(--sub);
    font-size: 12px; cursor: pointer; border: none;
    transition: background 0.12s;
  }
  .tab:hover { background: rgba(255,255,255,0.06); color: var(--text); }
  .tab.active { background: var(--bar); color: var(--text); font-weight: 500; }
  .tab-icon {
    width: 16px; height: 16px; border-radius: 3px; flex-shrink: 0;
    display: flex; align-items: center; justify-content: center; overflow: hidden;
  }
  .tab-icon img {
    width: 16px; height: 16px; object-fit: contain; display: block; border-radius: 3px;
  }
  .tab-title { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }
  .tab-close {
    width: 17px; height: 17px; border-radius: 50%; border: none; background: transparent;
    color: var(--sub); display: flex; align-items: center; justify-content: center; cursor: pointer;
    font-size: 13px; padding: 0;
  }
  .tab-close:hover { background: rgba(255,255,255,0.14); color: var(--text); }
  .new-tab-btn {
    width: 28px; height: 28px; border-radius: 50%; border: none; flex-shrink: 0;
    background: transparent; color: var(--sub); cursor: pointer;
    display: flex; align-items: center; justify-content: center; margin-bottom: 2px;
  }
  .new-tab-btn:hover { background: rgba(255,255,255,0.1); color: var(--text); }

  .nav-bar {
    height: 44px; background: var(--bar);
    display: flex; align-items: center; gap: 6px; padding: 0 10px;
    border-bottom: 1px solid #2b2c30;
  }
  .icon-btn {
    width: 30px; height: 30px; border-radius: 50%; border: none;
    background: transparent; color: #bdc1c6; cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    position: relative; flex-shrink: 0;
  }
  .icon-btn:hover:not(:disabled) { background: rgba(255,255,255,0.1); color: var(--text); }
  .icon-btn:disabled { opacity: 0.35; cursor: default; }
  .icon-btn.active { background: rgba(138,180,248,0.18); color: var(--blue); }

  .omnibox {
    flex: 1; height: 32px; background: var(--omni);
    border: 1px solid #444746; border-radius: 18px;
    display: flex; align-items: center; padding: 0 8px 0 6px; gap: 6px;
  }
  .omnibox:focus-within { border-color: var(--blue); }
  .sec-pill {
    height: 24px; padding: 0 8px; border-radius: 12px; border: none;
    background: rgba(255,255,255,0.06); color: var(--text);
    display: flex; align-items: center; gap: 5px; cursor: pointer; font-size: 11.5px;
  }
  .sec-pill:hover { background: rgba(255,255,255,0.12); }
  #url-input {
    flex: 1; background: transparent; border: none; outline: none;
    color: var(--text); font-size: 13px; user-select: text;
  }
  .shield-pill {
    height: 28px; padding: 0 9px; border-radius: 14px; border: 1px solid #444746;
    background: #292a2d; color: var(--text);
    display: flex; align-items: center; gap: 5px; cursor: pointer; font-size: 11.5px; font-weight: 600;
  }
  .shield-pill:hover { background: #3c4043; }
  .badge {
    background: #81c995; color: #202124; border-radius: 9px; padding: 0 5px;
    font-size: 10.5px; font-weight: 700; min-width: 16px; text-align: center;
  }
  .badge.off { background: #5f6368; color: #e8eaed; }

  .bm-bar {
    height: 30px; background: var(--bar);
    border-bottom: 1px solid var(--border);
    display: flex; align-items: center; gap: 4px; padding: 0 10px;
    overflow-x: auto;
  }
  .bm-bar.hidden { display: none; }
  .bm-bar::-webkit-scrollbar { display: none; }
  .bm-item {
    height: 22px; padding: 0 9px; border-radius: 11px; border: none;
    background: transparent; color: #bdc1c6; font-size: 11.5px;
    display: flex; align-items: center; gap: 5px; cursor: pointer; white-space: nowrap;
  }
  .bm-item:hover { background: rgba(255,255,255,0.1); color: var(--text); }
</style>
</head>
<body>
  <div class="tabs-strip">
    <div class="tabs-list" id="tabs-list"></div>
    <button class="new-tab-btn" title="Yeni Sekme (Ctrl/Cmd+T)" onclick="uiCmd('new_tab')">
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
    </button>
  </div>

  <div class="nav-bar">
    <button id="btn-back" class="icon-btn" title="Geri" onclick="uiCmd('back')">
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
    </button>
    <button id="btn-fwd" class="icon-btn" title="İleri" onclick="uiCmd('forward')">
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
    </button>
    <button class="icon-btn" title="Yenile" onclick="uiCmd('reload')">
      <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>
    </button>
    <button class="icon-btn" title="Ana Sayfa" onclick="uiCmd('home')">
      <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5L12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z"/></svg>
    </button>

    <div class="omnibox">
      <button id="btn-sec" class="sec-pill" title="Bağlantı ve DNS Bilgisi" onclick="openPop('security', this)">
        <span id="sec-icon"></span>
      </button>
      <input id="url-input" type="text" placeholder="Web'de ara veya adres gir..."
             onkeydown="if(event.key==='Enter'){ event.preventDefault(); uiCmd('navigate', {url:this.value}); this.blur(); }"
             onfocus="this.select()">
      <button id="btn-star" class="icon-btn" style="width:24px;height:24px;" title="Yer İmlerine Ekle" onclick="uiCmd('toggle_bookmark')">
        <svg id="star-svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
      </button>
    </div>

    <button id="btn-shield" class="shield-pill" title="AdBlock Kalkanı" onclick="openPop('adblock', this)">
      <svg id="shield-svg" viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="#81c995" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
      <span id="shield-badge" class="badge">0</span>
    </button>

    <button id="btn-pw" class="icon-btn" title="Şifreler" onclick="openPop('passwords', this)">
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/></svg>
    </button>

    <button id="btn-hist" class="icon-btn" title="Geçmiş Yan Paneli" onclick="uiCmd('toggle_side_panel', {mode:'history'})">
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
    </button>

    <button id="btn-dl" class="icon-btn" title="İndirilenler" onclick="openPop('downloads', this)">
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
    </button>

    <button id="btn-menu" class="icon-btn" title="Menü" onclick="openPop('menu', this)">
      <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor"><circle cx="12" cy="5" r="1.7"/><circle cx="12" cy="12" r="1.7"/><circle cx="12" cy="19" r="1.7"/></svg>
    </button>
  </div>

  <div class="bm-bar" id="bm-bar"></div>

  <script>
    function uiCmd(cmd, payload) {
      if (window.ipc) window.ipc.postMessage(JSON.stringify({ type: 'ui_cmd', cmd, ...(payload || {}) }));
    }
    function openPop(popKind, el) {
      const r = el.getBoundingClientRect();
      uiCmd('open_popover', { pop_kind: popKind, anchor_right: Math.round(r.right), anchor_left: Math.round(r.left) });
    }
    window.__focusUrl = () => {
      const inp = document.getElementById('url-input');
      inp.focus(); inp.select();
    };
    const INTERNAL_TAB_SVG = '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="#8ab4f8" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>';
    const GLOBE_TAB_SVG = '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="#9aa0a6" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>';
    function syncTabIcon(iconWrap, t) {
      const s2 = t.host ? ('https://www.google.com/s2/favicons?domain=' + encodeURIComponent(t.host) + '&sz=32') : '';
      const desired = t.is_internal ? '' : (t.favicon || s2);
      const key = t.is_internal ? 'internal' : ('ext:' + desired + '|' + (t.host || ''));
      if (iconWrap.dataset.key === key) return;
      iconWrap.dataset.key = key;
      iconWrap.innerHTML = '';
      if (t.is_internal) {
        iconWrap.innerHTML = INTERNAL_TAB_SVG;
        return;
      }
      if (!desired) {
        iconWrap.innerHTML = GLOBE_TAB_SVG;
        return;
      }
      const img = document.createElement('img');
      img.alt = '';
      img.referrerPolicy = 'no-referrer';
      img.src = desired;
      img.onerror = () => {
        if (s2 && !img.dataset.triedS2 && img.src !== s2) {
          img.dataset.triedS2 = '1';
          img.src = s2;
        } else {
          iconWrap.innerHTML = GLOBE_TAB_SVG;
        }
      };
      iconWrap.appendChild(img);
    }
    window.__updateBarState = (st) => {
      const tl = document.getElementById('tabs-list');
      const existing = new Map();
      for (const node of Array.from(tl.children)) {
        if (node.dataset && node.dataset.tabId) existing.set(Number(node.dataset.tabId), node);
      }
      const keep = new Set();
      for (const t of st.tabs) {
        keep.add(t.id);
        let div = existing.get(t.id);
        if (!div) {
          div = document.createElement('div');
          div.dataset.tabId = String(t.id);
          const ico = document.createElement('span');
          ico.className = 'tab-icon';
          const sp = document.createElement('span');
          sp.className = 'tab-title';
          const cl = document.createElement('button');
          cl.className = 'tab-close';
          cl.textContent = '×';
          cl.onclick = (e) => { e.stopPropagation(); uiCmd('close_tab', { id: Number(div.dataset.tabId) }); };
          div.appendChild(ico);
          div.appendChild(sp);
          div.appendChild(cl);
        }
        div.className = 'tab' + (t.active ? ' active' : '');
        div.onclick = () => uiCmd('switch_tab', { id: t.id });
        syncTabIcon(div.querySelector('.tab-icon'), t);
        div.querySelector('.tab-title').textContent = t.title || 'Yeni Sekme';
        tl.appendChild(div);
      }
      for (const [id, node] of existing.entries()) {
        if (!keep.has(id)) node.remove();
      }

      const inp = document.getElementById('url-input');
      if (document.activeElement !== inp) inp.value = st.url || '';

      document.getElementById('btn-back').disabled = !st.can_go_back;
      document.getElementById('btn-fwd').disabled = !st.can_go_forward;
      document.getElementById('btn-hist').classList.toggle('active', !!st.side_panel_open);

      const secIcon = document.getElementById('sec-icon');
      if (st.is_internal) {
        secIcon.innerHTML = '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="#8ab4f8" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>';
      } else if (st.is_secure) {
        secIcon.innerHTML = '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="#81c995" stroke-width="2"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>';
      } else {
        secIcon.innerHTML = '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="#f28b82" stroke-width="2"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>';
      }

      const star = document.getElementById('star-svg');
      star.setAttribute('fill', st.is_bookmarked ? '#8ab4f8' : 'none');
      star.setAttribute('stroke', st.is_bookmarked ? '#8ab4f8' : 'currentColor');

      const shieldActive = st.adblock_enabled && !st.site_whitelisted;
      document.getElementById('shield-svg').setAttribute('stroke', shieldActive ? '#81c995' : '#f28b82');
      const badge = document.getElementById('shield-badge');
      badge.textContent = String(st.tab_blocked || 0);
      badge.className = 'badge' + (shieldActive ? '' : ' off');

      document.getElementById('btn-pw').style.color = st.has_saved_pw ? '#8ab4f8' : '#bdc1c6';

      const bmBar = document.getElementById('bm-bar');
      bmBar.classList.toggle('hidden', !st.show_bookmarks_bar);
      bmBar.innerHTML = '';
      for (const b of (st.bookmarks || [])) {
        const btn = document.createElement('button');
        btn.className = 'bm-item';
        btn.innerHTML = '<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="#8ab4f8" stroke-width="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg><span></span>';
        btn.querySelector('span').textContent = b.title;
        btn.onclick = () => uiCmd('navigate', { url: b.url });
        bmBar.appendChild(btn);
      }
    };
  </script>
</body>
</html>
"###
}

pub fn render_side_panel_html(mode: &str, storage: &BrowserStorage) -> String {
    let hist_active = mode == "history";
    let mut rows = String::new();
    if hist_active {
        for h in storage.history.iter().take(120) {
            rows.push_str(&format!(
                r###"<button class="item" data-search="{}" onclick="spCmd('navigate', {{url:'{}'}})">
                  <div class="it-t">{}</div>
                  <div class="it-s">{} • {}</div>
                </button>"###,
                attr(&(h.title.clone() + " " + &h.url)),
                attr(&h.url),
                txt(&h.title),
                txt(&h.visited_at),
                txt(&h.url)
            ));
        }
        if rows.is_empty() {
            rows = r###"<div class="empty">Geçmiş boş.</div>"###.to_string();
        }
    } else {
        for b in &storage.bookmarks {
            rows.push_str(&format!(
                r###"<button class="item" data-search="{}" onclick="spCmd('navigate', {{url:'{}'}})">
                  <div class="it-t">{}</div>
                  <div class="it-s">{}</div>
                </button>"###,
                attr(&(b.title.clone() + " " + &b.url)),
                attr(&b.url),
                txt(&b.title),
                txt(&b.url)
            ));
        }
        if rows.is_empty() {
            rows = r###"<div class="empty">Yer imi yok.</div>"###.to_string();
        }
    }

    let full_route = if hist_active { "history" } else { "bookmarks" };
    let full_label = if hist_active {
        "Tüm Geçmişi Sekmede Aç"
    } else {
        "Yer İmlerini Sekmede Aç"
    };

    format!(
        r###"<!DOCTYPE html>
<html lang="tr"><head><meta charset="utf-8">
<style>
  * {{ box-sizing: border-box; }}
  body {{
    margin:0; background:#292a2d; color:#e8eaed;
    font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;
    border-left:1px solid #3c4043; height:100vh; display:flex; flex-direction:column;
  }}
  .hdr {{ display:flex; align-items:center; gap:6px; padding:10px 12px; border-bottom:1px solid #3c4043; }}
  .seg {{ flex:1; border:none; border-radius:14px; padding:6px 10px; font-size:12px; cursor:pointer; background:transparent; color:#9aa0a6; font-weight:600; }}
  .seg.active {{ background:rgba(138,180,248,0.18); color:#8ab4f8; }}
  .close-btn {{ width:26px; height:26px; border-radius:50%; border:none; background:transparent; color:#9aa0a6; cursor:pointer; font-size:15px; }}
  .close-btn:hover {{ background:rgba(255,255,255,0.1); color:#e8eaed; }}
  .search {{ margin:10px 12px 6px; padding:7px 12px; border-radius:16px; border:1px solid #3c4043; background:#202124; color:#e8eaed; font-size:12.5px; outline:none; }}
  .list {{ flex:1; overflow-y:auto; padding:6px 8px; }}
  .item {{ width:100%; border:none; background:transparent; color:#e8eaed; text-align:left; padding:8px 10px; border-radius:10px; cursor:pointer; display:block; }}
  .item:hover {{ background:rgba(255,255,255,0.08); }}
  .it-t {{ font-size:12.5px; font-weight:500; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .it-s {{ font-size:11px; color:#9aa0a6; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; margin-top:2px; }}
  .empty {{ padding:24px; text-align:center; color:#9aa0a6; font-size:12.5px; }}
  .ftr {{ padding:10px 12px; border-top:1px solid #3c4043; }}
  .ftr button {{ width:100%; border:1px solid #3c4043; background:#202124; color:#8ab4f8; border-radius:16px; padding:7px 10px; font-size:12px; font-weight:600; cursor:pointer; }}
  .ftr button:hover {{ background:#303134; }}
</style>
<script>
  function spCmd(cmd, payload) {{
    if (window.ipc) window.ipc.postMessage(JSON.stringify({{ type:'ui_cmd', cmd, ...(payload || {{}}) }}));
  }}
  function filterSp(q) {{
    const s = (q || '').toLowerCase();
    for (const el of document.querySelectorAll('[data-search]')) {{
      el.style.display = (el.getAttribute('data-search') || '').toLowerCase().includes(s) ? 'block' : 'none';
    }}
  }}
</script></head>
<body>
  <div class="hdr">
    <button class="seg {}" onclick="spCmd('toggle_side_panel', {{mode:'history', force_open:true}})">Geçmiş</button>
    <button class="seg {}" onclick="spCmd('toggle_side_panel', {{mode:'bookmarks', force_open:true}})">Yer İmleri</button>
    <button class="close-btn" onclick="spCmd('close_side_panel')">×</button>
  </div>
  <input class="search" type="text" placeholder="Ara..." oninput="filterSp(this.value)">
  <div class="list">{rows}</div>
  <div class="ftr"><button onclick="spCmd('open_internal', {{route:'{full_route}'}})">{full_label}</button></div>
</body></html>"###,
        if hist_active { "active" } else { "" },
        if !hist_active { "active" } else { "" },
    )
}

pub enum PopoverKind<'a> {
    AdBlock {
        host: &'a str,
        ab: &'a AdBlockService,
        blocked_items: &'a [BlockedItem],
    },
    Security {
        host: &'a str,
        url: &'a str,
        is_secure: bool,
        https_only: bool,
        dns_on: bool,
        http_allowed: bool,
        dns_res: Option<&'a DnsCheckResult>,
    },
    Downloads {
        downloads: &'a [DownloadEntry],
    },
    Passwords {
        host: &'a str,
        creds: Vec<&'a SavedCredential>,
        pending: Option<(&'a str, &'a str, &'a str)>,
    },
    Menu {
        show_bookmarks_bar: bool,
    },
}

pub fn render_popover_html(kind: PopoverKind<'_>) -> (String, u32, u32) {
    let base_head = r###"<meta charset="utf-8"><style>
      * { box-sizing:border-box; user-select:none; }
      html, body { margin:0; padding:4px 6px 10px 6px; background:transparent; font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; color:#e8eaed; overflow:hidden; }
      .bubble { background:#292a2d; border:1px solid #3c4043; border-radius:14px; padding:12px; width:100%; height:auto; display:flex; flex-direction:column; gap:9px; box-shadow:0 6px 18px rgba(0,0,0,0.45); }
      .hdr { display:flex; align-items:center; gap:8px; }
      .hdr-title { flex:1; font-size:14.5px; font-weight:600; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
      .ico-btn { width:26px; height:26px; border-radius:50%; border:none; background:transparent; color:#9aa0a6; cursor:pointer; display:flex; align-items:center; justify-content:center; }
      .ico-btn:hover { background:rgba(255,255,255,0.1); color:#e8eaed; }
      .card { background:#202124; border:1px solid #3c4043; border-radius:10px; padding:10px 12px; }
      .row { display:flex; align-items:center; justify-content:space-between; gap:8px; }
      .sub { font-size:11.5px; color:#9aa0a6; margin-top:2px; }
      .stats { display:grid; grid-template-columns:repeat(3,1fr); text-align:center; gap:6px; }
      .st-val { font-size:16px; font-weight:700; }
      .st-lbl { font-size:11px; color:#9aa0a6; }
      .menu-row { width:100%; border:none; background:transparent; color:#e8eaed; padding:8px 10px; border-radius:10px; display:flex; align-items:center; gap:10px; cursor:pointer; text-align:left; }
      .menu-row:hover { background:rgba(255,255,255,0.08); }
      .mr-txt { flex:1; min-width:0; }
      .mr-t { font-size:13px; font-weight:500; }
      .mr-s { font-size:11px; color:#9aa0a6; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
      .sep { height:1px; background:#3c4043; margin:2px 0; }
      .pill-btn { border-radius:14px; padding:5px 12px; font-size:12px; font-weight:600; cursor:pointer; border:none; }
      .pill-danger { background:rgba(242,139,130,0.16); color:#f28b82; border:1px solid rgba(242,139,130,0.4); }
      .pill-ok { background:#81c995; color:#202124; }
      .pill-blue { background:#8ab4f8; color:#202124; }
      .chk-row { display:flex; align-items:center; gap:8px; font-size:12.5px; cursor:pointer; }
      .scroll { max-height:290px; overflow-y:auto; display:flex; flex-direction:column; gap:5px; }
      .hidden { display:none !important; }
    </style>
    <script>
      function popCmd(cmd, payload) {
        if (window.ipc) window.ipc.postMessage(JSON.stringify({ type:'ui_cmd', cmd, ...(payload || {}) }));
      }
      function syncPopHeight() {
        const b = document.querySelector('.bubble');
        if (!b || !window.ipc) return;
        const h = Math.ceil(b.getBoundingClientRect().height) + 16;
        popCmd('resize_popover', { height: h });
      }
      window.addEventListener('DOMContentLoaded', () => {
        syncPopHeight();
        const b = document.querySelector('.bubble');
        if (b && window.ResizeObserver) new ResizeObserver(syncPopHeight).observe(b);
      });
      window.addEventListener('keydown', (e) => { if (e.key === 'Escape') popCmd('close_popover'); });
    </script>"###;

    match kind {
        PopoverKind::AdBlock {
            host,
            ab,
            blocked_items,
        } => {
            let whitelisted = !host.is_empty() && ab.is_site_whitelisted(host);
            let mut log_html = String::new();
            for item in blocked_items.iter().take(40) {
                log_html.push_str(&format!(
                    r###"<div class="card" style="padding:7px 10px;">
                        <div style="color:#f28b82;font-size:11px;font-weight:600;">{} • {}</div>
                        <div class="sub" style="word-break:break-all;">{}</div>
                    </div>"###,
                    txt(&item.resource_type.to_uppercase()),
                    txt(&item.rule),
                    txt(&item.request_url)
                ));
            }
            if log_html.is_empty() {
                log_html = r###"<div class="sub" style="text-align:center;padding:20px;">Bu sekmede henüz engellenen istek yok.</div>"###.to_string();
            }

            let site_btn = if host.is_empty() {
                r###"<span class="sub">Dahili</span>"###.to_string()
            } else if whitelisted {
                format!(
                    r###"<button class="pill-btn pill-ok" onclick="popCmd('ab_toggle_site', {{host:'{}', paused:false}})">Etkinleştir</button>"###,
                    attr(host)
                )
            } else {
                format!(
                    r###"<button class="pill-btn pill-danger" onclick="popCmd('ab_toggle_site', {{host:'{}', paused:true}})">Duraklat</button>"###,
                    attr(host)
                )
            };

            let html = format!(
                r###"<!DOCTYPE html><html><head>{base_head}</head><body><div class="bubble">
                  <div id="p0" style="display:flex;flex-direction:column;gap:9px;">
                    <div class="hdr">
                      <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="{}" stroke-width="2"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
                      <div class="hdr-title">AdBlock Kalkanı</div>
                      <button class="ico-btn" title="Yenile" onclick="popCmd('close_popover'); popCmd('reload')"><svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg></button>
                      <button class="ico-btn" onclick="popCmd('close_popover')">×</button>
                    </div>
                    <div class="card row">
                      <div style="min-width:0;flex:1;">
                        <div style="font-size:13px;font-weight:600;">{}</div>
                        <div class="sub">{}</div>
                      </div>
                      {site_btn}
                    </div>
                    <div class="card stats">
                      <div><div class="st-val" style="color:#81c995;">{}</div><div class="st-lbl">Bu Sekmede</div></div>
                      <div><div class="st-val" style="color:#8ab4f8;">{}</div><div class="st-lbl">Toplam</div></div>
                      <div><div class="st-val" style="color:#d2a8ff;">{}</div><div class="st-lbl">Kurallar</div></div>
                    </div>
                    <div class="card" style="display:flex;flex-direction:column;gap:7px;">
                      <label class="chk-row"><input type="checkbox" {} onchange="popCmd('set_setting', {{key:'adblock_enabled', val:this.checked}})"> Genel Reklam Engelleme Aktif</label>
                      <label class="chk-row"><input type="checkbox" {} onchange="popCmd('set_setting', {{key:'cosmetic_enabled', val:this.checked}})"> Kozmetik & YouTube Filtreleme Aktif</label>
                    </div>
                    <button class="menu-row" onclick="document.getElementById('p0').classList.add('hidden'); document.getElementById('p1').classList.remove('hidden');">
                      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="#81c995" stroke-width="2"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
                      <div class="mr-txt"><div class="mr-t">Engellenen İstekler</div><div class="mr-s">Bu sekmede {} istek engellendi</div></div>
                      <span>›</span>
                    </button>
                    <div class="sep"></div>
                    <button class="menu-row" onclick="popCmd('open_internal', {{route:'adblock'}})">
                      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="#8ab4f8" stroke-width="2"><circle cx="12" cy="12" r="3"/></svg>
                      <div class="mr-txt"><div class="mr-t">Özel Kurallar, Listeler ve Beyaz Liste</div><div class="mr-s">Tarayıcı sekmesinde aç</div></div>
                      <span>↗</span>
                    </button>
                  </div>
                  <div id="p1" class="hidden" style="display:flex;flex-direction:column;gap:8px;">
                    <div class="hdr">
                      <button class="ico-btn" onclick="document.getElementById('p1').classList.add('hidden'); document.getElementById('p0').classList.remove('hidden');">‹</button>
                      <div class="hdr-title">Engellenen İstekler</div>
                      <button class="ico-btn" onclick="popCmd('close_popover')">×</button>
                    </div>
                    <div class="scroll">{log_html}</div>
                  </div>
                </div></body></html>"###,
                if ab.enabled { "#81c995" } else { "#f28b82" },
                if host.is_empty() { "Dahili Güvenli Sayfa" } else { host },
                if whitelisted { "Bu sitede koruma duraklatıldı" } else { "Reklam & izleyici engelleme aktif" },
                blocked_items.len(),
                ab.total_blocked,
                ab.total_rules_count,
                if ab.enabled { "checked" } else { "" },
                if ab.cosmetic_enabled { "checked" } else { "" },
                blocked_items.len()
            );
            (html, 368, 460)
        }
        PopoverKind::Security {
            host,
            url,
            is_secure,
            https_only,
            dns_on,
            http_allowed,
            dns_res,
        } => {
            let dns_text = match dns_res {
                Some(r) if r.safe => {
                    let ip_note = if r.resolved_ips.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", r.resolved_ips.join(", "))
                    };
                    format!("Temiz ({}: {}{})", r.host, r.reason, ip_note)
                }
                Some(r) => format!("Uyarı ({}): {}", r.host, r.reason),
                None if dns_on => "DoH (1.1.1.2) Koruması Aktif".to_string(),
                None => "DNS Koruması Kapalı".to_string(),
            };
            let title = if host.is_empty() { url } else { host };
            let http_toggle = if !host.is_empty() {
                format!(
                    r###"<label class="chk-row" style="margin-top:4px;"><input type="checkbox" {} onchange="popCmd('toggle_http_allowed', {{host:'{}', allowed:this.checked}})"> Bu sitede şifresiz HTTP bağlantısına izin ver</label>"###,
                    if http_allowed { "checked" } else { "" },
                    attr(host)
                )
            } else {
                String::new()
            };
            let html = format!(
                r###"<!DOCTYPE html><html><head>{base_head}</head><body><div class="bubble">
                  <div class="hdr">
                    <div class="hdr-title">{}</div>
                    <button class="ico-btn" onclick="popCmd('close_popover')">×</button>
                  </div>
                  <div class="card">
                    <div style="font-weight:600; color:{};">{}</div>
                    <div class="sub">HTTPS-First: {}</div>
                    {http_toggle}
                  </div>
                  <div class="card">
                    <div style="font-weight:600;">DNS Güvenlik Kalkanı</div>
                    <div class="sub">{}</div>
                  </div>
                  <button class="menu-row" onclick="popCmd('open_internal', {{route:'settings'}})">
                    <div class="mr-txt"><div class="mr-t">Site ve Güvenlik Ayarları</div><div class="mr-s">Ayarlar sekmesinde yönet</div></div>
                    <span>↗</span>
                  </button>
                </div></body></html>"###,
                txt(title),
                if is_secure { "#81c995" } else { "#f28b82" },
                if is_secure { "Bağlantı güvenli (HTTPS)" } else { "Bağlantı güvenli değil (HTTP)" },
                if https_only { "Açık" } else { "Kapalı" },
                txt(&dns_text),
            );
            (html, 340, 310)
        }
        PopoverKind::Downloads { downloads } => {
            let mut rows = String::new();
            for d in downloads.iter().take(6) {
                rows.push_str(&format!(
                    r###"<button class="menu-row" onclick="popCmd('reveal_file', {{path:'{}'}})">
                        <div class="mr-txt"><div class="mr-t">{}</div><div class="mr-s">{} • {}</div></div>
                        <span>↗</span>
                    </button>"###,
                    attr(&d.save_path),
                    txt(&d.filename),
                    txt(&d.status),
                    txt(&d.started_at)
                ));
            }
            if rows.is_empty() {
                rows = r###"<div class="sub" style="text-align:center;padding:24px;">Son indirilen dosya yok.</div>"###.to_string();
            }
            let html = format!(
                r###"<!DOCTYPE html><html><head>{base_head}</head><body><div class="bubble">
                  <div class="hdr">
                    <div class="hdr-title">Son İndirme Geçmişi</div>
                    <button class="ico-btn" onclick="popCmd('close_popover')">×</button>
                  </div>
                  <div class="scroll">{rows}</div>
                  <div class="sep"></div>
                  <button class="menu-row" onclick="popCmd('open_internal', {{route:'downloads'}})">
                    <div class="mr-txt"><div class="mr-t">Tüm İndirme Geçmişi</div></div>
                    <span>↗</span>
                  </button>
                </div></body></html>"###
            );
            (html, 355, 365)
        }
        PopoverKind::Passwords {
            host,
            creds,
            pending,
        } => {
            let mut body = String::new();
            if let Some((p_orig, p_user, p_pass)) = pending {
                body.push_str(&format!(
                    r###"<div class="card">
                      <div style="font-weight:600;color:#8ab4f8;">Şifre kaydedilsin mi? ({})</div>
                      <div class="sub">Kullanıcı: {}</div>
                      <div style="display:flex;gap:8px;margin-top:8px;">
                        <button class="pill-btn pill-blue" onclick="popCmd('pw_save_pending', {{origin:'{}', username:'{}', password:'{}'}})">Kaydet</button>
                        <button class="pill-btn pill-danger" onclick="popCmd('pw_never_save', {{origin:'{}'}})">Asla</button>
                      </div>
                    </div>"###,
                    txt(p_orig),
                    txt(p_user),
                    attr(p_orig),
                    attr(p_user),
                    attr(p_pass),
                    attr(p_orig),
                ));
            }
            for c in &creds {
                body.push_str(&format!(
                    r###"<button class="menu-row" onclick="popCmd('pw_fill', {{username:'{}', password:'{}'}})">
                      <div class="mr-txt"><div class="mr-t">{}</div><div class="mr-s">Tıklayınca form doldurulur</div></div>
                      <span style="color:#8ab4f8;font-size:11.5px;">Doldur</span>
                    </button>"###,
                    attr(&c.username),
                    attr(&c.password),
                    txt(&c.username)
                ));
            }
            if creds.is_empty() && pending.is_none() {
                body.push_str(&format!(
                    r###"<div class="sub" style="padding:14px 4px;">{} için kayıtlı şifre yok.</div>"###,
                    if host.is_empty() { "Bu sayfa" } else { host }
                ));
            }
            let html = format!(
                r###"<!DOCTYPE html><html><head>{base_head}</head><body><div class="bubble">
                  <div class="hdr">
                    <div class="hdr-title">Şifreler</div>
                    <button class="ico-btn" onclick="popCmd('close_popover')">×</button>
                  </div>
                  <div class="scroll">{body}</div>
                  <div class="sep"></div>
                  <button class="menu-row" onclick="popCmd('open_internal', {{route:'passwords'}})">
                    <div class="mr-txt">
                      <div class="mr-t">Chrome'dan İçe Aktar (.csv)</div>
                      <div class="mr-s">Google Chrome şifrelerini aktar</div>
                    </div>
                    <span>⇣</span>
                  </button>
                  <button class="menu-row" onclick="popCmd('open_internal', {{route:'passwords'}})">
                    <div class="mr-txt"><div class="mr-t">Şifre Yöneticisini Aç</div></div>
                    <span>↗</span>
                  </button>
                </div></body></html>"###
            );
            (html, 345, 355)
        }
        PopoverKind::Menu { show_bookmarks_bar } => {
            let bm_label = if show_bookmarks_bar {
                "Yer İmleri Çubuğunu Gizle"
            } else {
                "Yer İmleri Çubuğunu Göster"
            };
            let html = format!(
                r###"<!DOCTYPE html><html><head>{base_head}</head><body><div class="bubble">
                  <button class="menu-row" onclick="popCmd('new_tab')"><div class="mr-txt"><div class="mr-t">Yeni Sekme</div></div></button>
                  <button class="menu-row" onclick="popCmd('toggle_side_panel', {{mode:'history', force_open:true}})"><div class="mr-txt"><div class="mr-t">Geçmiş</div></div></button>
                  <button class="menu-row" onclick="popCmd('open_internal', {{route:'downloads'}})"><div class="mr-txt"><div class="mr-t">İndirilenler</div></div></button>
                  <button class="menu-row" onclick="popCmd('open_internal', {{route:'passwords'}})"><div class="mr-txt"><div class="mr-t">Şifreler</div></div></button>
                  <button class="menu-row" onclick="popCmd('open_internal', {{route:'adblock'}})"><div class="mr-txt"><div class="mr-t">AdBlock Filtre Yönetimi</div></div></button>
                  <button class="menu-row" onclick="popCmd('set_setting', {{key:'show_bookmarks_bar', val:{}}})"><div class="mr-txt"><div class="mr-t">{bm_label}</div></div></button>
                  <div class="sep"></div>
                  <button class="menu-row" onclick="popCmd('open_devtools')"><div class="mr-txt"><div class="mr-t">Geliştirici Araçları (DevTools)</div></div></button>
                  <button class="menu-row" onclick="popCmd('open_internal', {{route:'settings'}})"><div class="mr-txt"><div class="mr-t">Ayarlar</div></div></button>
                </div></body></html>"###,
                !show_bookmarks_bar
            );
            (html, 278, 420)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adblock_engine::AdBlockService;
    use crate::storage::BrowserStorage;

    #[test]
    fn renders_all_popovers_and_side_panel_cleanly() {
        let tmp = std::env::temp_dir().join(format!("ab_test_ui_{}", std::process::id()));
        let storage = BrowserStorage::load(tmp.clone());
        let ab = AdBlockService::load(&tmp, true, true, 12, vec![]);

        let (ab_html, w, h) = render_popover_html(PopoverKind::AdBlock {
            host: "www.youtube.com",
            ab: &ab,
            blocked_items: &[BlockedItem {
                resource_type: "script".into(),
                request_url: "https://doubleclick.net/ad.js".into(),
                rule: "||doubleclick.net^".into(),
            }],
        });
        assert!(w > 300 && h > 300);
        assert!(ab_html.contains("AdBlock Kalkanı"));
        assert!(ab_html.contains("www.youtube.com"));

        let (sec_html, _, _) = render_popover_html(PopoverKind::Security {
            host: "example.com",
            url: "https://example.com",
            is_secure: true,
            https_only: true,
            dns_on: true,
            http_allowed: false,
            dns_res: None,
        });
        assert!(sec_html.contains("Bağlantı güvenli (HTTPS)"));

        let (menu_html, _, _) = render_popover_html(PopoverKind::Menu {
            show_bookmarks_bar: true,
        });
        assert!(menu_html.contains("Geliştirici Araçları"));

        let sp = render_side_panel_html("history", &storage);
        assert!(sp.contains("Geçmiş"));
        let bar = chrome_bar_html();
        assert!(bar.contains("syncTabIcon"));
        assert!(bar.contains(".tab-icon"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
