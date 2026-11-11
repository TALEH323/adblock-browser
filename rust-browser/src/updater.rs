use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_GITHUB_REPO: &str = "TALEH323/adblock-browser";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangelogEntry {
    pub version: String,
    pub date: String,
    pub title: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteCommitItem {
    pub sha: String,
    pub message: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteReleaseItem {
    pub version: String,
    pub date: String,
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct LauncherProfileStats {
    pub blocked_total: u64,
    pub password_count: usize,
    pub bookmark_count: usize,
}

pub fn built_in_changelog() -> Vec<ChangelogEntry> {
    serde_json::from_str(include_str!("../CHANGELOG.json")).unwrap_or_default()
}

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

#[derive(Debug, Clone)]
pub struct UpdateCheckBundle {
    pub result: UpdateCheckResult,
    pub remote_releases: Vec<RemoteReleaseItem>,
    pub remote_commits: Vec<RemoteCommitItem>,
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

fn effective_token(token: &str) -> String {
    if !token.trim().is_empty() {
        token.trim().to_string()
    } else {
        std::env::var("ADBLOCK_UPDATE_TOKEN").unwrap_or_default()
    }
}

pub fn check_for_updates_bundle(manifest_url: &str, token: &str) -> UpdateCheckBundle {
    let current = APP_VERSION.to_string();
    let eff_token = effective_token(token);
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
        if !eff_token.is_empty() {
            req = req.set("Authorization", &format!("Bearer {eff_token}"));
        }
        let result = match req.call() {
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
        return UpdateCheckBundle {
            result,
            remote_releases: Vec::new(),
            remote_commits: Vec::new(),
        };
    }

    let api_url = format!("https://api.github.com/repos/{DEFAULT_GITHUB_REPO}/releases/latest");
    let mut req = agent
        .get(&api_url)
        .set("User-Agent", &format!("AdBlockBrowser/{current}"))
        .set("Accept", "application/vnd.github+json");
    if !eff_token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {eff_token}"));
    }

    let result = match req.call() {
        Ok(resp) => match resp.into_string() {
            Ok(body) => parse_github_release(&body, &current),
            Err(e) => UpdateCheckResult::Unavailable {
                current: current.clone(),
                reason: format!("Yanıt okunamadı: {e}"),
            },
        },
        Err(ureq::Error::Status(404, _)) => UpdateCheckResult::Unavailable {
            current: current.clone(),
            reason: if eff_token.is_empty() {
                "Henüz yayınlanmış GitHub Release yok veya repo özel (Token gerekebilir).".to_string()
            } else {
                "Henüz yayınlanmış bir GitHub Release (etiketli sürüm) yok.".to_string()
            },
        },
        Err(ureq::Error::Status(401 | 403, _)) => UpdateCheckResult::Unavailable {
            current: current.clone(),
            reason: "Yetkilendirme veya hız limiti hatası — Ayarlardan Token kontrol edin.".to_string(),
        },
        Err(ureq::Error::Status(code, _)) => UpdateCheckResult::Unavailable {
            current: current.clone(),
            reason: format!("Sürüm sunucusu yanıtı: HTTP {code}"),
        },
        Err(_) => UpdateCheckResult::Unavailable {
            current: current.clone(),
            reason: "Sürüm sunucusuna erişilemedi (çevrimdışı mod).".to_string(),
        },
    };

    let remote_releases = fetch_github_releases(&agent, &eff_token, &current);
    let remote_commits = fetch_github_commits(&agent, &eff_token, &current);

    UpdateCheckBundle {
        result,
        remote_releases,
        remote_commits,
    }
}

fn fetch_github_releases(
    agent: &ureq::Agent,
    token: &str,
    current: &str,
) -> Vec<RemoteReleaseItem> {
    let url = format!("https://api.github.com/repos/{DEFAULT_GITHUB_REPO}/releases?per_page=8");
    let mut req = agent
        .get(&url)
        .set("User-Agent", &format!("AdBlockBrowser/{current}"))
        .set("Accept", "application/vnd.github+json");
    if !token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let Ok(resp) = req.call() else {
        return Vec::new();
    };
    let Ok(body) = resp.into_string() else {
        return Vec::new();
    };
    let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&body) else {
        return Vec::new();
    };
    arr.into_iter()
        .filter_map(|v| {
            let tag = v.get("tag_name")?.as_str()?.trim().trim_start_matches(['v', 'V']).to_string();
            let date = v
                .get("published_at")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .chars()
                .take(10)
                .collect();
            let notes = v
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .chars()
                .take(300)
                .collect();
            Some(RemoteReleaseItem {
                version: tag,
                date,
                notes,
            })
        })
        .collect()
}

fn fetch_github_commits(
    agent: &ureq::Agent,
    token: &str,
    current: &str,
) -> Vec<RemoteCommitItem> {
    let url = format!("https://api.github.com/repos/{DEFAULT_GITHUB_REPO}/commits?per_page=8");
    let mut req = agent
        .get(&url)
        .set("User-Agent", &format!("AdBlockBrowser/{current}"))
        .set("Accept", "application/vnd.github+json");
    if !token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let Ok(resp) = req.call() else {
        return Vec::new();
    };
    let Ok(body) = resp.into_string() else {
        return Vec::new();
    };
    let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&body) else {
        return Vec::new();
    };
    arr.into_iter()
        .filter_map(|v| {
            let sha = v.get("sha")?.as_str()?.chars().take(7).collect::<String>();
            let msg_full = v.get("commit")?.get("message")?.as_str()?;
            let message = msg_full.lines().next().unwrap_or("").trim().to_string();
            let date = v
                .get("commit")?
                .get("author")?
                .get("date")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .chars()
                .take(10)
                .collect();
            Some(RemoteCommitItem { sha, message, date })
        })
        .collect()
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
    let eff_token = effective_token(token);

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

pub fn launcher_html(
    auto_launch_if_current: bool,
    manifest_url: &str,
    has_token: bool,
    stats: &LauncherProfileStats,
) -> String {
    let icon_b64 = B64.encode(include_bytes!("../assets/icon.png"));
    let checked_attr = if auto_launch_if_current { "checked" } else { "" };
    let manifest_esc = html_escape::encode_double_quoted_attribute(manifest_url);
    let token_ph = if has_token {
        "•••••••• (Kayıtlı token mevcut — değiştirmek için yazın)"
    } else {
        "github_pat_... (Özel repo Release ve commit erişimi için)"
    };
    let changelog_json = serde_json::to_string(&built_in_changelog()).unwrap_or_else(|_| "[]".into());

    format!(
        r###"<!DOCTYPE html>
<html lang="tr">
<head>
<meta charset="utf-8">
<style>
  :root {{
    color-scheme: dark;
    --bg: #0b0f17;
    --rail: #0f141e;
    --pane: #131924;
    --card: rgba(255,255,255,0.038);
    --card-border: rgba(255,255,255,0.08);
    --text: #eef2f7;
    --sub: #8b98ac;
    --blue: #8ab4f8;
    --green: #81c995;
    --amber: #fdd663;
  }}
  * {{ box-sizing: border-box; margin: 0; padding: 0; user-select: none; -webkit-user-select: none; }}
  html, body {{
    width: 100%; height: 100%; overflow: hidden;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: var(--bg); color: var(--text);
  }}
  .shell {{
    width: 100%; height: 100%; display: flex;
    background: var(--pane);
    border: 1px solid rgba(255,255,255,0.09);
  }}
  /* Left Studio Rail */
  .rail {{
    width: 236px; flex-shrink: 0;
    background: var(--rail);
    border-right: 1px solid rgba(255,255,255,0.07);
    display: flex; flex-direction: column; justify-content: space-between;
    padding: 18px 14px 16px;
  }}
  .brand {{
    display: flex; align-items: center; gap: 11px; padding: 4px 6px 14px;
    border-bottom: 1px solid rgba(255,255,255,0.06);
  }}
  .brand img {{ width: 40px; height: 40px; border-radius: 10px; }}
  .brand-t {{ font-size: 14.5px; font-weight: 700; letter-spacing: -0.2px; }}
  .brand-v {{
    display: inline-block; margin-top: 3px; font-size: 11px; font-family: ui-monospace, monospace;
    color: var(--blue); background: rgba(138,180,248,0.12);
    border: 1px solid rgba(138,180,248,0.22); border-radius: 6px; padding: 1px 7px;
  }}
  .nav {{ display: flex; flex-direction: column; gap: 4px; margin-top: 14px; }}
  .nav-btn {{
    display: flex; align-items: center; gap: 10px;
    padding: 9px 11px; border-radius: 9px; border: none;
    background: transparent; color: var(--sub); font-size: 12.8px; font-weight: 500;
    cursor: pointer; text-align: left; transition: background .12s, color .12s;
  }}
  .nav-btn svg {{ width: 15px; height: 15px; flex-shrink: 0; opacity: 0.85; }}
  .nav-btn:hover {{ background: rgba(255,255,255,0.05); color: var(--text); }}
  .nav-btn.active {{ background: rgba(138,180,248,0.14); color: #d2e3fc; font-weight: 600; }}

  .stats-box {{
    margin-top: auto; margin-bottom: 12px;
    background: rgba(255,255,255,0.025);
    border: 1px solid rgba(255,255,255,0.06);
    border-radius: 11px; padding: 11px 12px;
    display: flex; flex-direction: column; gap: 7px;
  }}
  .stat-row {{ display: flex; justify-content: space-between; font-size: 11.8px; }}
  .stat-k {{ color: var(--sub); }}
  .stat-v {{ font-weight: 600; color: var(--text); font-variant-numeric: tabular-nums; }}
  .rail-foot {{ font-size: 11px; color: #5e6b80; padding: 0 4px; }}

  /* Right Main Workspace */
  .main {{ flex: 1; display: flex; flex-direction: column; min-width: 0; }}
  .titlebar {{
    height: 42px; flex-shrink: 0;
    display: flex; align-items: center; justify-content: space-between;
    padding: 0 12px 0 22px;
    border-bottom: 1px solid rgba(255,255,255,0.06);
    cursor: grab;
  }}
  .titlebar:active {{ cursor: grabbing; }}
  .tb-title {{ font-size: 12.5px; font-weight: 600; color: var(--sub); }}
  .win-ctrls {{ display: flex; gap: 6px; }}
  .win-btn {{
    width: 26px; height: 26px; border-radius: 7px; border: none;
    background: rgba(255,255,255,0.04); color: var(--sub); font-size: 14px;
    cursor: pointer; display: flex; align-items: center; justify-content: center;
  }}
  .win-btn:hover {{ background: rgba(255,255,255,0.12); color: var(--text); }}
  .win-btn.close:hover {{ background: rgba(242,139,130,0.22); color: #f28b82; }}

  .content {{ flex: 1; overflow-y: auto; padding: 20px 22px 14px; display: flex; flex-direction: column; gap: 14px; }}
  .content::-webkit-scrollbar {{ width: 6px; }}
  .content::-webkit-scrollbar-thumb {{ background: rgba(255,255,255,0.12); border-radius: 3px; }}
  .tab-pane {{ display: none; flex-direction: column; gap: 14px; }}
  .tab-pane.active {{ display: flex; }}

  .card {{
    background: var(--card); border: 1px solid var(--card-border);
    border-radius: 13px; padding: 15px 17px;
    display: flex; flex-direction: column; gap: 9px;
  }}
  .card-hdr {{ display: flex; align-items: center; justify-content: space-between; gap: 10px; }}
  .card-title {{ font-size: 12px; text-transform: uppercase; letter-spacing: 0.6px; color: var(--sub); font-weight: 600; }}
  .status-line {{ display: flex; align-items: center; gap: 10px; font-size: 14.5px; font-weight: 600; }}
  .dot {{ width: 9px; height: 9px; border-radius: 50%; background: var(--blue); flex-shrink: 0; }}
  .dot.spin {{ animation: pulse 1.1s infinite; }}
  .dot.ok {{ background: var(--green); }}
  .dot.upd {{ background: var(--blue); box-shadow: 0 0 10px rgba(138,180,248,0.65); }}
  .dot.warn {{ background: var(--amber); }}
  @keyframes pulse {{ 0%, 100% {{ opacity: .35; }} 50% {{ opacity: 1; }} }}

  .muted {{ font-size: 12.5px; color: var(--sub); line-height: 1.5; }}
  .prog-wrap {{ display: none; flex-direction: column; gap: 6px; margin-top: 2px; }}
  .prog-wrap.show {{ display: flex; }}
  .bar-bg {{ width: 100%; height: 5px; border-radius: 999px; background: rgba(255,255,255,0.08); overflow: hidden; }}
  .bar-fill {{ height: 100%; width: 25%; background: var(--blue); border-radius: 999px; transition: width .2s; }}
  .bar-fill.indet {{ width: 35%; animation: slide 1s infinite ease-in-out; }}
  @keyframes slide {{ 0% {{ transform: translateX(-110%); }} 100% {{ transform: translateX(300%); }} }}

  .badge {{
    font-size: 11px; font-weight: 600; padding: 2px 8px; border-radius: 999px;
    background: rgba(255,255,255,0.07); color: var(--sub);
  }}
  .badge.ok {{ background: rgba(129,201,149,0.14); color: var(--green); }}
  .badge.upd {{ background: rgba(138,180,248,0.16); color: var(--blue); }}

  .cl-list {{ list-style: none; display: flex; flex-direction: column; gap: 6px; }}
  .cl-list li {{ font-size: 12.5px; color: #c9d2e0; padding-left: 14px; position: relative; line-height: 1.4; }}
  .cl-list li::before {{ content: "•"; position: absolute; left: 2px; color: var(--blue); }}

  .commit-row {{
    display: flex; align-items: baseline; gap: 9px; font-size: 12.3px;
    padding: 6px 0; border-bottom: 1px solid rgba(255,255,255,0.05);
  }}
  .commit-row:last-child {{ border-bottom: none; }}
  .sha {{ font-family: ui-monospace, monospace; font-size: 11px; color: var(--blue); flex-shrink: 0; }}
  .cmsg {{ flex: 1; color: #d5dce6; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
  .cdate {{ font-size: 11px; color: var(--sub); flex-shrink: 0; }}

  .field {{ display: flex; flex-direction: column; gap: 5px; }}
  .field label {{ font-size: 12px; color: var(--sub); }}
  .field input {{
    padding: 9px 11px; border-radius: 9px; font-size: 12.5px;
    background: rgba(0,0,0,0.28); border: 1px solid rgba(255,255,255,0.11);
    color: var(--text); outline: none; user-select: text; -webkit-user-select: text;
  }}
  .field input:focus {{ border-color: var(--blue); }}

  .code-box {{
    font-family: ui-monospace, monospace; font-size: 11.2px; color: #b7c6db;
    background: rgba(0,0,0,0.3); border: 1px solid rgba(255,255,255,0.07);
    border-radius: 9px; padding: 9px 11px; line-height: 1.5; user-select: text; -webkit-user-select: text;
  }}

  /* Footer Bar */
  .footer {{
    padding: 12px 22px 16px;
    border-top: 1px solid rgba(255,255,255,0.07);
    background: rgba(0,0,0,0.16);
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
  }}
  .auto-lbl {{ display: inline-flex; align-items: center; gap: 7px; font-size: 12px; color: var(--sub); cursor: pointer; }}
  .auto-lbl input {{ accent-color: var(--blue); width: 14px; height: 14px; }}
  .btn-group {{ display: flex; gap: 8px; align-items: center; }}
  .btn {{
    border: none; border-radius: 9px; padding: 9px 15px; font-size: 12.8px; font-weight: 600;
    cursor: pointer; transition: background .12s, transform .08s, opacity .15s;
  }}
  .btn:active {{ transform: scale(0.98); }}
  .btn:disabled {{ opacity: 0.45; cursor: not-allowed; }}
  .btn-ghost {{ background: rgba(255,255,255,0.06); color: #dce3ee; border: 1px solid rgba(255,255,255,0.09); }}
  .btn-ghost:hover:not(:disabled) {{ background: rgba(255,255,255,0.11); }}
  .btn-primary {{ background: #8ab4f8; color: #0a1424; }}
  .btn-primary:hover:not(:disabled) {{ background: #a3c5fa; }}
  .btn-green {{ background: #81c995; color: #081c10; }}
  .btn-link {{
    background: none; border: none; color: var(--blue); font-size: 12px; font-weight: 600;
    cursor: pointer; padding: 0;
  }}
  .btn-link:hover {{ text-decoration: underline; }}
</style>
</head>
<body>
<div class="shell">
  <aside class="rail">
    <div>
      <div class="brand">
        <img src="data:image/png;base64,{icon_b64}" alt="Logo">
        <div>
          <div class="brand-t">AdBlock Tarayıcı</div>
          <span class="brand-v">v{APP_VERSION}</span>
        </div>
      </div>
      <nav class="nav">
        <button class="nav-btn active" data-tab="overview" onclick="switchTab('overview')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>
          Genel Bakış
        </button>
        <button class="nav-btn" data-tab="changelog" onclick="switchTab('changelog')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 8v4l3 3"/><circle cx="12" cy="12" r="9"/></svg>
          Sürüm Geçmişi
        </button>
        <button class="nav-btn" data-tab="source" onclick="switchTab('source')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9c.26.6.77 1.02 1.41 1.11H21a2 2 0 0 1 0 4h-.09c-.64.09-1.15.51-1.51 1.09z"/></svg>
          Güncelleme Ayarları
        </button>
      </nav>
    </div>

    <div>
      <div class="stats-box">
        <div class="stat-row"><span class="stat-k">Engellenen Reklam</span><span class="stat-v">{blocked}</span></div>
        <div class="stat-row"><span class="stat-k">Kayıtlı Şifre</span><span class="stat-v">{pws}</span></div>
        <div class="stat-row"><span class="stat-k">Yer İmleri</span><span class="stat-v">{bms}</span></div>
      </div>
      <div class="rail-foot">Rust Native • DoH • AES-256</div>
    </div>
  </aside>

  <section class="main">
    <div class="titlebar" onmousedown="if(event.target.closest('button')==null) sendL('launcher_drag')">
      <span class="tb-title" id="tb-label">Başlatıcı &amp; Sürüm Kontrolü</span>
      <div class="win-ctrls">
        <button class="win-btn" title="Küçült" onclick="sendL('launcher_minimize')">−</button>
        <button class="win-btn close" title="Kapat" onclick="sendL('launcher_close')">×</button>
      </div>
    </div>

    <div class="content">
      <!-- TAB 1: OVERVIEW -->
      <div id="pane-overview" class="tab-pane active">
        <div class="card">
          <div class="card-hdr">
            <span class="card-title">Sürüm Durumu</span>
            <span id="st-badge" class="badge">Kontrol Ediliyor</span>
          </div>
          <div class="status-line">
            <div id="dot" class="dot spin"></div>
            <span id="st-title">Güncellemeler denetleniyor...</span>
          </div>
          <div id="st-sub" class="muted">GitHub Releases üzerinden yeni sürüm kontrol ediliyor.</div>
          <div id="prog-wrap" class="prog-wrap show">
            <div class="bar-bg"><div id="bar" class="bar-fill indet"></div></div>
          </div>
          <div id="st-helper" style="display:none; padding-top:2px;">
            <button class="btn-link" onclick="switchTab('source')">Özel repo için Token / URL ayarla →</button>
          </div>
        </div>

        <div class="card">
          <div class="card-hdr">
            <span class="card-title">Bu Sürümdeki Değişiklikler (v{APP_VERSION})</span>
            <button class="btn-link" onclick="switchTab('changelog')">Tüm Geçmiş →</button>
          </div>
          <ul id="latest-cl" class="cl-list"></ul>
        </div>
      </div>

      <!-- TAB 2: CHANGELOG & REMOTE RELEASES -->
      <div id="pane-changelog" class="tab-pane">
        <div id="remote-rel-wrap" style="display:none;" class="card">
          <div class="card-hdr">
            <span class="card-title">GitHub Yayınlanmış Sürümler (Releases)</span>
            <span class="badge ok">Canlı</span>
          </div>
          <div id="remote-releases" style="display:flex; flex-direction:column; gap:8px;"></div>
        </div>
        <div id="remote-cl-wrap" style="display:none;" class="card">
          <div class="card-hdr">
            <span class="card-title">GitHub Son Değişiklikler (Commit Geçmişi)</span>
            <span class="badge ok">Canlı Bağlantı</span>
          </div>
          <div id="remote-commits"></div>
        </div>
        <div id="builtin-cl-list" style="display:flex; flex-direction:column; gap:10px;"></div>
      </div>

      <!-- TAB 3: SOURCE & HOW VERSIONING WORKS -->
      <div id="pane-source" class="tab-pane">
        <div class="card">
          <div class="card-title">Güncelleme Kaynağı (GitHub Private / Custom Feed)</div>
          <div class="field">
            <label>GitHub Token (Özel depo Release ve Commit geçmişi için isteğe bağlı)</label>
            <input id="cfg-tok" type="password" placeholder="{token_ph}">
          </div>
          <div class="field">
            <label>Özel version.json URL (İsteğe bağlı — boşsa TALEH323/adblock-browser kullanılır)</label>
            <input id="cfg-url" type="text" value="{manifest_esc}" placeholder="https://.../version.json">
          </div>
          <div style="display:flex; justify-content:flex-end; gap:8px; margin-top:4px;">
            <button class="btn btn-primary" onclick="saveUpdateCfg()">Kaydet ve Bağlantıyı Test Et</button>
          </div>
        </div>

        <div class="card">
          <div class="card-title">Değişikliklerden Sürüm Yükseltme Nasıl Çalışır?</div>
          <p class="muted">
            1. Kod değişikliği yaptıktan sonra aşağıdaki tek komutu çalıştırdığınızda son commit'ler otomatik olarak <strong>CHANGELOG.json</strong>'a eklenir, <strong>Cargo.toml</strong> ve Windows Installer sürümü yükseltilir.<br>
            2. Etiket (<code>v0.2.0</code>) GitHub'a gönderildiğinde GitHub Actions yeni <strong>Windows Setup (.exe)</strong> ve <strong>macOS (.dmg)</strong> kurucularını Release olarak yayınlar ve Başlatıcı bunu algılayıp günceller.
          </p>
          <div class="code-box">bash rust-browser/scripts/bump_release.sh 0.2.0 "Yeni özellikler"<br>git tag v0.2.0 &amp;&amp; git push origin main v0.2.0</div>
        </div>
      </div>
    </div>

    <div class="footer">
      <label class="auto-lbl">
        <input id="chk-auto" type="checkbox" {checked_attr} onchange="sendL('launcher_set_auto', {{enabled:this.checked}})">
        Güncelse otomatik başlat
      </label>
      <div class="btn-group">
        <button id="btn-recheck" class="btn btn-ghost" onclick="sendL('launcher_recheck')">Kontrol Et</button>
        <button id="btn-update" class="btn btn-green" style="display:none;" onclick="startUpdate()">Şimdi Güncelle</button>
        <button id="btn-launch" class="btn btn-primary" onclick="sendL('launcher_launch')">Tarayıcıyı Başlat</button>
      </div>
    </div>
  </section>
</div>
<script>
  const BUILTIN_CL = {changelog_json};
  function sendL(cmd, extra) {{
    if (window.ipc) {{
      window.ipc.postMessage(JSON.stringify({{ type: 'internal_cmd', cmd, ...(extra || {{}}) }}));
    }}
  }}
  function switchTab(name) {{
    document.querySelectorAll('.nav-btn').forEach(b => b.classList.toggle('active', b.dataset.tab === name));
    document.querySelectorAll('.tab-pane').forEach(p => p.classList.toggle('active', p.id === 'pane-' + name));
    const labels = {{
      overview: 'Başlatıcı & Sürüm Kontrolü',
      changelog: 'Dahili Sürüm Kaydı ve Değişiklikler',
      source: 'Güncelleme Kaynağı & Sürüm Yükseltme'
    }};
    document.getElementById('tb-label').textContent = labels[name] || '';
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
    switchTab('overview');
  }}
  function renderBuiltInChangelog() {{
    const latestEl = document.getElementById('latest-cl');
    const fullEl = document.getElementById('builtin-cl-list');
    if (BUILTIN_CL.length > 0) {{
      latestEl.innerHTML = (BUILTIN_CL[0].changes || []).slice(0, 4).map(c => '<li>' + esc(c) + '</li>').join('');
    }}
    fullEl.innerHTML = BUILTIN_CL.map((e, i) => `
      <div class="card">
        <div class="card-hdr">
          <div style="font-size:13.5px; font-weight:600;">v${{esc(e.version)}} — ${{esc(e.title)}}</div>
          <span class="badge ${{i===0 ? 'ok' : ''}}">${{esc(e.date)}}</span>
        </div>
        <ul class="cl-list">${{(e.changes || []).map(c => '<li>' + esc(c) + '</li>').join('')}}</ul>
      </div>
    `).join('');
  }}
  function esc(s) {{
    return String(s || '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
  }}
  renderBuiltInChangelog();

  window.__setLauncherState = (st) => {{
    const dot = document.getElementById('dot');
    const title = document.getElementById('st-title');
    const sub = document.getElementById('st-sub');
    const badge = document.getElementById('st-badge');
    const progWrap = document.getElementById('prog-wrap');
    const bar = document.getElementById('bar');
    const helper = document.getElementById('st-helper');
    const btnUpd = document.getElementById('btn-update');
    const btnLaunch = document.getElementById('btn-launch');
    const btnRecheck = document.getElementById('btn-recheck');

    dot.className = 'dot ' + (st.dot || '');
    title.textContent = st.title || '';
    sub.textContent = st.sub || '';
    badge.textContent = st.badge || 'Hazır';
    badge.className = 'badge ' + (st.badge_cls || '');

    if (st.show_progress) {{
      progWrap.classList.add('show');
      if (st.indeterminate) {{
        bar.className = 'bar-fill indet';
      }} else {{
        bar.className = 'bar-fill';
        bar.style.width = (st.progress ?? 0) + '%';
      }}
    }} else {{
      progWrap.classList.remove('show');
    }}

    helper.style.display = st.show_cfg_hint ? 'block' : 'none';

    if (st.can_update) {{
      btnUpd.style.display = 'inline-block';
      btnUpd.disabled = !!st.updating;
      btnUpd.textContent = st.update_label || 'Şimdi Güncelle';
    }} else {{
      btnUpd.style.display = 'none';
    }}
    btnRecheck.disabled = !!st.checking || !!st.updating;
    btnLaunch.disabled = !!st.updating;

    if (Array.isArray(st.releases) && st.releases.length > 0) {{
      document.getElementById('remote-rel-wrap').style.display = 'flex';
      document.getElementById('remote-releases').innerHTML = st.releases.map(r =>
        `<div class="commit-row" style="flex-direction:column; gap:3px;">
          <div style="display:flex; justify-content:space-between; width:100%;">
            <span style="font-weight:600; color:#eef2f7;">v${{esc(r.version)}}</span>
            <span class="cdate">${{esc(r.date)}}</span>
          </div>
          <div class="muted" style="font-size:11.8px;">${{esc(r.notes || 'Sürüm notu yok.')}}</div>
        </div>`
      ).join('');
    }}

    if (Array.isArray(st.commits) && st.commits.length > 0) {{
      document.getElementById('remote-cl-wrap').style.display = 'flex';
      document.getElementById('remote-commits').innerHTML = st.commits.map(c =>
        `<div class="commit-row"><span class="sha">${{esc(c.sha)}}</span><span class="cmsg">${{esc(c.message)}}</span><span class="cdate">${{esc(c.date)}}</span></div>`
      ).join('');
    }}
  }};
  window.addEventListener('keydown', (e) => {{
    if (e.key === 'Enter' && e.target.tagName !== 'INPUT' && !document.getElementById('btn-launch').disabled) {{
      sendL('launcher_launch');
    }}
  }});
</script>
</body>
</html>"###,
        blocked = stats.blocked_total,
        pws = stats.password_count,
        bms = stats.bookmark_count,
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
    fn parses_builtin_changelog_and_renders_launcher() {
        let cl = built_in_changelog();
        assert!(!cl.is_empty());
        assert_eq!(cl[0].version, APP_VERSION);

        let stats = LauncherProfileStats {
            blocked_total: 42,
            password_count: 5,
            bookmark_count: 3,
        };
        let html = launcher_html(true, "", false, &stats);
        assert!(html.contains("AdBlock Tarayıcı"));
        assert!(html.contains("launcher_drag"));
        assert!(html.contains("42"));
    }
}
