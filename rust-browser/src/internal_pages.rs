use crate::adblock_engine::{AdBlockService, BUILTIN_FILTER_LISTS};
use crate::passwords::PasswordManager;
use crate::storage::BrowserStorage;
use html_escape::{encode_double_quoted_attribute as attr, encode_text as txt};

const NAV_ITEMS: &[(&str, &str, &str)] = &[
    ("home", "Yeni Sekme", "<path d='M3 10.5L12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z'/>"),
    ("settings", "Ayarlar", "<circle cx='12' cy='12' r='3'/><path d='M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9c.26.6.85 1 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z'/>"),
    ("downloads", "İndirilenler", "<path d='M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4'/><polyline points='7 10 12 15 17 10'/><line x1='12' y1='15' x2='12' y2='3'/>"),
    ("passwords", "Şifreler", "<path d='M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4'/>"),
    ("history", "Geçmiş", "<circle cx='12' cy='12' r='10'/><polyline points='12 6 12 12 16 14'/>"),
    ("bookmarks", "Yer İmleri", "<polygon points='12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2'/>"),
    ("adblock", "AdBlock Yönetimi", "<path d='M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z'/>"),
    ("updates", "Sürüm Geçmişi", "<circle cx='12' cy='12' r='10'/><path d='M12 16v-4'/><path d='M12 8h.01'/>"),
];

fn wrap_shell(active: &str, title: &str, body_html: &str) -> String {
    // The new-tab home page is clean (no sidebar); the sidebar only appears
    // on settings-style internal pages (Ayarlar, İndirilenler, Şifreler ...).
    let show_sidebar = active != "home";
    let mut nav = String::new();
    for (key, label, svg_path) in NAV_ITEMS {
        if *key == "home" {
            continue;
        }
        let cls = if *key == active { "nav-item active" } else { "nav-item" };
        nav.push_str(&format!(
            r###"<button class="{cls}" onclick="sendCmd('open_internal', {{route:'{key}'}})">
                <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">{svg_path}</svg>
                <span>{label}</span>
            </button>"###
        ));
    }
    let aside_html = if show_sidebar {
        format!(
            r###"<aside>
    <div class="brand">
      <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="#81c995" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="M9 12l2 2 4-4"/></svg>
      <span>AdBlock Tarayıcı</span>
    </div>
    {nav}
  </aside>"###
        )
    } else {
        String::new()
    };
    let main_cls = if show_sidebar { "" } else { "home-main" };

    format!(
        r###"<!DOCTYPE html>
<html lang="tr">
<head>
<meta charset="utf-8">
<title>{}</title>
<style>
  :root {{
    color-scheme: dark;
    --bg: #202124;
    --surface: #292a2d;
    --surface2: #303134;
    --border: #3c4043;
    --text: #e8eaed;
    --sub: #9aa0a6;
    --blue: #8ab4f8;
    --green: #81c995;
    --red: #f28b82;
  }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: var(--bg);
    color: var(--text);
    display: flex;
    min-height: 100vh;
  }}
  aside {{
    width: 240px;
    background: #1b1c1f;
    border-right: 1px solid var(--border);
    padding: 20px 12px;
    flex-shrink: 0;
  }}
  .brand {{
    display: flex; align-items: center; gap: 10px;
    font-size: 16px; font-weight: 700; padding: 6px 12px 18px; color: var(--text);
  }}
  .nav-item {{
    width: 100%; border: none; background: transparent; color: var(--sub);
    display: flex; align-items: center; gap: 10px;
    padding: 10px 14px; border-radius: 20px; cursor: pointer;
    font-size: 13.5px; text-align: left; margin-bottom: 4px;
  }}
  .nav-item:hover {{ background: rgba(255,255,255,0.06); color: var(--text); }}
  .nav-item.active {{ background: rgba(138,180,248,0.16); color: var(--blue); font-weight: 600; }}
  main {{ flex: 1; padding: 32px 44px; max-width: 980px; }}
  main.home-main {{ max-width: 980px; margin: 0 auto; padding-top: 24px; }}
  .header-row {{ display: flex; justify-content: space-between; align-items: center; gap: 16px; margin-bottom: 22px; }}
  h1 {{ font-size: 22px; margin: 0; font-weight: 600; }}
  .card {{
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 14px; margin-bottom: 18px; overflow: hidden;
  }}
  .card-title {{
    padding: 13px 18px; font-size: 13px; font-weight: 600; color: var(--blue);
    border-bottom: 1px solid var(--border); background: rgba(255,255,255,0.02);
  }}
  .row {{
    display: flex; justify-content: space-between; align-items: center; gap: 14px;
    padding: 13px 18px; border-bottom: 1px solid rgba(255,255,255,0.06);
  }}
  .row:last-child {{ border-bottom: none; }}
  .row-info {{ min-width: 0; flex: 1; }}
  .row-title {{ font-size: 13.5px; font-weight: 500; word-break: break-all; }}
  .row-sub {{ font-size: 12px; color: var(--sub); margin-top: 2px; word-break: break-all; }}
  .actions {{ display: flex; gap: 8px; align-items: center; flex-shrink: 0; }}
  input[type=text], input[type=password], select, textarea {{
    background: var(--bg); color: var(--text); border: 1px solid var(--border);
    border-radius: 8px; padding: 8px 11px; font-size: 13px; outline: none;
  }}
  input:focus, textarea:focus, select:focus {{ border-color: var(--blue); }}
  .search-input {{ width: 260px; border-radius: 20px !important; padding: 8px 14px !important; }}
  button.btn, a.btn {{
    background: var(--surface2); color: var(--text); border: 1px solid var(--border);
    border-radius: 18px; padding: 6px 14px; font-size: 12.5px; cursor: pointer; text-decoration: none;
  }}
  button.btn:hover {{ background: #3c4043; }}
  button.btn-primary {{
    background: var(--blue); color: #202124; border: none; font-weight: 600;
    border-radius: 18px; padding: 7px 16px; cursor: pointer; font-size: 12.5px;
  }}
  button.btn-danger {{
    background: rgba(242,139,130,0.14); color: var(--red); border: 1px solid rgba(242,139,130,0.35);
    border-radius: 18px; padding: 6px 13px; cursor: pointer; font-size: 12.5px;
  }}
  .empty {{ padding: 28px; text-align: center; color: var(--sub); font-size: 13px; }}
  .hero {{ text-align: center; padding: 36px 16px 18px; }}
  .hero h2 {{ font-size: 30px; margin: 12px 0 8px; }}
  .hero-search {{ display: flex; gap: 8px; max-width: 600px; margin: 22px auto 10px; }}
  .hero-search input {{ flex: 1; border-radius: 24px; padding: 12px 18px; font-size: 14.5px; }}
  .bm-section {{ margin-top: 22px; }}
  .bm-head {{
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: 14px; padding: 0 2px;
  }}
  .bm-head-title {{ font-size: 14px; font-weight: 600; color: var(--sub); letter-spacing: 0.2px; }}
  .grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 18px; }}
  .tile {{
    background: var(--surface); border: 1px solid var(--border); border-radius: 16px;
    padding: 0; cursor: pointer; text-align: left; color: var(--text);
    overflow: hidden; display: flex; flex-direction: column;
    transition: transform 0.16s ease, border-color 0.16s ease, box-shadow 0.16s ease, background 0.16s ease;
  }}
  .tile:hover {{
    border-color: var(--blue); background: #303236;
    transform: translateY(-2px); box-shadow: 0 10px 26px rgba(0,0,0,0.32);
  }}
  .tile-preview {{
    position: relative; width: 100%; height: 154px;
    background: linear-gradient(135deg, #1b1d21 0%, #25282e 100%);
    border-bottom: 1px solid rgba(255,255,255,0.06);
    overflow: hidden; display: flex; align-items: center; justify-content: center;
  }}
  .tile-preview-bar {{
    position: absolute; top: 0; left: 0; right: 0; height: 22px; z-index: 2;
    background: rgba(20, 21, 24, 0.82); backdrop-filter: blur(6px);
    border-bottom: 1px solid rgba(255,255,255,0.07);
    display: flex; align-items: center; gap: 5px; padding: 0 10px;
  }}
  .tile-dot {{ width: 6px; height: 6px; border-radius: 50%; background: #4e5259; }}
  .tile-dot.r {{ background: #e06c75; }}
  .tile-dot.y {{ background: #e5c07b; }}
  .tile-dot.g {{ background: #81c995; }}
  .tile-preview-domain {{
    margin-left: 4px; font-size: 10.5px; color: var(--sub);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }}
  .tile-fallback {{
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    gap: 8px; padding-top: 22px; color: var(--sub); font-size: 12px;
  }}
  .tile-fallback-icon {{
    width: 44px; height: 44px; border-radius: 12px;
    background: rgba(138,180,248,0.12); border: 1px solid rgba(138,180,248,0.24);
    display: flex; align-items: center; justify-content: center;
    font-size: 18px; font-weight: 700; color: var(--blue); overflow: hidden;
  }}
  .tile-fallback-icon img {{ width: 28px; height: 28px; border-radius: 6px; }}
  .tile-shot {{
    position: absolute; top: 22px; left: 0; width: 100%; height: calc(100% - 22px);
    object-fit: cover; object-position: top center; z-index: 1;
    opacity: 0; transition: opacity 0.25s ease, transform 0.25s ease;
  }}
  .tile-shot.loaded {{ opacity: 1; }}
  .tile:hover .tile-shot.loaded {{ transform: scale(1.03); }}
  .tile-meta {{
    display: flex; align-items: center; gap: 11px; padding: 13px 15px; min-width: 0; width: 100%;
  }}
  .tile-fav {{
    width: 24px; height: 24px; border-radius: 7px; flex-shrink: 0;
    background: rgba(255,255,255,0.06); display: flex; align-items: center; justify-content: center;
    font-size: 12px; font-weight: 700; color: var(--blue); overflow: hidden;
  }}
  .tile-fav img {{ width: 18px; height: 18px; border-radius: 4px; }}
  .tile-text {{ min-width: 0; flex: 1; }}
  .tile-title {{
    font-size: 14.5px; font-weight: 600; color: var(--text);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }}
  .tile-url {{
    font-size: 12px; color: var(--sub); margin-top: 2px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }}
</style>
<script>
  function sendCmd(cmd, payload) {{
    if (window.ipc) {{
      window.ipc.postMessage(JSON.stringify({{ type: 'internal_cmd', cmd, ...(payload || {{}}) }}));
    }}
  }}
  function filterRows(q) {{
    const s = (q || '').toLowerCase();
    for (const el of document.querySelectorAll('[data-search]')) {{
      const hay = (el.getAttribute('data-search') || '').toLowerCase();
      el.style.display = hay.includes(s) ? '' : 'none';
    }}
  }}
</script>
</head>
<body>
  {aside_html}
  <main class="{main_cls}">{body_html}</main>
</body>
</html>"###,
        txt(title)
    )
}

fn bookmark_host(raw_url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(raw_url) {
        if let Some(host) = parsed.host_str() {
            return host.trim_start_matches("www.").to_string();
        }
    }
    raw_url.to_string()
}

fn is_public_http_url(raw_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(raw_url) else {
        return false;
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let lower = host.to_ascii_lowercase();
    lower.contains('.')
        && lower != "localhost"
        && !lower.starts_with("127.")
        && !lower.starts_with("10.")
        && !lower.starts_with("192.168.")
        && !lower.ends_with(".local")
}

pub fn render_route(
    route: &str,
    storage: &BrowserStorage,
    ab: &AdBlockService,
    pw: &PasswordManager,
) -> String {
    match route {
        "home" => render_home(storage),
        "settings" => render_settings(storage),
        "downloads" => render_downloads(storage),
        "passwords" => render_passwords(storage, pw),
        "history" => render_history(storage),
        "bookmarks" => render_bookmarks(storage),
        "adblock" => render_adblock(ab),
        "updates" => render_updates(),
        _ => render_home(storage),
    }
}

fn render_home(storage: &BrowserStorage) -> String {
    let mut tiles = String::new();
    for b in storage.bookmarks.iter().take(9) {
        let host = bookmark_host(&b.url);
        let initial: String = b
            .title
            .trim()
            .chars()
            .next()
            .or_else(|| host.chars().next())
            .unwrap_or('•')
            .to_uppercase()
            .collect();
        let initial_txt = txt(&initial);
        let host_txt = txt(&host);

        let (shot_html, fav_lg_html, fav_sm_html) = if is_public_http_url(&b.url) {
            let shot_src = format!(
                "https://image.thum.io/get/width/640/crop/400/noanimate/{}",
                b.url.trim()
            );
            let fav_src = format!(
                "https://www.google.com/s2/favicons?domain={}&sz=64",
                host
            );
            (
                format!(
                    r###"<img class="tile-shot" loading="lazy" src="{}" alt="" onload="this.classList.add('loaded')" onerror="this.style.display='none'">"###,
                    attr(&shot_src)
                ),
                format!(
                    r###"<img src="{}" alt="" onerror="this.replaceWith(document.createTextNode(this.dataset.fb))" data-fb="{}">"###,
                    attr(&fav_src),
                    attr(&initial)
                ),
                format!(
                    r###"<img src="{}" alt="" onerror="this.replaceWith(document.createTextNode(this.dataset.fb))" data-fb="{}">"###,
                    attr(&fav_src),
                    attr(&initial)
                ),
            )
        } else {
            (
                String::new(),
                initial_txt.to_string(),
                initial_txt.to_string(),
            )
        };

        tiles.push_str(&format!(
            r###"<button class="tile" data-url="{}" onclick="sendCmd('navigate', {{url: this.dataset.url}})">
                <div class="tile-preview">
                  <div class="tile-preview-bar">
                    <span class="tile-dot r"></span>
                    <span class="tile-dot y"></span>
                    <span class="tile-dot g"></span>
                    <span class="tile-preview-domain">{host_txt}</span>
                  </div>
                  <div class="tile-fallback">
                    <div class="tile-fallback-icon">{fav_lg_html}</div>
                    <span>{host_txt}</span>
                  </div>
                  {shot_html}
                </div>
                <div class="tile-meta">
                  <div class="tile-fav">{fav_sm_html}</div>
                  <div class="tile-text">
                    <div class="tile-title">{}</div>
                    <div class="tile-url">{host_txt}</div>
                  </div>
                </div>
            </button>"###,
            attr(&b.url),
            txt(&b.title),
        ));
    }

    let bookmarks_section = if tiles.is_empty() {
        r###"<div class="bm-section">
          <div class="empty">Henüz yer imi eklenmemiş. Adres çubuğundaki ★ simgesiyle hızlıca yer imi ekleyebilirsiniz.</div>
        </div>"###
            .to_string()
    } else {
        format!(
            r###"<div class="bm-section">
          <div class="bm-head">
            <div class="bm-head-title">Yer İmleri</div>
            <button class="btn" onclick="sendCmd('open_internal', {{route:'bookmarks'}})">Tümünü Yönet</button>
          </div>
          <div class="grid">{tiles}</div>
        </div>"###
        )
    };

    let body = format!(
        r###"
        <div class="hero">
          <svg viewBox="0 0 24 24" width="54" height="54" fill="none" stroke="#81c995" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="M9 12l2 2 4-4"/></svg>
          <h2>Gizlilik ve Reklam Kalkanı Aktif</h2>
          <div style="color:#9aa0a6; font-size:13.5px;">Rust motoru ile HTTPS-First + DNS Koruması + YouTube Reklam Engelleme</div>
          <form class="hero-search" onsubmit="event.preventDefault(); sendCmd('navigate', {{url: document.getElementById('hq').value}});">
            <input id="hq" type="text" placeholder="Web'de ara veya adres gir..." autofocus>
            <button type="submit" class="btn-primary">Git</button>
          </form>
        </div>
        {bookmarks_section}
        "###
    );
    wrap_shell("home", "Yeni Sekme — AdBlock Tarayıcı", &body)
}

fn render_settings(storage: &BrowserStorage) -> String {
    let s = &storage.settings;
    let chk = |b: bool| if b { "checked" } else { "" };
    let zoom_pct = (s.effective_zoom() * 100.0).round() as i32;
    let effective_dl = storage.effective_download_dir().to_string_lossy().to_string();

    let notice_html = if let Some(msg) = &storage.settings_notice {
        format!(
            r###"<div class="card" style="border-color:rgba(129,201,149,0.45); background:rgba(129,201,149,0.10);">
              <div class="row">
                <div class="row-info"><div class="row-title" style="color:var(--green);">{}</div></div>
                <button class="btn" onclick="sendCmd('clear_settings_notice')">Kapat</button>
              </div>
            </div>"###,
            txt(msg)
        )
    } else {
        String::new()
    };

    let mut http_rows = String::new();
    for d in &s.http_allowed_domains {
        http_rows.push_str(&format!(
            r###"<div class="row"><div class="row-info"><div class="row-title">{}</div></div>
               <button class="btn-danger" onclick="sendCmd('disallow_http', {{host:'{}'}})">Kaldır</button></div>"###,
            txt(d),
            attr(d)
        ));
    }
    if http_rows.is_empty() {
        http_rows = r###"<div class="empty">HTTP istisnası tanımlanmış site yok.</div>"###.to_string();
    }

    let zoom_opts = [75, 85, 90, 100, 110, 125, 150]
        .iter()
        .map(|pct| {
            let sel = if (*pct - zoom_pct).abs() <= 2 { "selected" } else { "" };
            let val = (*pct as f64) / 100.0;
            let lbl = if *pct == 100 {
                "%100 (Varsayılan)".to_string()
            } else {
                format!("%{pct}")
            };
            format!(r#"<option value="{val:.2}" {sel}>{lbl}</option>"#)
        })
        .collect::<Vec<_>>()
        .join("");

    let body = format!(
        r###"
        <div class="header-row"><h1>Tarayıcı Ayarları</h1></div>
        {notice_html}
        <div class="card">
          <div class="card-title">Gizlilik, Güvenlik ve Engelleme</div>
          <div class="row">
            <div class="row-info"><div class="row-title">Reklam &amp; İzleyici Engelleme</div><div class="row-sub">Genel AdBlock Plus motorunu aç/kapat</div></div>
            <input type="checkbox" {} onchange="sendCmd('set_setting', {{key:'adblock_enabled', val:this.checked}})">
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Kozmetik &amp; YouTube Filtreleme</div><div class="row-sub">Reklam kutularını gizle ve video reklamlarını atla</div></div>
            <input type="checkbox" {} onchange="sendCmd('set_setting', {{key:'cosmetic_enabled', val:this.checked}})">
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">HTTPS-First Modu</div><div class="row-sub">Şifresiz HTTP bağlantılarını otomatik HTTPS'e yükselt</div></div>
            <input type="checkbox" {} onchange="sendCmd('set_setting', {{key:'https_only', val:this.checked}})">
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">DNS Güvenlik Koruması (Cloudflare 1.1.1.2 DoH)</div><div class="row-sub">Zararlı ve oltalama alan adlarını açılmadan engelle</div></div>
            <input type="checkbox" {} onchange="sendCmd('set_setting', {{key:'dns_protection', val:this.checked}})">
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Şifre Kaydetme Önerisi</div><div class="row-sub">Giriş formlarını şifreli kasaya kaydetmeyi teklif et</div></div>
            <input type="checkbox" {} onchange="sendCmd('set_setting', {{key:'password_saving_enabled', val:this.checked}})">
          </div>
        </div>

        <div class="card">
          <div class="card-title">Tarama Verilerini Temizle</div>
          <div class="row">
            <div class="row-info"><div class="row-title">Tarama Geçmişi</div><div class="row-sub">Ziyaret edilen adreslerin listesini siler</div></div>
            <input id="cbd-history" type="checkbox" checked>
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Çerezler ve Site Oturumları</div><div class="row-sub">Sitelerdeki oturum ve çerez kayıtlarını temizler (kayıtlı şifre kasasına dokunmaz)</div></div>
            <input id="cbd-cookies" type="checkbox" checked>
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">İndirme Geçmişi Listesi</div><div class="row-sub">Yalnızca tarayıcıdaki listeyi temizler, diskteki dosyaları silmez</div></div>
            <input id="cbd-downloads" type="checkbox">
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Seçili Verileri Temizle</div></div>
            <button class="btn-danger" onclick="sendCmd('clear_browsing_data', {{clear_history: document.getElementById('cbd-history').checked, clear_cookies: document.getElementById('cbd-cookies').checked, clear_downloads: document.getElementById('cbd-downloads').checked}})">Verileri Temizle</button>
          </div>
        </div>

        <div class="card">
          <div class="card-title">Görünüm ve Performans</div>
          <div class="row">
            <div class="row-info"><div class="row-title">Yer İmleri Çubuğunu Göster</div><div class="row-sub">Adres çubuğunun altında hızlı erişim çubuğu</div></div>
            <input type="checkbox" {} onchange="sendCmd('set_setting', {{key:'show_bookmarks_bar', val:this.checked}})">
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Varsayılan Sayfa Yakınlaştırma</div><div class="row-sub">Tüm açık ve yeni sekmelerde sayfa ölçeğini ayarlar</div></div>
            <select onchange="sendCmd('set_setting', {{key:'default_zoom', val: parseFloat(this.value)}})">
              {zoom_opts}
            </select>
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Bellek &amp; Enerji Tasarrufu</div><div class="row-sub">Arka plandaki sekmelerin CPU kullanımını kısar (yeni açılan sekmelerde geçerli)</div></div>
            <input type="checkbox" {} onchange="sendCmd('set_setting', {{key:'background_throttling', val:this.checked}})">
          </div>
        </div>

        <div class="card">
          <div class="card-title">Arama Motoru, Başlangıç ve Ana Sayfa</div>
          <div class="row">
            <div class="row-info"><div class="row-title">Varsayılan Arama Motoru</div></div>
            <select onchange="sendCmd('set_setting', {{key:'search_engine', val:this.value}})">
              <option value="https://www.google.com/search?q=" {}>Google</option>
              <option value="https://duckduckgo.com/?q=" {}>DuckDuckGo</option>
              <option value="https://search.brave.com/search?q=" {}>Brave Search</option>
              <option value="https://www.bing.com/search?q=" {}>Bing</option>
              <option value="https://yandex.com/search/?text=" {}>Yandex</option>
            </select>
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Başlangıçta</div><div class="row-sub">Tarayıcı başlatıldığında hangi sekmelerin açılacağını belirler</div></div>
            <select onchange="sendCmd('set_setting', {{key:'startup_mode', val:this.value}})">
              <option value="new_tab" {}>Yeni Sekme sayfasını aç</option>
              <option value="restore" {}>Kaldığın yerden devam et (son sekmeler)</option>
              <option value="custom" {}>Özel ana sayfayı aç</option>
            </select>
          </div>
          <div class="row">
            <div class="row-info">
              <div class="row-title">Ana Sayfa Adresi</div>
              <div class="row-sub">Araç çubuğundaki Ana Sayfa düğmesi ve özel başlangıç için</div>
            </div>
            <div class="actions">
              <input id="home-url-inp" type="text" style="width:240px;" value="{}" placeholder="adblock://home">
              <button class="btn-primary" onclick="sendCmd('set_setting', {{key:'home_url', val: document.getElementById('home-url-inp').value}})">Kaydet</button>
              <button class="btn" onclick="sendCmd('set_setting', {{key:'home_url', val:'adblock://home'}})">Sıfırla</button>
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-title">İndirilenler, Sistem ve Sıfırlama</div>
          <div class="row">
            <div class="row-info">
              <div class="row-title">İndirme Klasörü</div>
              <div class="row-sub">Etkin konum: {}</div>
            </div>
            <div class="actions">
              <input id="dl-dir-inp" type="text" style="width:230px;" value="{}" placeholder="Sistem Varsayılanı">
              <button class="btn-primary" onclick="sendCmd('set_setting', {{key:'download_dir', val: document.getElementById('dl-dir-inp').value}})">Kaydet</button>
              <button class="btn" onclick="sendCmd('open_download_dir')">Klasörü Aç</button>
            </div>
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Varsayılan Tarayıcı</div><div class="row-sub">İşletim sistemi varsayılan tarayıcı ayarlarını açar</div></div>
            <button class="btn" onclick="sendCmd('open_default_browser_settings')">Sistem Ayarlarını Aç</button>
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Ayarları Sıfırla</div><div class="row-sub">Gizlilik, arama, yakınlaştırma ve başlangıç ayarlarını varsayılana döndürür (şifreleriniz ve yer imleriniz korunur)</div></div>
            <button class="btn-danger" onclick="if(confirm('Tüm tarayıcı ayarları varsayılanlara döndürülsün mü?')) sendCmd('reset_settings')">Ayarları Sıfırla</button>
          </div>
        </div>

        <div class="card">
          <div class="card-title">HTTP İstisnaları (Güvensiz Bağlantıya İzin Verilen Siteler)</div>
          {http_rows}
        </div>
        "###,
        chk(s.adblock_enabled),
        chk(s.cosmetic_enabled),
        chk(s.https_only),
        chk(s.dns_protection),
        chk(s.password_saving_enabled),
        chk(s.show_bookmarks_bar),
        chk(s.background_throttling),
        if s.search_engine.contains("google") { "selected" } else { "" },
        if s.search_engine.contains("duckduckgo") { "selected" } else { "" },
        if s.search_engine.contains("brave") { "selected" } else { "" },
        if s.search_engine.contains("bing") { "selected" } else { "" },
        if s.search_engine.contains("yandex") { "selected" } else { "" },
        if s.startup_mode == "new_tab" || (s.startup_mode != "restore" && s.startup_mode != "custom") { "selected" } else { "" },
        if s.startup_mode == "restore" { "selected" } else { "" },
        if s.startup_mode == "custom" { "selected" } else { "" },
        attr(&s.home_url),
        txt(&effective_dl),
        attr(&s.download_dir),
    );
    wrap_shell("settings", "Ayarlar — AdBlock Tarayıcı", &body)
}

fn render_downloads(storage: &BrowserStorage) -> String {
    let mut rows = String::new();
    for dl in &storage.downloads {
        rows.push_str(&format!(
            r###"<div class="row" data-search="{} {}">
                <div class="row-info">
                  <div class="row-title">{}</div>
                  <div class="row-sub">{} • {} • {}</div>
                </div>
                <div class="actions">
                  <button class="btn" onclick="sendCmd('reveal_file', {{path:'{}'}})">Klasörde Göster</button>
                </div>
              </div>"###,
            attr(&dl.filename),
            attr(&dl.url),
            txt(&dl.filename),
            txt(&dl.status),
            txt(&dl.started_at),
            txt(&dl.save_path),
            attr(&dl.save_path)
        ));
    }
    if rows.is_empty() {
        rows = r###"<div class="empty">Henüz indirilen dosya yok.</div>"###.to_string();
    }
    let body = format!(
        r###"
        <div class="header-row">
          <h1>İndirilenler</h1>
          <div class="actions">
            <input class="search-input" type="text" placeholder="İndirilenlerde ara..." oninput="filterRows(this.value)">
            <button class="btn-danger" onclick="sendCmd('clear_downloads')">Listeyi Temizle</button>
          </div>
        </div>
        <div class="card">{rows}</div>
        "###
    );
    wrap_shell("downloads", "İndirilenler — AdBlock Tarayıcı", &body)
}

fn render_passwords(_storage: &BrowserStorage, pw: &PasswordManager) -> String {
    let mut rows = String::new();
    for (idx, c) in pw.credentials.iter().enumerate() {
        rows.push_str(&format!(
            r###"<div class="row" data-search="{} {}">
                <div class="row-info">
                  <div class="row-title">{}</div>
                  <div class="row-sub">Kullanıcı: {} • Güncelleme: {}</div>
                </div>
                <div class="actions">
                  <span id="pw-{idx}" data-pw="{}" style="font-family:monospace; color:#9aa0a6; font-size:12.5px;">••••••••</span>
                  <button class="btn" onclick="const el=document.getElementById('pw-{idx}'); const shown=el.dataset.shown==='1'; el.textContent=shown ? '••••••••' : el.dataset.pw; el.dataset.shown=shown ? '0' : '1'; this.textContent=shown ? 'Göster' : 'Gizle';">Göster</button>
                  <button class="btn-danger" onclick="sendCmd('pw_delete', {{idx:{idx}}})">Sil</button>
                </div>
              </div>"###,
            attr(&c.origin),
            attr(&c.username),
            txt(&c.origin),
            txt(&c.username),
            txt(&c.updated_at),
            attr(&c.password)
        ));
    }
    if rows.is_empty() {
        rows = r###"<div class="empty">Kayıtlı şifre bulunmuyor.</div>"###.to_string();
    }

    let import_banner = if let Some(msg) = &pw.last_import_notice {
        format!(
            r###"<div class="card" style="border-color:rgba(129,201,149,.35); background:rgba(129,201,149,.09); color:#c8f7d4; font-size:13.5px; display:flex; justify-content:space-between; align-items:center;">
                <span>{}</span>
                <button class="btn" onclick="sendCmd('pw_clear_notice')">Tamam</button>
              </div>"###,
            txt(msg)
        )
    } else {
        String::new()
    };

    let body = format!(
        r###"
        <div class="header-row">
          <h1>Şifre Yöneticisi (AES-256-GCM Şifreli)</h1>
          <input class="search-input" type="text" placeholder="Site veya kullanıcı ara..." oninput="filterRows(this.value)">
        </div>
        {import_banner}
        <div class="card">
          <div class="card-title">Google Chrome'dan Şifre İçe Aktar (.csv)</div>
          <p style="font-size:12.5px; color:#9aa0a6; margin-bottom:12px; line-height:1.5;">
            Chrome'da <code style="background:rgba(255,255,255,.08); padding:2px 6px; border-radius:5px; color:#e8eaed;">chrome://password-manager/settings</code> &rarr; <strong>Şifreleri dışa aktar</strong> adımından aldığınız <code>.csv</code> dosyasını seçin veya buraya sürükleyin. Tüm şifreler AES-256-GCM kasasına şifrelenerek eklenir.
          </p>
          <div id="chrome-dropzone" style="border:1.5px dashed rgba(138,180,248,.38); border-radius:12px; padding:18px; text-align:center; background:rgba(138,180,248,.05); cursor:pointer;"
               onclick="document.getElementById('chrome-csv-input').click();"
               ondragover="event.preventDefault(); this.style.borderColor='#8ab4f8'; this.style.background='rgba(138,180,248,.12)';"
               ondragleave="this.style.borderColor='rgba(138,180,248,.38)'; this.style.background='rgba(138,180,248,.05)';"
               ondrop="event.preventDefault(); this.style.borderColor='rgba(138,180,248,.38)'; if(event.dataTransfer.files[0]) window.__importChromeCsvFile(event.dataTransfer.files[0]);">
            <input id="chrome-csv-input" type="file" accept=".csv,text/csv" style="display:none;" onchange="if(this.files[0]) window.__importChromeCsvFile(this.files[0]);">
            <div style="font-size:13.5px; font-weight:600; color:#e8eaed; margin-bottom:4px;">Chrome Şifreleri (.csv) Dosyası Seç veya Sürükle</div>
            <div style="font-size:12px; color:#9aa0a6;">Desteklenen biçim: Google Chrome / Brave / Edge Passwords CSV</div>
          </div>
          <script>
            window.__importChromeCsvFile = (file) => {{
              if (!file) return;
              const reader = new FileReader();
              reader.onload = () => {{
                sendCmd('pw_import_csv', {{ csv_data: String(reader.result || '') }});
              }};
              reader.readAsText(file, 'UTF-8');
            }};
          </script>
        </div>
        <div class="card">
          <div class="card-title">Yeni Şifre Ekle</div>
          <form class="row" onsubmit="event.preventDefault(); sendCmd('pw_add', {{origin:this.o.value, username:this.u.value, password:this.p.value}});">
            <input name="o" type="text" placeholder="Site (ör. github.com)" required>
            <input name="u" type="text" placeholder="Kullanıcı adı / e-posta" required>
            <input name="p" type="password" placeholder="Şifre" required>
            <button type="submit" class="btn-primary">Kaydet</button>
          </form>
        </div>
        <div class="card">{rows}</div>
        "###
    );
    wrap_shell("passwords", "Şifreler — AdBlock Tarayıcı", &body)
}

fn render_history(storage: &BrowserStorage) -> String {
    let mut rows = String::new();
    for h in storage.history.iter().take(200) {
        rows.push_str(&format!(
            r###"<div class="row" data-search="{} {}">
                <div class="row-info">
                  <div class="row-title"><a href="javascript:void(0)" style="color:#8ab4f8;text-decoration:none;" onclick="sendCmd('navigate', {{url:'{}'}})">{}</a></div>
                  <div class="row-sub">{} • {}</div>
                </div>
              </div>"###,
            attr(&h.title),
            attr(&h.url),
            attr(&h.url),
            txt(&h.title),
            txt(&h.visited_at),
            txt(&h.url)
        ));
    }
    if rows.is_empty() {
        rows = r###"<div class="empty">Tarama geçmişi boş.</div>"###.to_string();
    }
    let body = format!(
        r###"
        <div class="header-row">
          <h1>Geçmiş</h1>
          <div class="actions">
            <input class="search-input" type="text" placeholder="Geçmişte ara..." oninput="filterRows(this.value)">
            <button class="btn-danger" onclick="sendCmd('clear_history')">Geçmişi Temizle</button>
          </div>
        </div>
        <div class="card">{rows}</div>
        "###
    );
    wrap_shell("history", "Geçmiş — AdBlock Tarayıcı", &body)
}

fn render_bookmarks(storage: &BrowserStorage) -> String {
    let mut rows = String::new();
    for b in &storage.bookmarks {
        rows.push_str(&format!(
            r###"<div class="row" data-search="{} {}">
                <div class="row-info">
                  <div class="row-title"><a href="javascript:void(0)" style="color:#8ab4f8;text-decoration:none;" onclick="sendCmd('navigate', {{url:'{}'}})">{}</a></div>
                  <div class="row-sub">{}</div>
                </div>
                <div class="actions">
                  <button class="btn-danger" onclick="sendCmd('bm_del', {{url:'{}'}})">Kaldır</button>
                </div>
              </div>"###,
            attr(&b.title),
            attr(&b.url),
            attr(&b.url),
            txt(&b.title),
            txt(&b.url),
            attr(&b.url)
        ));
    }
    if rows.is_empty() {
        rows = r###"<div class="empty">Henüz kayıtlı yer imi yok.</div>"###.to_string();
    }
    let body = format!(
        r###"
        <div class="header-row">
          <h1>Yer İmleri</h1>
          <input class="search-input" type="text" placeholder="Yer imlerinde ara..." oninput="filterRows(this.value)">
        </div>
        <div class="card">
          <div class="card-title">Yeni Yer İmi Ekle</div>
          <form class="row" onsubmit="event.preventDefault(); sendCmd('bm_add', {{title:this.t.value, url:this.u.value}});">
            <input name="t" type="text" placeholder="Başlık" style="width:200px;">
            <input name="u" type="text" placeholder="https://..." style="flex:1;" required>
            <button type="submit" class="btn-primary">Ekle</button>
          </form>
        </div>
        <div class="card">{rows}</div>
        "###
    );
    wrap_shell("bookmarks", "Yer İmleri — AdBlock Tarayıcı", &body)
}

fn render_adblock(ab: &AdBlockService) -> String {
    let mut wl_rows = String::new();
    let mut domains: Vec<&String> = ab.whitelisted_domains.iter().collect();
    domains.sort();
    for d in domains {
        wl_rows.push_str(&format!(
            r###"<div class="row"><div class="row-info"><div class="row-title">{}</div></div>
               <button class="btn-danger" onclick="sendCmd('ab_wl_del', {{host:'{}'}})">Kaldır</button></div>"###,
            txt(d),
            attr(d)
        ));
    }
    if wl_rows.is_empty() {
        wl_rows = r###"<div class="empty">Beyaz listede domain bulunmuyor.</div>"###.to_string();
    }

    let mut list_rows = String::new();
    for item in BUILTIN_FILTER_LISTS {
        let p = ab.filter_dir.join(format!("{}.txt", item.id));
        let (status, color) = if p.exists() {
            ("İndirildi", "#81c995")
        } else {
            ("Dahili/Hazır", "#9aa0a6")
        };
        list_rows.push_str(&format!(
            r###"<div class="row">
                <div class="row-info"><div class="row-title">{}</div><div class="row-sub">{}</div></div>
                <span style="color:{color}; font-size:12.5px; font-weight:600;">{status}</span>
               </div>"###,
            txt(item.name),
            txt(item.url)
        ));
    }

    let body = format!(
        r###"
        <div class="header-row">
          <h1>AdBlock Kalkanı ve Filtre Yönetimi</h1>
          <button class="btn-primary" onclick="sendCmd('ab_update_lists')">Filtre Listelerini Güncelle</button>
        </div>
        <div class="card">
          <div class="card-title">Özel Filtre Kuralları (custom_filters.txt)</div>
          <div style="padding:14px 18px; display:flex; flex-direction:column; gap:12px;">
            <textarea id="custom-rules" rows="7" style="width:100%; font-family:monospace;">{}</textarea>
            <div class="actions" style="justify-content:flex-end;">
              <button class="btn-primary" onclick="sendCmd('ab_save_rules', {{rules:document.getElementById('custom-rules').value}})">Özel Kuralları Kaydet</button>
            </div>
          </div>
        </div>
        <div class="card">
          <div class="card-title">Beyaz Liste (Reklam Engelleme Duraklatılan Siteler)</div>
          <form class="row" onsubmit="event.preventDefault(); sendCmd('ab_wl_add', {{host:this.h.value}});">
            <input name="h" type="text" placeholder="Örnek: youtube.com" style="flex:1;" required>
            <button type="submit" class="btn-primary">Beyaz Listeye Ekle</button>
          </form>
          {wl_rows}
        </div>
        <div class="card">
          <div class="card-title">Dahili Filtre Listeleri ({} aktif kural)</div>
          {list_rows}
        </div>
        "###,
        txt(&ab.custom_rules_text),
        ab.total_rules_count
    );
    wrap_shell("adblock", "AdBlock Yönetimi — AdBlock Tarayıcı", &body)
}

fn render_updates() -> String {
    let entries = crate::updater::built_in_changelog();
    let mut cards = String::new();
    for (idx, e) in entries.iter().enumerate() {
        let mut lis = String::new();
        for ch in &e.changes {
            lis.push_str(&format!(
                "<li style='margin:6px 0; color:#d5dce6; font-size:13px; line-height:1.45;'>{}</li>",
                txt(ch)
            ));
        }
        let badge = if idx == 0 {
            "<span class='pill' style='background:rgba(129,201,149,.16); color:#81c995;'>Mevcut Sürüm</span>"
        } else {
            ""
        };
        cards.push_str(&format!(
            r###"<div class="card">
              <div class="row" style="justify-content:space-between;">
                <div style="font-size:15px; font-weight:600;">v{} — {}</div>
                <div style="display:flex; gap:8px; align-items:center;">{badge}<span style="color:#9aa0a6; font-size:12px;">{}</span></div>
              </div>
              <ul style="padding:10px 28px 14px; margin:0;">{lis}</ul>
            </div>"###,
            txt(&e.version),
            txt(&e.title),
            txt(&e.date)
        ));
    }

    let body = format!(
        r###"
        <div class="header-row">
          <h1>Sürüm Kaydı &amp; Değişiklikler (v{})</h1>
        </div>
        {cards}
        "###,
        txt(crate::updater::APP_VERSION)
    );
    wrap_shell("updates", "Sürüm Geçmişi — AdBlock Tarayıcı", &body)
}

pub fn render_security_interstitial(host: &str, target_url: &str, reason: &str) -> String {
    format!(
        r###"<!DOCTYPE html>
<html lang="tr"><head><meta charset="utf-8"><title>Güvenlik Uyarısı</title>
<style>
  body {{ margin:0; background:#202124; color:#e8eaed; font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; display:flex; justify-content:center; align-items:center; min-height:100vh; }}
  .box {{ max-width:560px; background:#292a2d; border:1px solid #5f3c3a; border-radius:16px; padding:28px; }}
  h1 {{ color:#f28b82; font-size:20px; margin:12px 0 8px; }}
  p {{ color:#bdc1c6; font-size:13.5px; line-height:1.5; }}
  .actions {{ display:flex; gap:10px; margin-top:20px; }}
  button {{ border-radius:18px; padding:8px 18px; font-size:13px; cursor:pointer; }}
  .safe {{ background:#8ab4f8; color:#202124; border:none; font-weight:600; }}
  .risk {{ background:transparent; color:#f28b82; border:1px solid rgba(242,139,130,0.5); }}
</style></head>
<body><div class="box">
  <svg viewBox="0 0 24 24" width="38" height="38" fill="none" stroke="#f28b82" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
  <h1>Güvenli Olmayan Bağlantı / DNS Engeli</h1>
  <p><strong>{}</strong> adresine erişim güvenlik kalkanı tarafından durduruldu.</p>
  <p>Neden: {}</p>
  <div class="actions">
    <button class="safe" onclick="window.ipc && window.ipc.postMessage(JSON.stringify({{type:'internal_cmd', cmd:'open_internal', route:'home'}}))">Güvenli Sayfaya Dön</button>
    <button class="risk" onclick="window.ipc && window.ipc.postMessage(JSON.stringify({{type:'internal_cmd', cmd:'allow_http_once', host:'{}', url:'{}'}}))">Yine de Devam Et</button>
  </div>
</div></body></html>"###,
        txt(host),
        txt(reason),
        attr(host),
        attr(target_url)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_internal_routes_and_interstitial() {
        let tmp = std::env::temp_dir().join(format!("ab_test_pages_{}", std::process::id()));
        let storage = BrowserStorage::load(tmp.clone());
        let ab = AdBlockService::load(&tmp, true, true, 5, vec![]);
        let pw = PasswordManager::load(&tmp);

        for route in [
            "home",
            "settings",
            "downloads",
            "passwords",
            "history",
            "bookmarks",
            "adblock",
            "updates",
        ] {
            let html = render_route(route, &storage, &ab, &pw);
            assert!(html.contains("AdBlock Tarayıcı"), "route {route} failed");
        }

        let home_html = render_route("home", &storage, &ab, &pw);
        assert!(!home_html.contains("Toplam Engellenen"));
        assert!(!home_html.contains("Aktif Kural"));
        assert!(!home_html.contains("Güvenlik Kalkanı"));
        assert!(home_html.contains("tile-preview"));
        assert!(home_html.contains("https://image.thum.io/get/width/640/crop/400/noanimate/https://www.google.com"));

        assert!(is_public_http_url("https://www.google.com"));
        assert!(!is_public_http_url("http://localhost:8080"));
        assert!(!is_public_http_url("adblock://settings"));

        let settings_html = render_route("settings", &storage, &ab, &pw);
        assert!(settings_html.contains("Tarama Verilerini Temizle"));
        assert!(settings_html.contains("Varsayılan Sayfa Yakınlaştırma"));
        assert!(settings_html.contains("Başlangıçta"));
        assert!(settings_html.contains("İndirme Klasörü"));
        assert!(settings_html.contains("Ayarları Sıfırla"));

        let sec = render_security_interstitial("<bad>.example", "http://bad.example", "DNS block");
        assert!(sec.contains("&lt;bad&gt;.example"));
        assert!(sec.contains("Güvenli Sayfaya Dön"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
