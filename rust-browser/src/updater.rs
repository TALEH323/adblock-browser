use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_GITHUB_REPO: &str = "TALEH323/adblock-browser";

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub notes: String,
    #[allow(dead_code)]
    pub html_url: String,
    pub asset_name: Option<String>,
    pub asset_browser_url: Option<String>,
    pub asset_api_url: Option<String>,
    pub asset_size: u64,
}

#[derive(Debug, Clone)]
pub enum UpdateCheckResult {
    UpToDate { current: String, latest: String },
    UpdateAvailable(ReleaseInfo),
    Unavailable { current: String, reason: String },
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CustomManifest {
    version: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    windows_url: String,
    #[serde(default)]
    macos_url: String,
    #[serde(default)]
    windows_name: String,
    #[serde(default)]
    macos_name: String,
}

pub fn normalize_version(raw: &str) -> (u64, u64, u64) {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let core = trimmed.split(['-', '+']).next().unwrap_or("0");
    let mut parts = core.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    normalize_version(candidate) > normalize_version(current)
}

pub fn check_for_updates(manifest_url: &str, token: &str) -> UpdateCheckResult {
    let current = APP_VERSION.to_string();
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(6))
        .build();

    let custom_url = manifest_url.trim();
    if !custom_url.is_empty() {
        let mut req = agent
            .get(custom_url)
            .set("User-Agent", &format!("AdBlockBrowser/{current}"))
            .set("Accept", "application/json");
        if !token.trim().is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", token.trim()));
        }
        return match req.call() {
            Ok(resp) => match resp.into_string() {
                Ok(body) => parse_custom_manifest(&body, &current),
                Err(e) => UpdateCheckResult::Unavailable {
                    current,
                    reason: format!("Yanıt okunamadı: {e}"),
                },
            },
            Err(e) => UpdateCheckResult::Unavailable {
                current,
                reason: format!("Sunucuya erişilemedi ({e})"),
            },
        };
    }

    let api_url = format!("https://api.github.com/repos/{DEFAULT_GITHUB_REPO}/releases/latest");
    let mut req = agent
        .get(&api_url)
        .set("User-Agent", &format!("AdBlockBrowser/{current}"))
        .set("Accept", "application/vnd.github+json");
    let eff_token = if !token.trim().is_empty() {
        token.trim().to_string()
    } else {
        std::env::var("ADBLOCK_UPDATE_TOKEN").unwrap_or_default()
    };
    if !eff_token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {eff_token}"));
    }

    match req.call() {
        Ok(resp) => match resp.into_string() {
            Ok(body) => parse_github_release(&body, &current),
            Err(e) => UpdateCheckResult::Unavailable {
                current,
                reason: format!("Yanıt okunamadı: {e}"),
            },
        },
        Err(ureq::Error::Status(404, _)) => UpdateCheckResult::Unavailable {
            current,
            reason: "Yayınlanmış sürüm bulunamadı (veya repo özel).".to_string(),
        },
        Err(ureq::Error::Status(code, _)) => UpdateCheckResult::Unavailable {
            current,
            reason: format!("Sürüm sunucusu yanıtı: HTTP {code}"),
        },
        Err(_) => UpdateCheckResult::Unavailable {
            current,
            reason: "Sürüm sunucusuna erişilemedi (çevrimdışı).".to_string(),
        },
    }
}

pub fn parse_custom_manifest(json_str: &str, current: &str) -> UpdateCheckResult {
    let Ok(m) = serde_json::from_str::<CustomManifest>(json_str) else {
        return UpdateCheckResult::Unavailable {
            current: current.to_string(),
            reason: "Manifest JSON ayrıştırılamadı.".to_string(),
        };
    };
    let ver = m.version.trim().trim_start_matches(['v', 'V']).to_string();
    if !is_newer_version(&ver, current) {
        return UpdateCheckResult::UpToDate {
            current: current.to_string(),
            latest: ver,
        };
    }

    #[cfg(target_os = "windows")]
    let (dl_url, dl_name) = (
        (!m.windows_url.trim().is_empty()).then(|| m.windows_url.trim().to_string()),
        (!m.windows_name.trim().is_empty())
            .then(|| m.windows_name.trim().to_string())
            .or_else(|| Some(format!("AdBlockBrowser-Setup-{ver}-x64.exe"))),
    );
    #[cfg(target_os = "macos")]
    let (dl_url, dl_name) = (
        (!m.macos_url.trim().is_empty()).then(|| m.macos_url.trim().to_string()),
        (!m.macos_name.trim().is_empty())
            .then(|| m.macos_name.trim().to_string())
            .or_else(|| Some(format!("AdBlockBrowser-{ver}-macOS.dmg"))),
    );
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let (dl_url, dl_name): (Option<String>, Option<String>) = (None, None);

    UpdateCheckResult::UpdateAvailable(ReleaseInfo {
        version: ver,
        notes: m.notes,
        html_url: m.html_url,
        asset_name: dl_name,
        asset_browser_url: dl_url,
        asset_api_url: None,
        asset_size: 0,
    })
}

pub fn parse_github_release(json_str: &str, current: &str) -> UpdateCheckResult {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) else {
        return UpdateCheckResult::Unavailable {
            current: current.to_string(),
            reason: "GitHub yanıtı çözümlenemedi.".to_string(),
        };
    };
    let tag = val
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    if tag.is_empty() {
        return UpdateCheckResult::Unavailable {
            current: current.to_string(),
            reason: "Sürüm etiketi bulunamadı.".to_string(),
        };
    }
    if !is_newer_version(&tag, current) {
        return UpdateCheckResult::UpToDate {
            current: current.to_string(),
            latest: tag,
        };
    }

    let notes = val
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .chars()
        .take(500)
        .collect();
    let html_url = val
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let assets = val
        .get("assets")
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let selected = select_platform_asset(assets);

    UpdateCheckResult::UpdateAvailable(ReleaseInfo {
        version: tag,
        notes,
        html_url,
        asset_name: selected.as_ref().map(|a| a.0.clone()),
        asset_browser_url: selected.as_ref().map(|a| a.1.clone()),
        asset_api_url: selected.as_ref().map(|a| a.2.clone()),
        asset_size: selected.as_ref().map(|a| a.3).unwrap_or(0),
    })
}

fn select_platform_asset(assets: &[serde_json::Value]) -> Option<(String, String, String, u64)> {
    let parsed: Vec<(String, String, String, u64)> = assets
        .iter()
        .filter_map(|a| {
            let name = a.get("name")?.as_str()?.to_string();
            let b_url = a.get("browser_download_url")?.as_str()?.to_string();
            let api_url = a
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let size = a.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            Some((name, b_url, api_url, size))
        })
        .collect();

    #[cfg(target_os = "windows")]
    {
        if let Some(a) = parsed.iter().find(|(n, ..)| {
            let l = n.to_lowercase();
            l.ends_with(".exe") && l.contains("setup")
        }) {
            return Some(a.clone());
        }
        if let Some(a) = parsed.iter().find(|(n, ..)| n.to_lowercase().ends_with(".exe")) {
            return Some(a.clone());
        }
        if let Some(a) = parsed.iter().find(|(n, ..)| {
            let l = n.to_lowercase();
            l.ends_with(".zip") && l.contains("windows")
        }) {
            return Some(a.clone());
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(a) = parsed.iter().find(|(n, ..)| n.to_lowercase().ends_with(".dmg")) {
            return Some(a.clone());
        }
        if let Some(a) = parsed.iter().find(|(n, ..)| {
            let l = n.to_lowercase();
            l.ends_with(".zip") && l.contains("macos")
        }) {
            return Some(a.clone());
        }
    }

    parsed.into_iter().next()
}

pub fn download_and_install_update<F>(
    info: &ReleaseInfo,
    token: &str,
    mut on_progress: F,
) -> Result<bool, String>
where
    F: FnMut(u8, &str),
{
    let file_name = info
        .asset_name
        .clone()
        .unwrap_or_else(|| format!("AdBlockBrowser-Update-{}", info.version));
    let dest_path = std::env::temp_dir().join(&file_name);

    let eff_token = if !token.trim().is_empty() {
        token.trim().to_string()
    } else {
        std::env::var("ADBLOCK_UPDATE_TOKEN").unwrap_or_default()
    };

    on_progress(5, "Güncelleme dosyasına bağlanılıyor...");
    let resp = if !eff_token.is_empty() {
        if let Some(api_url) = info.asset_api_url.as_deref().filter(|s| !s.is_empty()) {
            let no_redir = ureq::AgentBuilder::new()
                .redirects(0)
                .timeout_connect(Duration::from_secs(8))
                .build();
            let first = no_redir
                .get(api_url)
                .set("User-Agent", &format!("AdBlockBrowser/{APP_VERSION}"))
                .set("Accept", "application/octet-stream")
                .set("Authorization", &format!("Bearer {eff_token}"))
                .call();
            match first {
                Ok(r) if r.status() >= 300 && r.status() < 400 => {
                    let loc = r
                        .header("Location")
                        .ok_or_else(|| "Yönlendirme adresi alınamadı.".to_string())?
                        .to_string();
                    ureq::AgentBuilder::new()
                        .timeout_connect(Duration::from_secs(8))
                        .build()
                        .get(&loc)
                        .call()
                        .map_err(|e| format!("İndirme hatası: {e}"))?
                }
                Ok(r) => r,
                Err(e) => return Err(format!("GitHub indirme hatası: {e}")),
            }
        } else {
            let url = info
                .asset_browser_url
                .as_deref()
                .ok_or_else(|| "İndirme adresi bulunamadı.".to_string())?;
            ureq::get(url)
                .set("User-Agent", &format!("AdBlockBrowser/{APP_VERSION}"))
                .call()
                .map_err(|e| format!("İndirme hatası: {e}"))?
        }
    } else {
        let url = info
            .asset_browser_url
            .as_deref()
            .ok_or_else(|| "İndirme adresi bulunamadı.".to_string())?;
        ureq::get(url)
            .set("User-Agent", &format!("AdBlockBrowser/{APP_VERSION}"))
            .call()
            .map_err(|e| format!("İndirme hatası: {e}"))?
    };

    let content_len = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(info.asset_size);

    let mut reader = resp.into_reader();
    let mut out = File::create(&dest_path).map_err(|e| format!("Dosya yazılamadı: {e}"))?;
    let mut buf = [0u8; 32 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_pct: u8 = 5;

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Okuma hatası: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|e| format!("Disk yazma hatası: {e}"))?;
        downloaded += n as u64;
        let pct = if content_len > 0 {
            ((downloaded * 90) / content_len).min(95) as u8 + 5
        } else {
            50
        };
        if pct != last_pct {
            last_pct = pct;
            let mb = downloaded as f64 / (1024.0 * 1024.0);
            on_progress(pct, &format!("İndiriliyor... {mb:.1} MB"));
        }
    }
    let _ = out.flush();
    on_progress(98, "Kurulum başlatılıyor...");

    apply_downloaded_package(&dest_path)
}

fn apply_downloaded_package(pkg_path: &Path) -> Result<bool, String> {
    let lower = pkg_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    #[cfg(target_os = "windows")]
    {
        if lower.ends_with(".exe") {
            Command::new(pkg_path)
                .spawn()
                .map_err(|e| format!("Kurulum çalıştırılamadı: {e}"))?;
            return Ok(true);
        }
        let _ = Command::new("explorer")
            .arg(format!("/select,{}", pkg_path.display()))
            .spawn();
        return Ok(false);
    }

    #[cfg(target_os = "macos")]
    {
        if lower.ends_with(".dmg") {
            if try_install_macos_dmg(pkg_path).unwrap_or(false) {
                return Ok(true);
            }
            Command::new("open")
                .arg(pkg_path)
                .spawn()
                .map_err(|e| format!("DMG açılamadı: {e}"))?;
            return Ok(true);
        }
        let _ = Command::new("open").arg("-R").arg(pkg_path).spawn();
        return Ok(false);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = pkg_path;
        Ok(false)
    }
}

#[cfg(target_os = "macos")]
fn try_install_macos_dmg(dmg_path: &Path) -> Result<bool, String> {
    // Only replace cleanly when running inside an installed `.app` bundle.
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let current_app = exe
        .ancestors()
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("app"))
        .map(PathBuf::from);
    let Some(target_app) = current_app else {
        return Ok(false);
    };

    let mount_dir = std::env::temp_dir().join(format!("ab_upd_{}", std::process::id()));
    let _ = fs::create_dir_all(&mount_dir);

    let attach = Command::new("hdiutil")
        .args(["attach", "-readonly", "-nobrowse", "-mountpoint"])
        .arg(&mount_dir)
        .arg(dmg_path)
        .output()
        .map_err(|e| e.to_string())?;
    if !attach.status.success() {
        return Ok(false);
    }

    let mut installed = false;
    if let Ok(entries) = fs::read_dir(&mount_dir) {
        if let Some(src_app) = entries
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|e| e.to_str()) == Some("app"))
        {
            let stage = target_app.with_extension("app.new");
            let _ = fs::remove_dir_all(&stage);
            let copy_ok = Command::new("ditto")
                .arg(&src_app)
                .arg(&stage)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if copy_ok {
                let backup = target_app.with_extension("app.old");
                let _ = fs::remove_dir_all(&backup);
                if fs::rename(&target_app, &backup).is_ok() {
                    if fs::rename(&stage, &target_app).is_ok() {
                        let _ = fs::remove_dir_all(&backup);
                        let _ = Command::new("open").arg("-n").arg(&target_app).spawn();
                        installed = true;
                    } else {
                        let _ = fs::rename(&backup, &target_app);
                    }
                }
            }
            let _ = fs::remove_dir_all(&stage);
        }
    }

    let _ = Command::new("hdiutil")
        .arg("detach")
        .arg(&mount_dir)
        .arg("-force")
        .output();
    let _ = fs::remove_dir_all(&mount_dir);
    Ok(installed)
}

pub fn launcher_html(auto_launch_if_current: bool, manifest_url: &str, has_token: bool) -> String {
    let icon_b64 = B64.encode(include_bytes!("../assets/icon.png"));
    let checked_attr = if auto_launch_if_current { "checked" } else { "" };
    let manifest_esc = html_escape::encode_double_quoted_attribute(manifest_url);
    let token_ph = if has_token {
        "•••••••• (Kayıtlı token)"
    } else {
        "GitHub Token (özel repo için isteğe bağlı)"
    };

    format!(
        r###"<!DOCTYPE html>
<html lang="tr">
<head>
<meta charset="utf-8">
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; user-select: none; -webkit-user-select: none; }}
  html, body {{
    width: 100%; height: 100%; overflow: hidden;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    color: #f1f5f9;
    background: radial-gradient(circle at 22% 18%, rgba(74, 168, 255, 0.16), transparent 45%),
                radial-gradient(circle at 82% 78%, rgba(78, 224, 164, 0.14), transparent 48%),
                linear-gradient(155deg, #10151f 0%, #0b0f17 100%);
  }}
  .wrap {{
    height: 100%; display: flex; flex-direction: column; justify-content: space-between;
    padding: 28px 30px 22px;
  }}
  .top {{ display: flex; align-items: center; gap: 16px; }}
  .logo {{
    width: 64px; height: 64px; border-radius: 16px;
    filter: drop-shadow(0 8px 20px rgba(74, 174, 255, 0.28));
  }}
  .brand h1 {{ font-size: 20px; font-weight: 700; letter-spacing: -0.2px; }}
  .brand .ver {{
    margin-top: 4px; display: inline-flex; align-items: center; gap: 8px;
    font-size: 12px; color: #9aa7ba;
  }}
  .pill {{
    padding: 2px 9px; border-radius: 999px; font-weight: 600; font-size: 11px;
    background: rgba(138, 180, 248, 0.15); color: #9ec5ff;
    border: 1px solid rgba(138, 180, 248, 0.28);
  }}
  .gear-btn {{
    margin-left: auto; width: 32px; height: 32px; border-radius: 9px; border: 1px solid rgba(255,255,255,.1);
    background: rgba(255,255,255,.05); color: #9aa7ba; cursor: pointer; font-size: 15px;
    display: flex; align-items: center; justify-content: center;
  }}
  .gear-btn:hover {{ background: rgba(255,255,255,.12); color: #fff; }}

  .card {{
    background: rgba(255, 255, 255, 0.045);
    border: 1px solid rgba(255, 255, 255, 0.09);
    border-radius: 16px; padding: 16px 18px;
    display: flex; flex-direction: column; gap: 10px;
  }}
  .status-row {{ display: flex; align-items: center; gap: 10px; font-size: 13.5px; font-weight: 600; }}
  .dot {{
    width: 10px; height: 10px; border-radius: 50%; flex-shrink: 0;
    background: #8ab4f8; box-shadow: 0 0 10px rgba(138, 180, 248, 0.8);
    animation: pulse 1.2s infinite ease-in-out;
  }}
  .dot.ok {{ background: #52e0a1; box-shadow: 0 0 10px rgba(82, 224, 161, 0.7); animation: none; }}
  .dot.upd {{ background: #60a5fa; box-shadow: 0 0 12px rgba(96, 165, 250, 0.9); animation: none; }}
  .dot.warn {{ background: #fbbf24; box-shadow: 0 0 8px rgba(251, 191, 36, 0.5); animation: none; }}
  @keyframes pulse {{ 0%, 100% {{ opacity: .45; transform: scale(.9); }} 50% {{ opacity: 1; transform: scale(1.1); }} }}

  .sub-msg {{ font-size: 12.2px; color: #9aa7ba; line-height: 1.45; max-height: 74px; overflow-y: auto; }}
  .bar-bg {{
    width: 100%; height: 6px; border-radius: 999px;
    background: rgba(255,255,255,0.08); overflow: hidden;
  }}
  .bar-fill {{
    height: 100%; width: 22%; border-radius: 999px;
    background: linear-gradient(90deg, #56b2ff, #52e0a1);
    transition: width .22s ease;
  }}
  .bar-fill.indet {{ width: 35%; animation: slide 1.1s infinite ease-in-out; }}
  @keyframes slide {{ 0% {{ transform: translateX(-110%); }} 100% {{ transform: translateX(300%); }} }}

  .cfg {{
    display: none; flex-direction: column; gap: 8px;
    background: rgba(0,0,0,0.28); border: 1px solid rgba(255,255,255,.08);
    border-radius: 12px; padding: 11px 13px;
  }}
  .cfg.open {{ display: flex; }}
  .cfg input {{
    width: 100%; padding: 7px 10px; border-radius: 8px; font-size: 12px;
    background: rgba(255,255,255,.06); border: 1px solid rgba(255,255,255,.12);
    color: #f1f5f9; outline: none; user-select: text; -webkit-user-select: text;
  }}
  .cfg-row {{ display: flex; justify-content: flex-end; gap: 8px; }}

  .bottom {{ display: flex; flex-direction: column; gap: 12px; }}
  .actions {{ display: flex; gap: 10px; align-items: center; }}
  .btn {{
    border: none; border-radius: 12px; padding: 11px 16px; font-size: 13.5px; font-weight: 600;
    cursor: pointer; transition: transform .08s ease, opacity .15s ease;
  }}
  .btn:active {{ transform: scale(0.98); }}
  .btn:disabled {{ opacity: 0.45; cursor: not-allowed; }}
  .btn-primary {{
    flex: 1; color: #08121b;
    background: linear-gradient(135deg, #6bbdff 0%, #5ce2ab 100%);
    box-shadow: 0 6px 20px rgba(92, 226, 171, 0.22);
  }}
  .btn-secondary {{
    background: rgba(255,255,255,.08); color: #e2e8f0;
    border: 1px solid rgba(255,255,255,.1);
  }}
  .btn-secondary:hover {{ background: rgba(255,255,255,.14); }}
  .btn-sm {{ padding: 6px 12px; font-size: 12px; border-radius: 8px; }}

  .opt-row {{
    display: flex; align-items: center; justify-content: space-between;
    font-size: 12px; color: #8c9bb0;
  }}
  .opt-row label {{ display: inline-flex; align-items: center; gap: 7px; cursor: pointer; }}
  .opt-row input[type="checkbox"] {{ accent-color: #5ce2ab; width: 14px; height: 14px; }}
</style>
</head>
<body>
<div class="wrap">
  <div class="top">
    <img class="logo" src="data:image/png;base64,{icon_b64}" alt="Logo">
    <div class="brand">
      <h1>AdBlock Tarayıcı</h1>
      <div class="ver">
        <span class="pill">v{APP_VERSION}</span>
        <span>Başlatıcı ve Sürüm Yöneticisi</span>
      </div>
    </div>
    <button class="gear-btn" title="Güncelleme Kaynağı Ayarları" onclick="document.getElementById('cfg').classList.toggle('open')">⚙</button>
  </div>

  <div class="card">
    <div class="status-row">
      <div id="dot" class="dot"></div>
      <div id="st-title">Güncellemeler kontrol ediliyor...</div>
    </div>
    <div id="st-sub" class="sub-msg">GitHub Releases üzerinden en güncel sürüm sorgulanıyor.</div>
    <div class="bar-bg"><div id="bar" class="bar-fill indet"></div></div>
  </div>

  <div id="cfg" class="cfg">
    <input id="cfg-url" type="text" value="{manifest_esc}" placeholder="Özel version.json URL (isteğe bağlı)">
    <input id="cfg-tok" type="password" placeholder="{token_ph}">
    <div class="cfg-row">
      <button class="btn btn-secondary btn-sm" onclick="saveUpdateCfg()">Kaydet ve Kontrol Et</button>
    </div>
  </div>

  <div class="bottom">
    <div class="actions">
      <button id="btn-recheck" class="btn btn-secondary" onclick="sendL('launcher_recheck')">Kontrol Et</button>
      <button id="btn-update" class="btn btn-primary" style="display:none;" onclick="startUpdate()">Şimdi Güncelle</button>
      <button id="btn-launch" class="btn btn-primary" onclick="sendL('launcher_launch')">Tarayıcıyı Başlat</button>
    </div>
    <div class="opt-row">
      <label><input id="chk-auto" type="checkbox" {checked_attr} onchange="sendL('launcher_set_auto', {{enabled:this.checked}})"> Güncelse tarayıcıyı otomatik aç</label>
      <span>AdBlock Engine • DoH • AES-256</span>
    </div>
  </div>
</div>
<script>
  function sendL(cmd, extra) {{
    if (window.ipc) {{
      window.ipc.postMessage(JSON.stringify({{ type: 'internal_cmd', cmd, ...(extra || {{}}) }}));
    }}
  }}
  function startUpdate() {{
    document.getElementById('btn-update').disabled = true;
    document.getElementById('btn-recheck').disabled = true;
    sendL('launcher_start_update');
  }}
  function saveUpdateCfg() {{
    sendL('launcher_save_config', {{
      url: document.getElementById('cfg-url').value,
      rules: document.getElementById('cfg-tok').value
    }});
    document.getElementById('cfg').classList.remove('open');
  }}
  window.__setLauncherState = (st) => {{
    const dot = document.getElementById('dot');
    const title = document.getElementById('st-title');
    const sub = document.getElementById('st-sub');
    const bar = document.getElementById('bar');
    const btnUpd = document.getElementById('btn-update');
    const btnLaunch = document.getElementById('btn-launch');
    const btnRecheck = document.getElementById('btn-recheck');

    dot.className = 'dot ' + (st.dot || '');
    title.textContent = st.title || '';
    sub.textContent = st.sub || '';
    if (st.indeterminate) {{
      bar.className = 'bar-fill indet';
    }} else {{
      bar.className = 'bar-fill';
      bar.style.width = (st.progress ?? 100) + '%';
    }}
    if (st.can_update) {{
      btnUpd.style.display = 'block';
      btnUpd.disabled = !!st.updating;
      btnUpd.textContent = st.update_label || 'Şimdi Güncelle';
      btnLaunch.className = 'btn btn-secondary';
    }} else {{
      btnUpd.style.display = 'none';
      btnLaunch.className = 'btn btn-primary';
    }}
    btnRecheck.disabled = !!st.checking || !!st.updating;
    btnLaunch.disabled = !!st.updating;
  }};
  window.addEventListener('keydown', (e) => {{
    if (e.key === 'Enter' && !document.getElementById('btn-launch').disabled) {{
      sendL('launcher_launch');
    }}
  }});
</script>
</body>
</html>"###
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_cleanly() {
        assert!(is_newer_version("0.2.0", "0.1.0"));
        assert!(is_newer_version("v1.0.0", "0.9.9"));
        assert!(is_newer_version("0.1.1", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.0.9", "0.1.0"));
    }

    #[test]
    fn parses_github_release_and_picks_platform_asset() {
        let json = r#"{
          "tag_name": "v0.2.0",
          "body": "Chrome sifre ice aktarma ve launcher.",
          "html_url": "https://github.com/TALEH323/adblock-browser/releases/tag/v0.2.0",
          "assets": [
            {
              "name": "AdBlockBrowser-Setup-0.2.0-x64.exe",
              "browser_download_url": "https://example.com/setup.exe",
              "url": "https://api.github.com/repos/x/y/releases/assets/1",
              "size": 3800000
            },
            {
              "name": "AdBlockBrowser-0.2.0-macOS.dmg",
              "browser_download_url": "https://example.com/app.dmg",
              "url": "https://api.github.com/repos/x/y/releases/assets/2",
              "size": 6700000
            }
          ]
        }"#;

        match parse_github_release(json, "0.1.0") {
            UpdateCheckResult::UpdateAvailable(info) => {
                assert_eq!(info.version, "0.2.0");
                #[cfg(target_os = "macos")]
                assert_eq!(info.asset_name.as_deref(), Some("AdBlockBrowser-0.2.0-macOS.dmg"));
                #[cfg(target_os = "windows")]
                assert_eq!(info.asset_name.as_deref(), Some("AdBlockBrowser-Setup-0.2.0-x64.exe"));
            }
            other => panic!("expected UpdateAvailable, got {other:?}"),
        }
    }

    #[test]
    fn renders_launcher_html_cleanly() {
        let html = launcher_html(true, "", false);
        assert!(html.contains("AdBlock Tarayıcı"));
        assert!(html.contains("launcher_launch"));
        assert!(html.contains("data:image/png;base64,"));
    }
}
