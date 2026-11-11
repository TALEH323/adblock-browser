use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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
        }
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
}
