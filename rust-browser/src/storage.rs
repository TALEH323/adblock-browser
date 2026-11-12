use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SitePermissionEntry {
    #[serde(default)]
    pub notifications: Option<bool>,
    #[serde(default)]
    pub microphone: Option<bool>,
    #[serde(default)]
    pub camera: Option<bool>,
    #[serde(default)]
    pub file_access: Option<bool>,
    #[serde(default)]
    pub geolocation: Option<bool>,
    #[serde(default)]
    pub clipboard: Option<bool>,
}

impl SitePermissionEntry {
    pub fn get(&self, kind: &str) -> Option<bool> {
        match kind {
            "notifications" => self.notifications,
            "microphone" => self.microphone,
            "camera" => self.camera,
            "file_access" => self.file_access,
            "geolocation" => self.geolocation,
            "clipboard" => self.clipboard,
            _ => None,
        }
    }

    pub fn set(&mut self, kind: &str, val: Option<bool>) {
        match kind {
            "notifications" => self.notifications = val,
            "microphone" => self.microphone = val,
            "camera" => self.camera = val,
            "file_access" => self.file_access = val,
            "geolocation" => self.geolocation = val,
            "clipboard" => self.clipboard = val,
            _ => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.notifications.is_none()
            && self.microphone.is_none()
            && self.camera.is_none()
            && self.file_access.is_none()
            && self.geolocation.is_none()
            && self.clipboard.is_none()
    }
}

pub fn normalize_perm_host(host: &str) -> String {
    let h = host.trim().to_lowercase();
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

/// Document-start permission interception script providing Chrome-like per-site prompts & state.
pub const PERMISSION_INJECT_JS: &str = r##"
(() => {
  if (window.__abPermInit) return;
  window.__abPermInit = true;
  if (!location.protocol.startsWith('http')) return;

  window.__abSitePerms = window.__abSitePerms || {};
  let nextReqId = 1;
  const pending = new Map();

  window.__abSetSitePerms = (map) => {
    if (map && typeof map === 'object') {
      window.__abSitePerms = map;
    }
  };

  window.__abResolvePerm = (reqId, allowed, permName) => {
    if (permName && typeof allowed === 'boolean') {
      window.__abSitePerms[permName] = allowed;
    }
    const entry = pending.get(reqId);
    if (entry) {
      pending.delete(reqId);
      entry.resolve(!!allowed);
    }
  };

  function requestPerm(permName) {
    const cached = window.__abSitePerms[permName];
    if (cached === true) return Promise.resolve(true);
    if (cached === false) return Promise.resolve(false);
    if (!window.ipc) return Promise.resolve(false);
    const id = nextReqId++;
    return new Promise((resolve) => {
      pending.set(id, { resolve });
      try {
        window.ipc.postMessage(JSON.stringify({
          type: 'ui_cmd',
          cmd: 'perm_request',
          perm: permName,
          id
        }));
      } catch (_) {
        pending.delete(id);
        resolve(false);
      }
      setTimeout(() => {
        if (pending.has(id)) {
          pending.delete(id);
          resolve(false);
        }
      }, 60000);
    });
  }

  // 1. Notifications
  if (window.Notification) {
    const OrigNotif = window.Notification;
    try {
      Object.defineProperty(OrigNotif, 'permission', {
        configurable: true,
        get() {
          const v = window.__abSitePerms.notifications;
          if (v === true) return 'granted';
          if (v === false) return 'denied';
          return 'default';
        }
      });
    } catch (_) {}
    const WrappedNotif = function(title, opts) {
      if (window.__abSitePerms.notifications !== true) {
        throw new TypeError('Notification permission not granted');
      }
      return new OrigNotif(title, opts);
    };
    WrappedNotif.prototype = OrigNotif.prototype;
    Object.defineProperty(WrappedNotif, 'permission', {
      configurable: true,
      get() {
        const v = window.__abSitePerms.notifications;
        return v === true ? 'granted' : (v === false ? 'denied' : 'default');
      }
    });
    WrappedNotif.requestPermission = (cb) => {
      return requestPerm('notifications').then((ok) => {
        const res = ok ? 'granted' : 'denied';
        if (typeof cb === 'function') {
          try { cb(res); } catch (_) {}
        }
        return res;
      });
    };
    if ('maxActions' in OrigNotif) WrappedNotif.maxActions = OrigNotif.maxActions;
    try { window.Notification = WrappedNotif; } catch (_) {}
  }

  // 2. Microphone & Camera (getUserMedia)
  if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) {
    const origGUM = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
    navigator.mediaDevices.getUserMedia = async function(constraints) {
      const c = constraints || {};
      if (c.audio) {
        const okMic = await requestPerm('microphone');
        if (!okMic) throw new DOMException('Microphone permission denied', 'NotAllowedError');
      }
      if (c.video) {
        const okCam = await requestPerm('camera');
        if (!okCam) throw new DOMException('Camera permission denied', 'NotAllowedError');
      }
      return origGUM(constraints);
    };
  }

  // 3. Local File System Access (File System Access API)
  for (const fnName of ['showOpenFilePicker', 'showSaveFilePicker', 'showDirectoryPicker']) {
    if (typeof window[fnName] === 'function') {
      const origPicker = window[fnName].bind(window);
      window[fnName] = async function(...args) {
        const ok = await requestPerm('file_access');
        if (!ok) throw new DOMException('File access permission denied', 'NotAllowedError');
        return origPicker(...args);
      };
    }
  }

  // 4. Geolocation
  if (navigator.geolocation) {
    const origGetPos = navigator.geolocation.getCurrentPosition.bind(navigator.geolocation);
    const origWatchPos = navigator.geolocation.watchPosition.bind(navigator.geolocation);
    navigator.geolocation.getCurrentPosition = (success, error, opts) => {
      requestPerm('geolocation').then((ok) => {
        if (ok) origGetPos(success, error, opts);
        else if (typeof error === 'function') error({ code: 1, message: 'User denied Geolocation', PERMISSION_DENIED: 1 });
      });
    };
    navigator.geolocation.watchPosition = (success, error, opts) => {
      let realId = 0;
      requestPerm('geolocation').then((ok) => {
        if (ok) realId = origWatchPos(success, error, opts);
        else if (typeof error === 'function') error({ code: 1, message: 'User denied Geolocation', PERMISSION_DENIED: 1 });
      });
      return realId;
    };
  }

  // 5. Clipboard Read
  if (navigator.clipboard) {
    if (typeof navigator.clipboard.readText === 'function') {
      const origReadText = navigator.clipboard.readText.bind(navigator.clipboard);
      navigator.clipboard.readText = async () => {
        const ok = await requestPerm('clipboard');
        if (!ok) throw new DOMException('Clipboard read denied', 'NotAllowedError');
        return origReadText();
      };
    }
    if (typeof navigator.clipboard.read === 'function') {
      const origRead = navigator.clipboard.read.bind(navigator.clipboard);
      navigator.clipboard.read = async () => {
        const ok = await requestPerm('clipboard');
        if (!ok) throw new DOMException('Clipboard read denied', 'NotAllowedError');
        return origRead();
      };
    }
  }

  // 6. navigator.permissions.query synchronization
  if (navigator.permissions && typeof navigator.permissions.query === 'function') {
    const origQuery = navigator.permissions.query.bind(navigator.permissions);
    const mapName = {
      'notifications': 'notifications',
      'microphone': 'microphone',
      'camera': 'camera',
      'geolocation': 'geolocation',
      'clipboard-read': 'clipboard'
    };
    navigator.permissions.query = async function(desc) {
      const key = desc && mapName[desc.name];
      if (key) {
        const val = window.__abSitePerms[key];
        const state = val === true ? 'granted' : (val === false ? 'denied' : 'prompt');
        return {
          name: desc.name,
          state,
          onchange: null,
          addEventListener() {},
          removeEventListener() {},
          dispatchEvent() { return true; }
        };
      }
      return origQuery(desc);
    };
  }
})();
"##;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSettings {
    #[serde(default = "default_true")]
    pub adblock_enabled: bool,
    #[serde(default = "default_true")]
    pub cosmetic_enabled: bool,
    #[serde(default = "default_true")]
    pub https_only: bool,
    #[serde(default = "default_true")]
    pub dns_protection: bool,
    #[serde(default = "default_true")]
    pub password_saving_enabled: bool,
    #[serde(default = "default_true")]
    pub show_bookmarks_bar: bool,
    #[serde(default = "default_home")]
    pub home_url: String,
    #[serde(default = "default_search")]
    pub search_engine: String,
    #[serde(default)]
    pub whitelisted_domains: Vec<String>,
    #[serde(default)]
    pub http_allowed_domains: Vec<String>,
    #[serde(default)]
    pub total_blocked: u64,
    #[serde(default)]
    pub auto_launch_if_current: bool,
    #[serde(default)]
    pub update_manifest_url: String,
    #[serde(default)]
    pub update_token: String,
    #[serde(default = "default_startup_mode")]
    pub startup_mode: String,
    #[serde(default)]
    pub last_session_tabs: Vec<String>,
    #[serde(default = "default_zoom_factor")]
    pub default_zoom: f64,
    #[serde(default = "default_true")]
    pub background_throttling: bool,
    #[serde(default)]
    pub download_dir: String,
    #[serde(default)]
    pub site_permissions: BTreeMap<String, SitePermissionEntry>,
}

fn default_true() -> bool {
    true
}
fn default_home() -> String {
    "adblock://home".to_string()
}
fn default_search() -> String {
    "https://www.google.com/search?q=".to_string()
}
fn default_startup_mode() -> String {
    "new_tab".to_string()
}
fn default_zoom_factor() -> f64 {
    1.0
}

impl BrowserSettings {
    pub fn effective_zoom(&self) -> f64 {
        if !self.default_zoom.is_finite() || self.default_zoom <= 0.0 {
            1.0
        } else {
            self.default_zoom.clamp(0.5, 2.0)
        }
    }
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            adblock_enabled: true,
            cosmetic_enabled: true,
            https_only: true,
            dns_protection: true,
            password_saving_enabled: true,
            show_bookmarks_bar: true,
            home_url: default_home(),
            search_engine: default_search(),
            whitelisted_domains: Vec::new(),
            http_allowed_domains: Vec::new(),
            total_blocked: 0,
            auto_launch_if_current: false,
            update_manifest_url: String::new(),
            update_token: String::new(),
            startup_mode: default_startup_mode(),
            last_session_tabs: Vec::new(),
            default_zoom: default_zoom_factor(),
            background_throttling: true,
            download_dir: String::new(),
            site_permissions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visited_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookmarkEntry {
    pub title: String,
    pub url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadEntry {
    pub id: String,
    pub filename: String,
    pub url: String,
    pub save_path: String,
    pub status: String, // "İndiriliyor", "Tamamlandı", "İptal/Hata"
    pub started_at: String,
}

pub struct BrowserStorage {
    pub data_dir: PathBuf,
    pub settings: BrowserSettings,
    pub history: Vec<HistoryEntry>,
    pub bookmarks: Vec<BookmarkEntry>,
    pub downloads: Vec<DownloadEntry>,
    pub settings_notice: Option<String>,
}

pub fn resolve_data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let local_data = parent.join("user_data");
            if local_data.exists() {
                return local_data;
            }
        }
    }
    let cwd_data = PathBuf::from("user_data");
    if cwd_data.exists() {
        return cwd_data;
    }
    if let Some(base) = dirs::data_local_dir() {
        return base.join("AdBlockBrowser");
    }
    PathBuf::from("user_data")
}

fn load_json_or_default<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<T>(&raw).ok())
        .unwrap_or_default()
}

fn save_json_atomic<T: Serialize>(path: &Path, val: &T) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(val) {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, &json).is_ok() {
            let _ = fs::rename(&tmp, path);
        } else {
            let _ = fs::write(path, &json);
        }
    }
}

impl BrowserStorage {
    pub fn load(data_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&data_dir);
        let settings: BrowserSettings = load_json_or_default(&data_dir.join("settings.json"));
        let history: Vec<HistoryEntry> = load_json_or_default(&data_dir.join("history.json"));
        let mut bookmarks: Vec<BookmarkEntry> =
            load_json_or_default(&data_dir.join("bookmarks.json"));
        if bookmarks.is_empty() && !data_dir.join("bookmarks.json").exists() {
            let now = Local::now().format("%Y-%m-%d %H:%M").to_string();
            bookmarks = vec![
                BookmarkEntry {
                    title: "Google".to_string(),
                    url: "https://www.google.com".to_string(),
                    created_at: now.clone(),
                },
                BookmarkEntry {
                    title: "YouTube (Reklamsız)".to_string(),
                    url: "https://www.youtube.com".to_string(),
                    created_at: now.clone(),
                },
                BookmarkEntry {
                    title: "Wikipedia".to_string(),
                    url: "https://www.wikipedia.org".to_string(),
                    created_at: now,
                },
            ];
            save_json_atomic(&data_dir.join("bookmarks.json"), &bookmarks);
        }
        let downloads: Vec<DownloadEntry> = load_json_or_default(&data_dir.join("downloads.json"));
        Self {
            data_dir,
            settings,
            history,
            bookmarks,
            downloads,
            settings_notice: None,
        }
    }

    pub fn effective_download_dir(&self) -> PathBuf {
        let custom = self.settings.download_dir.trim();
        let dir = if !custom.is_empty() {
            PathBuf::from(custom)
        } else {
            dirs::download_dir().unwrap_or_else(|| self.data_dir.join("downloads"))
        };
        let _ = fs::create_dir_all(&dir);
        dir
    }

    pub fn reset_settings_to_defaults(&mut self) {
        let total_blocked = self.settings.total_blocked;
        let auto_launch_if_current = self.settings.auto_launch_if_current;
        let update_manifest_url = self.settings.update_manifest_url.clone();
        let update_token = self.settings.update_token.clone();

        self.settings = BrowserSettings {
            total_blocked,
            auto_launch_if_current,
            update_manifest_url,
            update_token,
            ..BrowserSettings::default()
        };
        self.save_settings();
    }

    pub fn save_settings(&self) {
        save_json_atomic(&self.data_dir.join("settings.json"), &self.settings);
    }

    pub fn save_history(&self) {
        save_json_atomic(&self.data_dir.join("history.json"), &self.history);
    }

    pub fn save_bookmarks(&self) {
        save_json_atomic(&self.data_dir.join("bookmarks.json"), &self.bookmarks);
    }

    pub fn save_downloads(&self) {
        save_json_atomic(&self.data_dir.join("downloads.json"), &self.downloads);
    }

    pub fn record_history(&mut self, url: &str, title: &str) {
        if url.starts_with("adblock://") || url.starts_with("about:") || url.trim().is_empty() {
            return;
        }
        let now = Local::now().format("%Y-%m-%d %H:%M").to_string();
        if let Some(first) = self.history.first_mut() {
            if first.url == url {
                if !title.trim().is_empty() {
                    first.title = title.trim().to_string();
                }
                first.visited_at = now;
                self.save_history();
                return;
            }
        }
        let clean_title = if title.trim().is_empty() {
            url.to_string()
        } else {
            title.trim().to_string()
        };
        self.history.insert(
            0,
            HistoryEntry {
                url: url.to_string(),
                title: clean_title,
                visited_at: now,
            },
        );
        if self.history.len() > 1000 {
            self.history.truncate(1000);
        }
        self.save_history();
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.save_history();
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        let target = url.trim_end_matches('/');
        self.bookmarks
            .iter()
            .any(|b| b.url.trim_end_matches('/') == target)
    }

    pub fn add_bookmark(&mut self, title: &str, url: &str) {
        let u = url.trim();
        if u.is_empty() || self.is_bookmarked(u) {
            return;
        }
        let t = if title.trim().is_empty() {
            u.to_string()
        } else {
            title.trim().to_string()
        };
        self.bookmarks.push(BookmarkEntry {
            title: t,
            url: u.to_string(),
            created_at: Local::now().format("%Y-%m-%d %H:%M").to_string(),
        });
        self.save_bookmarks();
    }

    pub fn remove_bookmark(&mut self, url: &str) {
        let target = url.trim_end_matches('/');
        self.bookmarks
            .retain(|b| b.url.trim_end_matches('/') != target);
        self.save_bookmarks();
    }

    pub fn is_http_allowed(&self, host: &str) -> bool {
        let h = host.to_lowercase();
        let set: HashSet<&str> = self
            .settings
            .http_allowed_domains
            .iter()
            .map(|s| s.as_str())
            .collect();
        set.contains(h.as_str())
    }

    pub fn set_http_allowed(&mut self, host: &str, allowed: bool) {
        let h = host.trim().trim_matches('.').to_lowercase();
        if h.is_empty() {
            return;
        }
        self.settings.http_allowed_domains.retain(|x| x != &h);
        if allowed {
            self.settings.http_allowed_domains.push(h);
            self.settings.http_allowed_domains.sort();
        }
        self.save_settings();
    }

    pub fn get_site_permission(&self, host: &str, kind: &str) -> Option<bool> {
        let h = normalize_perm_host(host);
        if h.is_empty() {
            return None;
        }
        self.settings
            .site_permissions
            .get(&h)
            .and_then(|e| e.get(kind))
    }

    pub fn set_site_permission(&mut self, host: &str, kind: &str, val: Option<bool>) {
        let h = normalize_perm_host(host);
        if h.is_empty() {
            return;
        }
        let entry = self.settings.site_permissions.entry(h.clone()).or_default();
        entry.set(kind, val);
        if entry.is_empty() {
            self.settings.site_permissions.remove(&h);
        }
        self.save_settings();
    }

    pub fn clear_site_permissions(&mut self, host: Option<&str>) {
        match host {
            Some(raw) => {
                let h = normalize_perm_host(raw);
                self.settings.site_permissions.remove(&h);
            }
            None => self.settings.site_permissions.clear(),
        }
        self.save_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_history_and_bookmarks_persist() {
        let tmp = std::env::temp_dir().join(format!("ab_test_storage_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        {
            let mut st = BrowserStorage::load(tmp.clone());
            assert!(st.settings.https_only);
            assert!(st.settings.dns_protection);
            st.record_history("https://example.com", "Example");
            st.add_bookmark("Example", "https://example.com");
            st.set_http_allowed("legacy.local", true);
            assert!(st.is_bookmarked("https://example.com/"));
            assert!(st.is_http_allowed("legacy.local"));
        }
        let reloaded = BrowserStorage::load(tmp.clone());
        assert_eq!(reloaded.history.len(), 1);
        assert_eq!(reloaded.history[0].title, "Example");
        assert!(reloaded.is_bookmarked("https://example.com"));
        assert!(reloaded.is_http_allowed("legacy.local"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn settings_reset_zoom_and_download_dir_work() {
        let tmp = std::env::temp_dir().join(format!("ab_test_reset_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut st = BrowserStorage::load(tmp.clone());
        st.settings.search_engine = "https://duckduckgo.com/?q=".to_string();
        st.settings.startup_mode = "restore".to_string();
        st.settings.home_url = "https://example.org".to_string();
        st.settings.default_zoom = 5.0;
        assert_eq!(st.settings.effective_zoom(), 2.0);
        st.settings.default_zoom = f64::NAN;
        assert_eq!(st.settings.effective_zoom(), 1.0);

        let custom_dl = tmp.join("custom_dl");
        st.settings.download_dir = custom_dl.to_string_lossy().to_string();
        assert_eq!(st.effective_download_dir(), custom_dl);
        assert!(custom_dl.exists());

        st.settings.update_token = "ghp_secret".to_string();
        st.reset_settings_to_defaults();
        assert_eq!(st.settings.startup_mode, "new_tab");
        assert_eq!(st.settings.home_url, "adblock://home");
        assert_eq!(st.settings.default_zoom, 1.0);
        assert!(st.settings.download_dir.is_empty());
        assert_eq!(st.settings.update_token, "ghp_secret");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn site_permissions_persist_and_clear() {
        let tmp = std::env::temp_dir().join(format!("ab_test_perms_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        {
            let mut st = BrowserStorage::load(tmp.clone());
            assert_eq!(st.get_site_permission("www.meet.google.com", "microphone"), None);
            st.set_site_permission("www.meet.google.com", "microphone", Some(true));
            st.set_site_permission("meet.google.com", "camera", Some(false));
            st.set_site_permission("example.com", "notifications", Some(true));
            st.set_site_permission("example.com", "file_access", Some(false));
            assert_eq!(st.get_site_permission("meet.google.com", "microphone"), Some(true));
            assert_eq!(st.get_site_permission("www.meet.google.com", "camera"), Some(false));
        }
        let mut reloaded = BrowserStorage::load(tmp.clone());
        assert_eq!(reloaded.get_site_permission("meet.google.com", "microphone"), Some(true));
        assert_eq!(reloaded.get_site_permission("example.com", "file_access"), Some(false));
        reloaded.clear_site_permissions(Some("example.com"));
        assert_eq!(reloaded.get_site_permission("example.com", "notifications"), None);
        reloaded.clear_site_permissions(None);
        assert!(reloaded.settings.site_permissions.is_empty());
        assert!(PERMISSION_INJECT_JS.contains("showOpenFilePicker"));
        assert!(PERMISSION_INJECT_JS.contains("Notification"));
        let _ = fs::remove_dir_all(&tmp);
    }
}
