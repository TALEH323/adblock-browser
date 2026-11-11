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
  main.home-main {{ max-width: 860px; margin: 0 auto; }}
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
  .hero {{ text-align: center; padding: 48px 16px 28px; }}
  .hero h2 {{ font-size: 30px; margin: 12px 0 8px; }}
  .hero-search {{ display: flex; gap: 8px; max-width: 580px; margin: 22px auto; }}
  .hero-search input {{ flex: 1; border-radius: 24px; padding: 12px 18px; font-size: 14.5px; }}
  .grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 12px; margin-top: 20px; }}
  .tile {{
    background: var(--surface); border: 1px solid var(--border); border-radius: 14px;
    padding: 14px; cursor: pointer; text-align: left; color: var(--text);
  }}
  .tile:hover {{ border-color: var(--blue); background: #303236; }}
  .stat-strip {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; margin-top: 24px; }}
  .stat-box {{ background: var(--surface); border: 1px solid var(--border); border-radius: 14px; padding: 16px; text-align: center; }}
  .stat-val {{ font-size: 24px; font-weight: 700; color: var(--green); }}
  .stat-lbl {{ font-size: 12px; color: var(--sub); margin-top: 4px; }}
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

pub fn render_route(
    route: &str,
    storage: &BrowserStorage,
    ab: &AdBlockService,
    pw: &PasswordManager,
) -> String {
    match route {
        "home" => render_home(storage, ab),
        "settings" => render_settings(storage),
        "downloads" => render_downloads(storage),
        "passwords" => render_passwords(storage, pw),
        "history" => render_history(storage),
        "bookmarks" => render_bookmarks(storage),
        "adblock" => render_adblock(ab),
        "updates" => render_updates(),
        _ => render_home(storage, ab),
    }
}

fn render_home(storage: &BrowserStorage, ab: &AdBlockService) -> String {
    let mut tiles = String::new();
    for b in storage.bookmarks.iter().take(8) {
        tiles.push_str(&format!(
            r###"<button class="tile" onclick="sendCmd('navigate', {{url:'{}'}})">
                <div class="row-title">{}</div>
                <div class="row-sub">{}</div>
            </button>"###,
            attr(&b.url),
            txt(&b.title),
            txt(&b.url)
        ));
    }

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
        <div class="stat-strip">
          <div class="stat-box"><div class="stat-val">{}</div><div class="stat-lbl">Toplam Engellenen</div></div>
          <div class="stat-box"><div class="stat-val" style="color:#8ab4f8;">{}</div><div class="stat-lbl">Aktif Kural</div></div>
          <div class="stat-box"><div class="stat-val" style="color:#d2a8ff;">{}</div><div class="stat-lbl">Güvenlik Kalkanı</div></div>
        </div>
        <div class="grid">{tiles}</div>
        "###,
        ab.total_blocked,
        ab.total_rules_count,
        if storage.settings.dns_protection { "HTTPS + DoH" } else { "Standart" }
    );
    wrap_shell("home", "Yeni Sekme — AdBlock Tarayıcı", &body)
}

fn render_settings(storage: &BrowserStorage) -> String {
    let s = &storage.settings;
    let chk = |b: bool| if b { "checked" } else { "" };
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

    let body = format!(
        r###"
        <div class="header-row"><h1>Tarayıcı Ayarları</h1></div>
        <div class="card">
          <div class="card-title">Gizlilik, Güvenlik ve Engelleme</div>
          <div class="row">
            <div class="row-info"><div class="row-title">Reklam & İzleyici Engelleme</div><div class="row-sub">Genel AdBlock Plus motorunu aç/kapat</div></div>
            <input type="checkbox" {} onchange="sendCmd('set_setting', {{key:'adblock_enabled', val:this.checked}})">
          </div>
          <div class="row">
            <div class="row-info"><div class="row-title">Kozmetik & YouTube Filtreleme</div><div class="row-sub">Reklam kutularını gizle ve video reklamlarını atla</div></div>
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
          <div class="row">
            <div class="row-info"><div class="row-title">Yer İmleri Çubuğunu Göster</div></div>
            <input type="checkbox" {} onchange="sendCmd('set_setting', {{key:'show_bookmarks_bar', val:this.checked}})">
          </div>
        </div>
        <div class="card">
          <div class="card-title">Arama Motoru & Ana Sayfa</div>
          <div class="row">
            <div class="row-info"><div class="row-title">Varsayılan Arama Motoru</div></div>
            <select onchange="sendCmd('set_setting', {{key:'search_engine', val:this.value}})">
              <option value="https://www.google.com/search?q=" {}>Google</option>
              <option value="https://duckduckgo.com/?q=" {}>DuckDuckGo</option>
              <option value="https://search.brave.com/search?q=" {}>Brave Search</option>
            </select>
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
        if s.search_engine.contains("google") { "selected" } else { "" },
        if s.search_engine.contains("duckduckgo") { "selected" } else { "" },
        if s.search_engine.contains("brave") { "selected" } else { "" },
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

        let sec = render_security_interstitial("<bad>.example", "http://bad.example", "DNS block");
        assert!(sec.contains("&lt;bad&gt;.example"));
        assert!(sec.contains("Güvenli Sayfaya Dön"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
