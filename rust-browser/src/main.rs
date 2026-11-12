#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adblock_engine;
mod chrome_ui;
mod dns_guard;
mod internal_pages;
mod passwords;
mod proxy;
mod storage;
mod updater;

use crate::adblock_engine::{
    extract_host, AdBlockService, BlockedItem, SharedAdBlock, EARLY_ADBLOCK_INIT_JS,
};
use crate::chrome_ui::{
    chrome_bar_html, render_popover_html, render_side_panel_html, BarUiState, PopoverKind,
    TabUiItem,
};
use crate::dns_guard::DnsGuard;
use crate::passwords::{PasswordManager, PASSWORD_INJECT_JS};
use crate::proxy::{LocalSecurityProxy, ProxyEvent};
use crate::storage::{resolve_data_dir, BrowserStorage, DownloadEntry};
use crate::updater::{
    check_for_updates_bundle, download_and_install_update, launcher_html, LauncherProfileStats,
    ReleaseInfo, UpdateCheckBundle, UpdateCheckResult, APP_VERSION,
};
use chrono::Local;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
#[cfg(not(target_os = "macos"))]
use tao::window::Fullscreen;
use tao::window::WindowBuilder;
use wry::dpi::{LogicalPosition, LogicalSize as WrySize};
use wry::{
    BackgroundThrottlingPolicy, NewWindowResponse, PageLoadEvent, ProxyConfig, ProxyEndpoint, Rect,
    WebContext, WebView, WebViewBuilder,
};

const TAB_SHORTCUTS_JS: &str = r###"
(() => {
  if (window.__abShortcuts) return;
  window.__abShortcuts = true;
  if (window.top === window.self) {
    const syncFs = () => {
      if (!window.ipc) return;
      const active = !!(document.fullscreenElement || document.webkitFullscreenElement);
      window.ipc.postMessage(JSON.stringify({ type: 'ui_cmd', cmd: 'dom_fullscreen', enabled: active }));
    };
    document.addEventListener('fullscreenchange', syncFs, true);
    document.addEventListener('webkitfullscreenchange', syncFs, true);

    if (location.protocol.startsWith('http')) {
      let lastFav = '';
      const pickFavicon = () => {
        if (!window.ipc) return;
        const sels = [
          'link[rel="icon"]',
          'link[rel="shortcut icon"]',
          'link[rel~="icon"]',
          'link[rel="apple-touch-icon"]'
        ];
        let href = '';
        for (const s of sels) {
          const el = document.querySelector(s);
          if (el && el.getAttribute('href')) {
            href = el.getAttribute('href').trim();
            break;
          }
        }
        let resolved = '';
        if (href) {
          try { resolved = new URL(href, location.href).href; } catch (e) {}
        } else {
          resolved = location.origin + '/favicon.ico';
        }
        if (resolved && resolved !== lastFav) {
          lastFav = resolved;
          window.ipc.postMessage(JSON.stringify({ type: 'ui_cmd', cmd: 'tab_favicon', url: resolved }));
        }
      };
      if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', pickFavicon, { once: true });
      } else {
        pickFavicon();
      }
      window.addEventListener('load', pickFavicon, { once: true });
      const headTarget = document.head || document.documentElement;
      if (headTarget && window.MutationObserver) {
        let favTimer = null;
        const mo = new MutationObserver(() => {
          if (favTimer) return;
          favTimer = setTimeout(() => { favTimer = null; pickFavicon(); }, 250);
        });
        mo.observe(headTarget, { childList: true, subtree: true, attributes: true, attributeFilter: ['href', 'rel'] });
      }
    }
  }
  window.addEventListener('keydown', (e) => {
    const mod = e.metaKey || e.ctrlKey;
    if (e.key === 'Escape' && window.ipc) {
      window.ipc.postMessage(JSON.stringify({ type: 'ui_cmd', cmd: 'close_popover' }));
    }
    if (!mod || !window.ipc) return;
    const k = e.key.toLowerCase();
    if (k === 't') { e.preventDefault(); window.ipc.postMessage(JSON.stringify({ type: 'ui_cmd', cmd: 'new_tab' })); }
    else if (k === 'w') { e.preventDefault(); window.ipc.postMessage(JSON.stringify({ type: 'ui_cmd', cmd: 'close_current_tab' })); }
    else if (k === 'l') { e.preventDefault(); window.ipc.postMessage(JSON.stringify({ type: 'ui_cmd', cmd: 'focus_url' })); }
    else if (k === 'r') { e.preventDefault(); window.ipc.postMessage(JSON.stringify({ type: 'ui_cmd', cmd: 'reload' })); }
  }, true);
})();
"###;

#[derive(Debug)]
enum UserEvent {
    LauncherReady,
    UpdateChecked(UpdateCheckBundle),
    UpdateProgress { percent: u8, text: String },
    UpdateFinished(Result<bool, String>),
    AutoLaunchBrowser,
    CloseLauncher,
    ChromeReady,
    Ipc { tab_id: Option<usize>, raw: String },
    TabTitleChanged { tab_id: usize, title: String },
    TabPageLoad { tab_id: usize, finished: bool, url: String },
    OpenNewTab { url: String },
    Proxy(ProxyEvent),
    DownloadStarted { url: String, save_path: String },
    DownloadFinished { url: String, ok: bool },
    FilterListsUpdated,
}

#[derive(Deserialize)]
struct IpcEnvelope {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    cmd: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    route: String,
    #[serde(default)]
    id: Option<usize>,
    #[serde(default)]
    pop_kind: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    force_open: bool,
    #[serde(default)]
    anchor_right: Option<i32>,
    #[serde(default)]
    host: String,
    #[serde(default)]
    paused: bool,
    #[serde(default)]
    allowed: bool,
    #[serde(default)]
    key: String,
    #[serde(default)]
    val: Option<serde_json::Value>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    rules: String,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    csv_data: String,
    #[serde(default)]
    idx: Option<usize>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    rtype: String,
    #[serde(default)]
    rule: String,
    #[serde(default)]
    height: Option<f64>,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    clear_history: bool,
    #[serde(default)]
    clear_cookies: bool,
    #[serde(default)]
    clear_downloads: bool,
}

struct BrowserTab {
    id: usize,
    title: String,
    url: String,
    favicon_url: String,
    internal_route: Option<String>,
    blocked_items: Vec<BlockedItem>,
    webview: WebView,
}

struct BrowserApp {
    window: tao::window::Window,
    event_proxy: EventLoopProxy<UserEvent>,
    proxy_port: u16,
    storage: BrowserStorage,
    download_dir_shared: Arc<RwLock<PathBuf>>,
    passwords: PasswordManager,
    adblock: SharedAdBlock,
    dns_guard: Arc<DnsGuard>,
    dns_enabled: Arc<AtomicBool>,
    ui_web_context: WebContext,
    tab_web_context: WebContext,
    chrome_bar: WebView,
    chrome_ready: bool,
    side_panel: Option<WebView>,
    side_panel_mode: Option<String>,
    popover: Option<WebView>,
    popover_rect: Option<(f64, f64, f64)>,
    active_popover_kind: Option<String>,
    tabs: Vec<BrowserTab>,
    active_tab_idx: usize,
    next_tab_id: usize,
    pending_password: Option<(String, String, String)>,
    launcher_webview: Option<WebView>,
    launcher_ready: bool,
    in_launcher: bool,
    video_fullscreen: bool,
    pre_fullscreen_maximized: bool,
    updating_now: bool,
    pending_update: Option<ReleaseInfo>,
}

fn center_window_logical(window: &tao::window::Window, w: f64, h: f64) {
    if let Some(mon) = window.current_monitor().or_else(|| window.primary_monitor()) {
        let sf = window.scale_factor();
        let ms = mon.size();
        let mp = mon.position();
        let pw = (w * sf).round() as i32;
        let ph = (h * sf).round() as i32;
        let x = mp.x + (ms.width as i32 - pw) / 2;
        let y = mp.y + (ms.height as i32 - ph) / 2;
        window.set_outer_position(PhysicalPosition::new(x, y));
    }
}

impl BrowserApp {
    fn logical_size(&self) -> (f64, f64) {
        let s = self
            .window
            .inner_size()
            .to_logical::<f64>(self.window.scale_factor());
        (s.width.max(320.0), s.height.max(240.0))
    }

    fn bar_height(&self) -> f64 {
        if self.storage.settings.show_bookmarks_bar {
            110.0
        } else {
            80.0
        }
    }

    fn set_launcher_state(&self, state: serde_json::Value) {
        if !self.launcher_ready {
            return;
        }
        if let Some(lw) = &self.launcher_webview {
            if let Ok(js_arg) = serde_json::to_string(&state) {
                let _ = lw.evaluate_script(&format!(
                    "window.__setLauncherState && window.__setLauncherState({js_arg});"
                ));
            }
        }
    }

    fn start_update_check(&self) {
        self.set_launcher_state(serde_json::json!({
            "dot": "spin",
            "badge": "Kontrol Ediliyor",
            "badge_cls": "",
            "title": "Güncellemeler denetleniyor...",
            "sub": "GitHub Releases üzerinden en güncel sürüm ve commit geçmişi kontrol ediliyor.",
            "show_progress": true,
            "indeterminate": true,
            "show_cfg_hint": false,
            "checking": true,
            "can_update": false,
            "updating": false,
        }));
        let manifest = self.storage.settings.update_manifest_url.clone();
        let token = self.storage.settings.update_token.clone();
        let ep = self.event_proxy.clone();
        thread::spawn(move || {
            let bundle = check_for_updates_bundle(&manifest, &token);
            let _ = ep.send_event(UserEvent::UpdateChecked(bundle));
        });
    }

    fn start_update_download(&mut self) {
        let Some(info) = self.pending_update.clone() else {
            return;
        };
        self.updating_now = true;
        self.set_launcher_state(serde_json::json!({
            "dot": "upd",
            "badge": "İndiriliyor",
            "badge_cls": "upd",
            "title": format!("Sürüm v{} indiriliyor...", info.version),
            "sub": "Güncelleme paketi indiriliyor...",
            "show_progress": true,
            "indeterminate": false,
            "progress": 5,
            "show_cfg_hint": false,
            "can_update": true,
            "updating": true,
            "update_label": "Güncelleniyor..."
        }));
        let token = self.storage.settings.update_token.clone();
        let ep = self.event_proxy.clone();
        thread::spawn(move || {
            let ep_prog = ep.clone();
            let res = download_and_install_update(&info, &token, |pct, msg| {
                let _ = ep_prog.send_event(UserEvent::UpdateProgress {
                    percent: pct,
                    text: msg.to_string(),
                });
            });
            let _ = ep.send_event(UserEvent::UpdateFinished(res));
        });
    }

    fn save_session_tabs(&mut self) {
        if self.in_launcher || self.tabs.is_empty() {
            return;
        }
        let urls: Vec<String> = self
            .tabs
            .iter()
            .take(20)
            .map(|t| t.url.trim().to_string())
            .filter(|u| !u.is_empty())
            .collect();
        if !urls.is_empty() && urls != self.storage.settings.last_session_tabs {
            self.storage.settings.last_session_tabs = urls;
            self.storage.save_settings();
        }
    }

    fn launch_browser_from_launcher(&mut self) {
        if !self.in_launcher || self.updating_now {
            return;
        }
        self.in_launcher = false;
        if let Some(lw) = self.launcher_webview.take() {
            let _ = lw.set_visible(false);
            drop(lw);
        }
        // `set_resizable(true)` MUST precede `set_decorations(true)` on macOS because `tao::set_decorations`
        // snapshots `resizable` and applies `set_style_mask_async` later, which would otherwise strip `Resizable`.
        self.window.set_resizable(true);
        self.window.set_decorations(true);
        self.window.set_maximizable(true);
        self.window.set_minimizable(true);
        self.window.set_closable(true);
        self.window.set_title("AdBlock Tarayıcı");
        self.window.set_inner_size(LogicalSize::new(1280.0, 820.0));
        center_window_logical(&self.window, 1280.0, 820.0);
        let _ = self.chrome_bar.set_visible(true);
        if self.tabs.is_empty() {
            match self.storage.settings.startup_mode.as_str() {
                "restore" if !self.storage.settings.last_session_tabs.is_empty() => {
                    let urls = self.storage.settings.last_session_tabs.clone();
                    for (i, u) in urls.into_iter().enumerate() {
                        self.create_tab(&u, i == 0);
                    }
                }
                "custom" => {
                    let home = self.storage.settings.home_url.clone();
                    self.create_tab(&home, true);
                }
                _ => {
                    self.create_tab("adblock://home", true);
                }
            }
        }
        self.layout_views();
        self.sync_chrome_bar();
    }

    fn set_video_fullscreen(&mut self, enabled: bool) {
        if self.in_launcher || self.video_fullscreen == enabled {
            return;
        }
        self.video_fullscreen = enabled;
        if enabled {
            self.close_popover();
            self.pre_fullscreen_maximized = self.window.is_maximized();
            let _ = self.chrome_bar.set_visible(false);
            if let Some(sp) = &self.side_panel {
                let _ = sp.set_visible(false);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let mon = self.window.current_monitor();
                self.window.set_fullscreen(Some(Fullscreen::Borderless(mon)));
            }
        } else {
            #[cfg(not(target_os = "macos"))]
            {
                self.window.set_fullscreen(None);
                if self.pre_fullscreen_maximized {
                    self.window.set_maximized(true);
                }
            }
            let _ = self.chrome_bar.set_visible(true);
            if let Some(sp) = &self.side_panel {
                let _ = sp.set_visible(true);
            }
        }
        self.layout_views();
    }

    fn layout_views(&self) {
        let (w, h) = self.logical_size();
        if self.in_launcher {
            if let Some(lw) = &self.launcher_webview {
                let _ = lw.set_bounds(Rect {
                    position: LogicalPosition::new(0.0, 0.0).into(),
                    size: WrySize::new(w, h).into(),
                });
                let _ = lw.set_visible(true);
            }
            let _ = self.chrome_bar.set_visible(false);
            return;
        }

        if self.video_fullscreen {
            let _ = self.chrome_bar.set_visible(false);
            if let Some(sp) = &self.side_panel {
                let _ = sp.set_visible(false);
            }
            let full_rect = Rect {
                position: LogicalPosition::new(0.0, 0.0).into(),
                size: WrySize::new(w, h).into(),
            };
            for (i, tab) in self.tabs.iter().enumerate() {
                let active = i == self.active_tab_idx;
                let _ = tab.webview.set_bounds(full_rect);
                let _ = tab.webview.set_visible(active);
            }
            return;
        }

        let _ = self.chrome_bar.set_visible(true);
        let bh = self.bar_height();
        let _ = self.chrome_bar.set_bounds(Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: WrySize::new(w, bh).into(),
        });

        let sp_w = if self.side_panel.is_some() { 340.0 } else { 0.0 };
        let content_w = (w - sp_w).max(200.0);
        let content_h = (h - bh).max(120.0);

        if let Some(sp) = &self.side_panel {
            let _ = sp.set_visible(true);
            let _ = sp.set_bounds(Rect {
                position: LogicalPosition::new(content_w, bh).into(),
                size: WrySize::new(sp_w, content_h).into(),
            });
        }

        let tab_rect = Rect {
            position: LogicalPosition::new(0.0, bh).into(),
            size: WrySize::new(content_w, content_h).into(),
        };
        for (i, tab) in self.tabs.iter().enumerate() {
            let active = i == self.active_tab_idx;
            let _ = tab.webview.set_bounds(tab_rect);
            let _ = tab.webview.set_visible(active);
        }
    }

    fn create_tab(&mut self, initial_target: &str, activate: bool) {
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;

        let (w, h) = self.logical_size();
        let bh = self.bar_height();
        let sp_w = if self.side_panel.is_some() { 340.0 } else { 0.0 };
        let rect = Rect {
            position: LogicalPosition::new(0.0, bh).into(),
            size: WrySize::new((w - sp_w).max(200.0), (h - bh).max(120.0)).into(),
        };

        let dl_dir_shared = self.download_dir_shared.clone();
        let initial_dl = self.storage.effective_download_dir();
        if let Ok(mut guard) = dl_dir_shared.write() {
            *guard = initial_dl;
        }
        let bg_throttling = self.storage.settings.background_throttling;
        let proxy_port = self.proxy_port;
        let ep = self.event_proxy.clone();
        let window_ref = &self.window;

        let build_wv = |ctx: &mut WebContext| {
            let ep_ipc = ep.clone();
            let ep_title = ep.clone();
            let ep_load = ep.clone();
            let ep_new = ep.clone();
            let ep_dl_start = ep.clone();
            let ep_dl_done = ep.clone();
            let dl_dir = dl_dir_shared.clone();
            let throttle = if bg_throttling {
                BackgroundThrottlingPolicy::Throttle
            } else {
                BackgroundThrottlingPolicy::Disabled
            };

            WebViewBuilder::new_with_web_context(ctx)
                .with_bounds(rect)
                .with_visible(activate)
                .with_devtools(true)
                .with_clipboard(true)
                .with_background_throttling(throttle)
                .with_proxy_config(ProxyConfig::Http(ProxyEndpoint {
                    host: "127.0.0.1".to_string(),
                    port: proxy_port.to_string(),
                }))
                .with_initialization_script(TAB_SHORTCUTS_JS)
                .with_initialization_script(EARLY_ADBLOCK_INIT_JS)
                .with_initialization_script(PASSWORD_INJECT_JS)
                .with_ipc_handler(move |req| {
                    let _ = ep_ipc.send_event(UserEvent::Ipc {
                        tab_id: Some(tab_id),
                        raw: req.body().clone(),
                    });
                })
                .with_document_title_changed_handler(move |title| {
                    let _ = ep_title.send_event(UserEvent::TabTitleChanged { tab_id, title });
                })
                .with_on_page_load_handler(move |ev, url| {
                    let finished = matches!(ev, PageLoadEvent::Finished);
                    let _ = ep_load.send_event(UserEvent::TabPageLoad {
                        tab_id,
                        finished,
                        url,
                    });
                })
                .with_new_window_req_handler(move |url, _| {
                    if !url.is_empty() && url != "about:blank" {
                        let _ = ep_new.send_event(UserEvent::OpenNewTab { url });
                    }
                    NewWindowResponse::Deny
                })
                .with_download_started_handler(move |url, dest| {
                    let downloads_dir = dl_dir.read().unwrap().clone();
                    let _ = std::fs::create_dir_all(&downloads_dir);
                    let fname = url
                        .split('/')
                        .last()
                        .and_then(|s| s.split('?').next())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("dosya.bin");
                    let mut target: PathBuf = downloads_dir.join(fname);
                    if target.exists() {
                        let stem = target
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("dosya")
                            .to_string();
                        let ext = target
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|e| format!(".{e}"))
                            .unwrap_or_default();
                        let ts = Local::now().format("%H%M%S");
                        target = downloads_dir.join(format!("{stem}_{ts}{ext}"));
                    }
                    *dest = target.clone();
                    let _ = ep_dl_start.send_event(UserEvent::DownloadStarted {
                        url,
                        save_path: target.to_string_lossy().to_string(),
                    });
                    true
                })
                .with_download_completed_handler(move |url, _path, ok| {
                    let _ = ep_dl_done.send_event(UserEvent::DownloadFinished { url, ok });
                })
                .build_as_child(window_ref)
        };

        let webview = match build_wv(&mut self.tab_web_context) {
            Ok(wv) => wv,
            Err(_) => {
                // Fallback isolated context (e.g. if another instance owns `webview_tabs` with a different proxy port)
                let fallback_dir = self
                    .storage
                    .data_dir
                    .join(format!("webview_tabs_{}", std::process::id()));
                let _ = std::fs::create_dir_all(&fallback_dir);
                self.tab_web_context = WebContext::new(Some(fallback_dir));
                build_wv(&mut self.tab_web_context).expect("Tab WebView oluşturulamadı")
            }
        };
        let _ = webview.zoom(self.storage.settings.effective_zoom());

        let tab = BrowserTab {
            id: tab_id,
            title: "Yeni Sekme".to_string(),
            url: "adblock://home".to_string(),
            favicon_url: String::new(),
            internal_route: Some("home".to_string()),
            blocked_items: Vec::new(),
            webview,
        };
        self.tabs.push(tab);
        if activate {
            self.active_tab_idx = self.tabs.len() - 1;
        }
        let idx = self.tabs.len() - 1;
        self.navigate_tab_idx(idx, initial_target);
        self.save_session_tabs();
        self.layout_views();
        self.sync_chrome_bar();
    }

    fn close_tab_by_id(&mut self, id: usize) {
        if self.video_fullscreen {
            self.set_video_fullscreen(false);
        }
        if self.tabs.len() <= 1 {
            self.navigate_tab_idx(0, "adblock://home");
            self.save_session_tabs();
            return;
        }
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            self.tabs.remove(pos);
            if self.active_tab_idx >= self.tabs.len() {
                self.active_tab_idx = self.tabs.len() - 1;
            }
            self.save_session_tabs();
            self.close_popover();
            self.layout_views();
            self.sync_chrome_bar();
        }
    }

    fn switch_to_tab_id(&mut self, id: usize) {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            if self.video_fullscreen && pos != self.active_tab_idx {
                self.set_video_fullscreen(false);
            }
            self.active_tab_idx = pos;
            if let Ok(mut ab) = self.adblock.write() {
                ab.active_page_url = self.tabs[pos].url.clone();
            }
            self.close_popover();
            self.layout_views();
            self.sync_chrome_bar();
        }
    }

    fn navigate_tab_idx(&mut self, idx: usize, raw_input: &str) {
        if idx >= self.tabs.len() {
            return;
        }
        let input = raw_input.trim();
        if input.is_empty() {
            return;
        }
        if let Some(route) = input.strip_prefix("adblock://") {
            let clean = route.trim_matches('/');
            let route_name = if clean.is_empty() { "home" } else { clean };
            let html = {
                let ab = self.adblock.read().unwrap();
                internal_pages::render_route(route_name, &self.storage, &ab, &self.passwords)
            };
            let t = &mut self.tabs[idx];
            t.internal_route = Some(route_name.to_string());
            t.url = format!("adblock://{route_name}");
            t.favicon_url.clear();
            t.title = match route_name {
                "settings" => "Ayarlar",
                "downloads" => "İndirilenler",
                "passwords" => "Şifreler",
                "history" => "Geçmiş",
                "bookmarks" => "Yer İmleri",
                "adblock" => "AdBlock Yönetimi",
                "updates" => "Sürüm Geçmişi",
                _ => "Yeni Sekme",
            }
            .to_string();
            let _ = t.webview.load_html(&html);
            if idx == self.active_tab_idx {
                if let Ok(mut ab) = self.adblock.write() {
                    ab.active_page_url = t.url.clone();
                }
                self.sync_chrome_bar();
            }
            return;
        }

        let mut target_url = normalize_address(input, &self.storage.settings.search_engine);
        let host = extract_host(&target_url);

        if self.storage.settings.https_only
            && target_url.starts_with("http://")
            && !DnsGuard::is_local_or_ip(&host)
            && !self.storage.is_http_allowed(&host)
        {
            target_url = target_url.replacen("http://", "https://", 1);
        }

        if self.storage.settings.dns_protection && !DnsGuard::is_local_or_ip(&host) {
            let dns_res = self.dns_guard.check_host(&host, true);
            if !dns_res.safe {
                let html = internal_pages::render_security_interstitial(
                    &host,
                    &target_url,
                    &dns_res.reason,
                );
                let t = &mut self.tabs[idx];
                t.internal_route = None;
                t.url = target_url;
                t.favicon_url.clear();
                t.title = "Güvenlik Uyarısı".to_string();
                let _ = t.webview.load_html(&html);
                self.sync_chrome_bar();
                return;
            }
        }

        let t = &mut self.tabs[idx];
        t.internal_route = None;
        if extract_host(&t.url) != host {
            t.favicon_url.clear();
        }
        t.url = target_url.clone();
        t.blocked_items.clear();
        let _ = t.webview.load_url(&target_url);
        if idx == self.active_tab_idx {
            if let Ok(mut ab) = self.adblock.write() {
                ab.active_page_url = target_url;
            }
            self.sync_chrome_bar();
        }
    }

    fn reload_active_internal_if_matching(&mut self) {
        for tab in &mut self.tabs {
            if let Some(route) = &tab.internal_route {
                let html = {
                    let ab = self.adblock.read().unwrap();
                    internal_pages::render_route(route, &self.storage, &ab, &self.passwords)
                };
                let _ = tab.webview.load_html(&html);
            }
        }
    }

    fn live_tab_url(tab: &BrowserTab) -> String {
        tab.webview
            .url()
            .ok()
            .filter(|u| u.starts_with("http"))
            .unwrap_or_else(|| tab.url.clone())
    }

    fn push_pw_hints_to_tab(&self, tab_idx: usize) {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return;
        };
        if tab.internal_route.is_some() {
            return;
        }
        let live_url = Self::live_tab_url(tab);
        if !live_url.starts_with("http") {
            return;
        }
        let host = extract_host(&live_url);
        let hints: Vec<serde_json::Value> = if host.is_empty() {
            Vec::new()
        } else {
            self.passwords
                .for_origin(&host)
                .into_iter()
                .map(|c| {
                    serde_json::json!({
                        "origin": c.origin,
                        "username": c.username,
                    })
                })
                .collect()
        };
        if let Ok(json) = serde_json::to_string(&hints) {
            let _ = tab.webview.evaluate_script(&format!(
                "window.__adblockSetPwHints && window.__adblockSetPwHints({json});"
            ));
        }
    }

    fn sync_chrome_bar(&self) {
        if !self.chrome_ready {
            return;
        }
        let Some(active) = self.tabs.get(self.active_tab_idx) else {
            return;
        };
        let host = if active.internal_route.is_some() {
            String::new()
        } else {
            extract_host(&active.url)
        };
        let is_internal = active.internal_route.is_some();
        let is_secure = is_internal || active.url.starts_with("https://");
        let ab = self.adblock.read().unwrap();
        let state = BarUiState {
            tabs: self
                .tabs
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let tab_internal = t.internal_route.is_some();
                    let tab_host = if tab_internal {
                        String::new()
                    } else {
                        extract_host(&t.url)
                    };
                    let favicon = if tab_internal {
                        String::new()
                    } else if !t.favicon_url.is_empty() {
                        t.favicon_url.clone()
                    } else if !tab_host.is_empty() && t.url.starts_with("http") {
                        format!("https://www.google.com/s2/favicons?domain={tab_host}&sz=32")
                    } else {
                        String::new()
                    };
                    TabUiItem {
                        id: t.id,
                        title: t.title.clone(),
                        active: i == self.active_tab_idx,
                        blocked_count: t.blocked_items.len(),
                        favicon,
                        host: tab_host,
                        is_internal: tab_internal,
                    }
                })
                .collect(),
            url: active.url.clone(),
            host: host.clone(),
            is_secure,
            is_internal,
            is_bookmarked: self.storage.is_bookmarked(&active.url),
            can_go_back: active.webview.can_go_back().unwrap_or(false),
            can_go_forward: active.webview.can_go_forward().unwrap_or(false),
            adblock_enabled: ab.enabled,
            site_whitelisted: !host.is_empty() && ab.is_site_whitelisted(&host),
            tab_blocked: active.blocked_items.len(),
            show_bookmarks_bar: self.storage.settings.show_bookmarks_bar,
            bookmarks: self.storage.bookmarks.clone(),
            has_saved_pw: !host.is_empty() && !self.passwords.for_origin(&host).is_empty(),
            side_panel_open: self.side_panel.is_some(),
        };
        if let Ok(json) = serde_json::to_string(&state) {
            let _ = self
                .chrome_bar
                .evaluate_script(&format!("window.__updateBarState && window.__updateBarState({json});"));
        }
    }

    fn close_popover(&mut self) {
        self.popover = None;
        self.popover_rect = None;
        self.active_popover_kind = None;
    }

    fn open_popover(&mut self, kind_name: &str, anchor_right: Option<i32>) {
        if self.active_popover_kind.as_deref() == Some(kind_name) && self.popover.is_some() {
            self.close_popover();
            return;
        }
        self.close_popover();
        let Some(active) = self.tabs.get(self.active_tab_idx) else {
            return;
        };
        let host = if active.internal_route.is_some() {
            String::new()
        } else {
            extract_host(&active.url)
        };
        let ab = self.adblock.read().unwrap();
        let dns_cached = if host.is_empty() {
            None
        } else {
            self.dns_guard.get_cached(&host)
        };
        let is_secure = active.internal_route.is_some() || active.url.starts_with("https://");

        let (html, pop_w, pop_h) = match kind_name {
            "adblock" => render_popover_html(PopoverKind::AdBlock {
                host: &host,
                ab: &ab,
                blocked_items: &active.blocked_items,
            }),
            "security" => render_popover_html(PopoverKind::Security {
                host: &host,
                url: &active.url,
                is_secure,
                https_only: self.storage.settings.https_only,
                dns_on: self.storage.settings.dns_protection,
                http_allowed: !host.is_empty() && self.storage.is_http_allowed(&host),
                dns_res: dns_cached.as_ref(),
            }),
            "downloads" => render_popover_html(PopoverKind::Downloads {
                downloads: &self.storage.downloads,
            }),
            "passwords" => {
                let creds = if host.is_empty() {
                    Vec::new()
                } else {
                    self.passwords.for_origin(&host)
                };
                let pending = self
                    .pending_password
                    .as_ref()
                    .map(|(o, u, p)| (o.as_str(), u.as_str(), p.as_str()));
                render_popover_html(PopoverKind::Passwords {
                    host: &host,
                    creds,
                    pending,
                })
            }
            _ => render_popover_html(PopoverKind::Menu {
                show_bookmarks_bar: self.storage.settings.show_bookmarks_bar,
            }),
        };
        drop(ab);

        let (win_w, _) = self.logical_size();
        let right_edge = anchor_right.unwrap_or((win_w as i32) - 16) as f64;
        let x = if kind_name == "security" {
            (right_edge - 34.0).clamp(8.0, (win_w - pop_w as f64 - 8.0).max(8.0))
        } else {
            (right_edge - pop_w as f64).clamp(8.0, (win_w - pop_w as f64 - 8.0).max(8.0))
        };
        let y = self.bar_height() - 4.0;

        let ep = self.event_proxy.clone();
        if let Ok(wv) = WebViewBuilder::new_with_web_context(&mut self.ui_web_context)
            .with_transparent(true)
            .with_bounds(Rect {
                position: LogicalPosition::new(x, y).into(),
                size: WrySize::new(pop_w as f64, pop_h as f64).into(),
            })
            .with_html(&html)
            .with_ipc_handler(move |req| {
                let _ = ep.send_event(UserEvent::Ipc {
                    tab_id: None,
                    raw: req.body().clone(),
                });
            })
            .build_as_child(&self.window)
        {
            let _ = wv.focus();
            self.popover = Some(wv);
            self.popover_rect = Some((x, y, pop_w as f64));
            self.active_popover_kind = Some(kind_name.to_string());
        }
    }

    fn toggle_side_panel(&mut self, mode: &str, force_open: bool) {
        let same_mode = self.side_panel_mode.as_deref() == Some(mode);
        if self.side_panel.is_some() && same_mode && !force_open {
            self.side_panel = None;
            self.side_panel_mode = None;
            self.layout_views();
            self.sync_chrome_bar();
            return;
        }
        let html = render_side_panel_html(mode, &self.storage);
        if let Some(sp) = &self.side_panel {
            let _ = sp.load_html(&html);
        } else {
            let (w, h) = self.logical_size();
            let bh = self.bar_height();
            let ep = self.event_proxy.clone();
            if let Ok(wv) = WebViewBuilder::new_with_web_context(&mut self.ui_web_context)
                .with_bounds(Rect {
                    position: LogicalPosition::new((w - 340.0).max(200.0), bh).into(),
                    size: WrySize::new(340.0, (h - bh).max(120.0)).into(),
                })
                .with_html(&html)
                .with_ipc_handler(move |req| {
                    let _ = ep.send_event(UserEvent::Ipc {
                        tab_id: None,
                        raw: req.body().clone(),
                    });
                })
                .build_as_child(&self.window)
            {
                self.side_panel = Some(wv);
            }
        }
        self.side_panel_mode = Some(mode.to_string());
        self.layout_views();
        self.sync_chrome_bar();
    }

    fn handle_ipc(&mut self, source_tab_id: Option<usize>, raw: &str) {
        let Ok(env) = serde_json::from_str::<IpcEnvelope>(raw) else {
            return;
        };

        if env.kind == "blocked_req" {
            if let Some(tab) = self.tabs.get_mut(self.active_tab_idx) {
                if tab.blocked_items.len() < 250 {
                    tab.blocked_items.push(BlockedItem {
                        resource_type: env.rtype,
                        request_url: env.url,
                        rule: env.rule,
                    });
                }
            }
            if let Ok(mut ab) = self.adblock.write() {
                ab.total_blocked += 1;
                self.storage.settings.total_blocked = ab.total_blocked;
            }
            self.sync_chrome_bar();
            return;
        }

        if env.kind == "pw_capture" {
            if self.storage.settings.password_saving_enabled
                && !self.passwords.is_never_save(&env.origin)
                && !self
                    .passwords
                    .is_already_saved(&env.origin, &env.username, &env.password)
            {
                self.pending_password = Some((env.origin, env.username, env.password));
                self.open_popover("passwords", None);
            }
            return;
        }

        // Handle UI & internal page commands
        let active_idx = self.active_tab_idx;
        match env.cmd.as_str() {
            "close_popover" => self.close_popover(),
            "dom_fullscreen" => {
                let from_active = source_tab_id
                    .and_then(|id| self.tabs.get(active_idx).map(|t| t.id == id))
                    .unwrap_or(true);
                if from_active {
                    self.set_video_fullscreen(env.enabled);
                }
            }
            "tab_favicon" => {
                let raw = env.url.trim();
                let valid = (raw.starts_with("https://")
                    || raw.starts_with("http://")
                    || raw.starts_with("data:image/"))
                    && raw.len() <= 65_536;
                if valid {
                    if let Some(id) = source_tab_id {
                        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                            if t.internal_route.is_none() && t.favicon_url != raw {
                                t.favicon_url = raw.to_string();
                                self.sync_chrome_bar();
                            }
                        }
                    }
                }
            }
            "resize_popover" => {
                if let (Some(pop), Some((px, py, pw)), Some(req_h)) =
                    (&self.popover, self.popover_rect, env.height)
                {
                    let (_, win_h) = self.logical_size();
                    let max_h = (win_h - py - 8.0).max(140.0);
                    let final_h = req_h.clamp(100.0, max_h);
                    let _ = pop.set_bounds(Rect {
                        position: LogicalPosition::new(px, py).into(),
                        size: WrySize::new(pw, final_h).into(),
                    });
                }
            }
            "open_popover" => {
                let k = if env.pop_kind.is_empty() {
                    "menu".to_string()
                } else {
                    env.pop_kind.clone()
                };
                self.open_popover(&k, env.anchor_right);
            }
            "new_tab" => {
                self.close_popover();
                self.create_tab("adblock://home", true);
            }
            "switch_tab" => {
                if let Some(id) = env.id {
                    self.switch_to_tab_id(id);
                }
            }
            "close_tab" => {
                if let Some(id) = env.id {
                    self.close_tab_by_id(id);
                }
            }
            "close_current_tab" => {
                if let Some(t) = self.tabs.get(active_idx) {
                    let id = t.id;
                    self.close_tab_by_id(id);
                }
            }
            "focus_url" => {
                let _ = self.chrome_bar.evaluate_script("window.__focusUrl && window.__focusUrl();");
            }
            "navigate" => {
                self.close_popover();
                self.navigate_tab_idx(active_idx, &env.url);
            }
            "open_internal" => {
                self.close_popover();
                let target = format!("adblock://{}", env.route);
                self.navigate_tab_idx(active_idx, &target);
            }
            "back" => {
                if let Some(t) = self.tabs.get(active_idx) {
                    let _ = t.webview.go_back();
                }
            }
            "forward" => {
                if let Some(t) = self.tabs.get(active_idx) {
                    let _ = t.webview.go_forward();
                }
            }
            "reload" => {
                if let Some(t) = self.tabs.get(active_idx) {
                    if let Some(route) = t.internal_route.clone() {
                        self.navigate_tab_idx(active_idx, &format!("adblock://{route}"));
                    } else {
                        let _ = t.webview.reload();
                    }
                }
            }
            "home" => {
                let home = self.storage.settings.home_url.clone();
                self.navigate_tab_idx(active_idx, &home);
            }
            "toggle_bookmark" => {
                if let Some(t) = self.tabs.get(active_idx) {
                    let u = t.url.clone();
                    let title = t.title.clone();
                    if self.storage.is_bookmarked(&u) {
                        self.storage.remove_bookmark(&u);
                    } else {
                        self.storage.add_bookmark(&title, &u);
                    }
                    self.sync_chrome_bar();
                }
            }
            "toggle_side_panel" => {
                let m = if env.mode.is_empty() { "history" } else { &env.mode };
                self.toggle_side_panel(m, env.force_open);
            }
            "close_side_panel" => {
                self.side_panel = None;
                self.side_panel_mode = None;
                self.layout_views();
                self.sync_chrome_bar();
            }
            "open_devtools" => {
                self.close_popover();
                if let Some(t) = self.tabs.get(active_idx) {
                    t.webview.open_devtools();
                }
            }
            "ab_toggle_site" => {
                if let Ok(mut ab) = self.adblock.write() {
                    ab.set_site_whitelisted(&env.host, env.paused);
                    self.storage.settings.whitelisted_domains =
                        ab.whitelisted_domains.iter().cloned().collect();
                    self.storage.settings.whitelisted_domains.sort();
                    self.storage.save_settings();
                    if let Some(t) = self.tabs.get(active_idx) {
                        if t.internal_route.is_none() {
                            let js = ab.build_page_script(&t.url);
                            if !js.is_empty() {
                                let _ = t.webview.evaluate_script(&js);
                            }
                        }
                    }
                }
                self.sync_chrome_bar();
                self.open_popover("adblock", None);
            }
            "ab_wl_add" => {
                if let Ok(mut ab) = self.adblock.write() {
                    ab.set_site_whitelisted(&env.host, true);
                    self.storage.settings.whitelisted_domains =
                        ab.whitelisted_domains.iter().cloned().collect();
                    self.storage.settings.whitelisted_domains.sort();
                    self.storage.save_settings();
                }
                self.reload_active_internal_if_matching();
                self.sync_chrome_bar();
            }
            "ab_wl_del" => {
                if let Ok(mut ab) = self.adblock.write() {
                    ab.set_site_whitelisted(&env.host, false);
                    self.storage.settings.whitelisted_domains =
                        ab.whitelisted_domains.iter().cloned().collect();
                    self.storage.settings.whitelisted_domains.sort();
                    self.storage.save_settings();
                }
                self.reload_active_internal_if_matching();
                self.sync_chrome_bar();
            }
            "ab_save_rules" => {
                if let Ok(mut ab) = self.adblock.write() {
                    ab.set_custom_rules(env.rules);
                }
                self.reload_active_internal_if_matching();
                self.sync_chrome_bar();
            }
            "ab_update_lists" => {
                let ab = self.adblock.clone();
                let ep = self.event_proxy.clone();
                thread::spawn(move || {
                    if let Ok(mut guard) = ab.write() {
                        guard.update_filter_lists();
                    }
                    let _ = ep.send_event(UserEvent::FilterListsUpdated);
                });
            }
            "set_setting" => {
                self.apply_setting(&env.key, env.val);
            }
            "toggle_http_allowed" => {
                self.storage.set_http_allowed(&env.host, env.allowed);
                self.sync_chrome_bar();
            }
            "disallow_http" => {
                self.storage.set_http_allowed(&env.host, false);
                self.reload_active_internal_if_matching();
            }
            "allow_http_once" => {
                self.storage.set_http_allowed(&env.host, true);
                self.navigate_tab_idx(active_idx, &env.url);
            }
            "clear_history" => {
                self.storage.clear_history();
                self.reload_active_internal_if_matching();
            }
            "clear_downloads" => {
                self.storage.downloads.clear();
                self.storage.save_downloads();
                self.reload_active_internal_if_matching();
            }
            "bm_add" => {
                self.storage.add_bookmark(&env.title, &env.url);
                self.reload_active_internal_if_matching();
                self.sync_chrome_bar();
            }
            "bm_del" => {
                self.storage.remove_bookmark(&env.url);
                self.reload_active_internal_if_matching();
                self.sync_chrome_bar();
            }
            "pw_add" | "pw_save_pending" => {
                self.passwords
                    .add_or_update(&env.origin, &env.username, &env.password);
                self.pending_password = None;
                self.close_popover();
                self.reload_active_internal_if_matching();
                self.push_pw_hints_to_tab(active_idx);
                self.sync_chrome_bar();
            }
            "pw_import_csv" => {
                self.passwords.import_chrome_csv(&env.csv_data);
                self.reload_active_internal_if_matching();
                self.push_pw_hints_to_tab(active_idx);
                self.sync_chrome_bar();
            }
            "pw_clear_notice" => {
                self.passwords.last_import_notice = None;
                self.reload_active_internal_if_matching();
            }
            "launcher_launch" => {
                self.launch_browser_from_launcher();
            }
            "launcher_drag" => {
                let _ = self.window.drag_window();
            }
            "launcher_minimize" => {
                self.window.set_minimized(true);
            }
            "launcher_close" => {
                let _ = self.event_proxy.send_event(UserEvent::CloseLauncher);
            }
            "launcher_recheck" => {
                self.start_update_check();
            }
            "launcher_start_update" => {
                self.start_update_download();
            }
            "launcher_set_auto" => {
                self.storage.settings.auto_launch_if_current = env.enabled;
                self.storage.save_settings();
            }
            "launcher_save_config" => {
                self.storage.settings.update_manifest_url = env.url.trim().to_string();
                if !env.rules.trim().is_empty() {
                    self.storage.settings.update_token = env.rules.trim().to_string();
                }
                self.storage.save_settings();
                self.start_update_check();
            }
            "pw_never_save" => {
                self.passwords.mark_never_save(&env.origin);
                self.pending_password = None;
                self.close_popover();
            }
            "pw_delete" => {
                if let Some(i) = env.idx {
                    self.passwords.delete_at(i);
                    self.reload_active_internal_if_matching();
                    self.push_pw_hints_to_tab(active_idx);
                    self.sync_chrome_bar();
                }
            }
            "pw_query_hints" => {
                let target_idx = source_tab_id
                    .and_then(|id| self.tabs.iter().position(|t| t.id == id))
                    .unwrap_or(active_idx);
                if target_idx == active_idx {
                    self.push_pw_hints_to_tab(target_idx);
                }
            }
            "pw_fill_inline" => {
                let target_idx = source_tab_id
                    .and_then(|id| self.tabs.iter().position(|t| t.id == id))
                    .unwrap_or(active_idx);
                if target_idx != active_idx {
                    return;
                }
                if let Some(t) = self.tabs.get(target_idx) {
                    if t.internal_route.is_some() {
                        return;
                    }
                    let live_url = Self::live_tab_url(t);
                    let host = extract_host(&live_url);
                    if host.is_empty() {
                        return;
                    }
                    let matches = self.passwords.for_origin(&host);
                    let picked = matches
                        .iter()
                        .find(|c| {
                            (env.origin.is_empty() || c.origin == env.origin)
                                && c.username == env.username
                        })
                        .or_else(|| matches.iter().find(|c| c.username == env.username));
                    if let Some(cred) = picked {
                        let u_json = serde_json::to_string(&cred.username).unwrap_or_default();
                        let p_json = serde_json::to_string(&cred.password).unwrap_or_default();
                        let _ = t.webview.evaluate_script(&format!(
                            "window.__adblockFillPassword && window.__adblockFillPassword({u_json}, {p_json});"
                        ));
                    }
                }
            }
            "pw_fill" => {
                self.close_popover();
                if let Some(t) = self.tabs.get(active_idx) {
                    let u_json = serde_json::to_string(&env.username).unwrap_or_default();
                    let p_json = serde_json::to_string(&env.password).unwrap_or_default();
                    let _ = t.webview.evaluate_script(&format!(
                        "window.__adblockFillPassword && window.__adblockFillPassword({u_json}, {p_json});"
                    ));
                }
            }
            "reveal_file" => {
                reveal_in_file_manager(&env.path);
            }
            "clear_settings_notice" => {
                self.storage.settings_notice = None;
                self.reload_active_internal_if_matching();
            }
            "clear_browsing_data" => {
                let mut cleared = Vec::new();
                if env.clear_history {
                    self.storage.clear_history();
                    cleared.push("tarama geçmişi");
                }
                if env.clear_cookies {
                    let mut cookie_count = 0usize;
                    for t in &self.tabs {
                        if let Ok(cookies) = t.webview.cookies() {
                            for c in cookies {
                                if t.webview.delete_cookie(&c).is_ok() {
                                    cookie_count += 1;
                                }
                            }
                        }
                        if t.internal_route.is_none() {
                            let _ = t.webview.evaluate_script(
                                "try{localStorage.clear();sessionStorage.clear();}catch(e){}",
                            );
                        }
                    }
                    cleared.push(if cookie_count > 0 {
                        "çerezler ve oturum verileri"
                    } else {
                        "site çerezleri"
                    });
                }
                if env.clear_downloads {
                    self.storage.downloads.clear();
                    self.storage.save_downloads();
                    cleared.push("indirme kayıtları");
                }
                self.storage.settings_notice = Some(if cleared.is_empty() {
                    "Temizlenecek veri türü seçilmedi.".to_string()
                } else {
                    format!("Temizlendi: {}.", cleared.join(", "))
                });
                self.reload_active_internal_if_matching();
                self.sync_chrome_bar();
            }
            "open_download_dir" => {
                let dir = self.storage.effective_download_dir();
                open_directory_in_file_manager(&dir);
            }
            "open_default_browser_settings" => {
                open_default_browser_system_settings();
                self.storage.settings_notice = Some(
                    "Sistem varsayılan uygulama ayarları açıldı. AdBlock Tarayıcı'yı varsayılan web tarayıcısı olarak seçebilirsiniz."
                        .to_string(),
                );
                self.reload_active_internal_if_matching();
            }
            "reset_settings" => {
                self.storage.reset_settings_to_defaults();
                if let Ok(mut ab) = self.adblock.write() {
                    ab.enabled = self.storage.settings.adblock_enabled;
                    ab.cosmetic_enabled = self.storage.settings.cosmetic_enabled;
                    ab.whitelisted_domains.clear();
                }
                self.dns_enabled
                    .store(self.storage.settings.dns_protection, Ordering::Relaxed);
                let dl = self.storage.effective_download_dir();
                if let Ok(mut guard) = self.download_dir_shared.write() {
                    *guard = dl;
                }
                let zoom = self.storage.settings.effective_zoom();
                for t in &self.tabs {
                    let _ = t.webview.zoom(zoom);
                }
                self.storage.settings_notice = Some(
                    "Tüm tarayıcı ayarları varsayılan değerlere sıfırlandı."
                        .to_string(),
                );
                self.layout_views();
                self.reload_active_internal_if_matching();
                self.sync_chrome_bar();
            }
            _ => {}
        }
    }

    fn apply_setting(&mut self, key: &str, val: Option<serde_json::Value>) {
        let Some(v) = val else { return };
        let mut reload_internal = false;
        match key {
            "adblock_enabled" => {
                if let Some(b) = v.as_bool() {
                    self.storage.settings.adblock_enabled = b;
                    if let Ok(mut ab) = self.adblock.write() {
                        ab.enabled = b;
                    }
                }
            }
            "cosmetic_enabled" => {
                if let Some(b) = v.as_bool() {
                    self.storage.settings.cosmetic_enabled = b;
                    if let Ok(mut ab) = self.adblock.write() {
                        ab.cosmetic_enabled = b;
                    }
                }
            }
            "https_only" => {
                if let Some(b) = v.as_bool() {
                    self.storage.settings.https_only = b;
                }
            }
            "dns_protection" => {
                if let Some(b) = v.as_bool() {
                    self.storage.settings.dns_protection = b;
                    self.dns_enabled.store(b, Ordering::Relaxed);
                }
            }
            "password_saving_enabled" => {
                if let Some(b) = v.as_bool() {
                    self.storage.settings.password_saving_enabled = b;
                }
            }
            "show_bookmarks_bar" => {
                if let Some(b) = v.as_bool() {
                    self.storage.settings.show_bookmarks_bar = b;
                    self.close_popover();
                    self.layout_views();
                }
            }
            "search_engine" => {
                if let Some(s) = v.as_str() {
                    self.storage.settings.search_engine = s.to_string();
                }
            }
            "default_zoom" => {
                if let Some(z) = v.as_f64() {
                    let clamped = z.clamp(0.5, 2.0);
                    self.storage.settings.default_zoom = clamped;
                    for t in &self.tabs {
                        let _ = t.webview.zoom(clamped);
                    }
                    self.storage.settings_notice = Some(format!(
                        "Varsayılan yakınlaştırma %{} olarak güncellendi.",
                        (clamped * 100.0).round() as i32
                    ));
                    reload_internal = true;
                }
            }
            "background_throttling" => {
                if let Some(b) = v.as_bool() {
                    self.storage.settings.background_throttling = b;
                    self.storage.settings_notice = Some(if b {
                        "Arka plan sekme enerji tasarrufu etkinleştirildi.".to_string()
                    } else {
                        "Arka plan sekme kısıtlaması kapatıldı.".to_string()
                    });
                    reload_internal = true;
                }
            }
            "startup_mode" => {
                if let Some(s) = v.as_str() {
                    let mode = match s {
                        "restore" | "custom" | "new_tab" => s,
                        _ => "new_tab",
                    };
                    self.storage.settings.startup_mode = mode.to_string();
                    self.storage.settings_notice =
                        Some("Başlangıç davranışı kaydedildi.".to_string());
                    reload_internal = true;
                }
            }
            "home_url" => {
                if let Some(raw) = v.as_str() {
                    let trimmed = raw.trim();
                    let next = if trimmed.is_empty() {
                        "adblock://home".to_string()
                    } else if trimmed.starts_with("adblock://")
                        || trimmed.starts_with("http://")
                        || trimmed.starts_with("https://")
                    {
                        trimmed.to_string()
                    } else {
                        normalize_address(trimmed, &self.storage.settings.search_engine)
                    };
                    self.storage.settings.home_url = next.clone();
                    self.storage.settings_notice =
                        Some(format!("Ana sayfa adresi güncellendi: {next}"));
                    reload_internal = true;
                }
            }
            "download_dir" => {
                if let Some(raw) = v.as_str() {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() {
                        self.storage.settings.download_dir.clear();
                        let eff = self.storage.effective_download_dir();
                        if let Ok(mut guard) = self.download_dir_shared.write() {
                            *guard = eff.clone();
                        }
                        self.storage.settings_notice = Some(format!(
                            "İndirme klasörü sistem varsayılanına döndürüldü: {}",
                            eff.display()
                        ));
                    } else if std::fs::create_dir_all(trimmed).is_ok() {
                        self.storage.settings.download_dir = trimmed.to_string();
                        if let Ok(mut guard) = self.download_dir_shared.write() {
                            *guard = PathBuf::from(trimmed);
                        }
                        self.storage.settings_notice =
                            Some(format!("İndirme klasörü güncellendi: {trimmed}"));
                    } else {
                        self.storage.settings_notice =
                            Some("Belirtilen indirme klasörü oluşturulamadı/erişilemedi.".to_string());
                    }
                    reload_internal = true;
                }
            }
            _ => {}
        }
        self.storage.save_settings();
        if reload_internal {
            self.reload_active_internal_if_matching();
        }
        self.sync_chrome_bar();
    }
}

fn normalize_address(raw: &str, search_engine: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if !trimmed.contains(' ') && (trimmed.contains('.') || trimmed.starts_with("localhost")) {
        return format!("https://{trimmed}");
    }
    let encoded: String = url::form_urlencoded::byte_serialize(trimmed.as_bytes()).collect();
    format!("{search_engine}{encoded}")
}

fn reveal_in_file_manager(path: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path))
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
        }
    }
}

fn open_directory_in_file_manager(dir: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(dir).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

fn open_default_browser_system_settings() {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.Desktop-Settings.extension")
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:defaultapps"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

fn main() {
    let data_dir = resolve_data_dir();
    let crash_log = data_dir.join("last_crash.log");
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::write(&crash_log, format!("[{}] PANIC: {info}\n", Local::now()));
    }));

    let storage = BrowserStorage::load(data_dir.clone());
    let ui_ctx_dir = data_dir.join("webview_ui");
    let tab_ctx_dir = data_dir.join("webview_tabs");
    let _ = std::fs::create_dir_all(&ui_ctx_dir);
    let _ = std::fs::create_dir_all(&tab_ctx_dir);
    let mut ui_web_context = WebContext::new(Some(ui_ctx_dir));
    let tab_web_context = WebContext::new(Some(tab_ctx_dir));
    let passwords = PasswordManager::load(&data_dir);
    let adblock: SharedAdBlock = Arc::new(RwLock::new(AdBlockService::load(
        &data_dir,
        storage.settings.adblock_enabled,
        storage.settings.cosmetic_enabled,
        storage.settings.total_blocked,
        storage.settings.whitelisted_domains.clone(),
    )));
    let dns_guard = Arc::new(DnsGuard::new());
    let dns_enabled = Arc::new(AtomicBool::new(storage.settings.dns_protection));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let event_proxy = event_loop.create_proxy();

    let (proxy_tx, proxy_rx) = mpsc::channel();
    let local_proxy = LocalSecurityProxy::start(
        adblock.clone(),
        dns_guard.clone(),
        dns_enabled.clone(),
        proxy_tx,
    )
    .expect("Yerel güvenlik proxy'si başlatılamadı");

    let proxy_evt = event_proxy.clone();
    thread::spawn(move || {
        while let Ok(evt) = proxy_rx.recv() {
            let _ = proxy_evt.send_event(UserEvent::Proxy(evt));
        }
    });

    let window_icon = tao::window::Icon::from_rgba(
        include_bytes!("../assets/icon_128.rgba").to_vec(),
        128,
        128,
    )
    .ok();

    let window = WindowBuilder::new()
        .with_title("AdBlock Tarayıcı — Başlatıcı")
        .with_window_icon(window_icon)
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(780.0, 500.0))
        .with_resizable(false)
        .build(&event_loop)
        .expect("Ana pencere oluşturulamadı");
    center_window_logical(&window, 780.0, 500.0);

    let ep_bar_ipc = event_proxy.clone();
    let ep_bar_load = event_proxy.clone();
    let chrome_bar = WebViewBuilder::new_with_web_context(&mut ui_web_context)
        .with_bounds(Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: WrySize::new(1280.0, 110.0).into(),
        })
        .with_html(chrome_bar_html())
        .with_ipc_handler(move |req| {
            let _ = ep_bar_ipc.send_event(UserEvent::Ipc {
                tab_id: None,
                raw: req.body().clone(),
            });
        })
        .with_on_page_load_handler(move |ev, _| {
            if matches!(ev, PageLoadEvent::Finished) {
                let _ = ep_bar_load.send_event(UserEvent::ChromeReady);
            }
        })
        .build_as_child(&window)
        .expect("Üst araç çubuğu oluşturulamadı");
    let _ = chrome_bar.set_visible(false);

    let launcher_stats = LauncherProfileStats {
        blocked_total: storage.settings.total_blocked,
        password_count: passwords.credentials.len(),
        bookmark_count: storage.bookmarks.len(),
    };
    let ep_ln_ipc = event_proxy.clone();
    let ep_ln_load = event_proxy.clone();
    let launcher_webview = WebViewBuilder::new_with_web_context(&mut ui_web_context)
        .with_bounds(Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: WrySize::new(780.0, 500.0).into(),
        })
        .with_html(launcher_html(
            storage.settings.auto_launch_if_current,
            &storage.settings.update_manifest_url,
            !storage.settings.update_token.is_empty(),
            &launcher_stats,
        ))
        .with_ipc_handler(move |req| {
            let _ = ep_ln_ipc.send_event(UserEvent::Ipc {
                tab_id: None,
                raw: req.body().clone(),
            });
        })
        .with_on_page_load_handler(move |ev, _| {
            if matches!(ev, PageLoadEvent::Finished) {
                let _ = ep_ln_load.send_event(UserEvent::LauncherReady);
            }
        })
        .build_as_child(&window)
        .expect("Başlatıcı oluşturulamadı");

    let download_dir_shared = Arc::new(RwLock::new(storage.effective_download_dir()));

    let mut app = BrowserApp {
        window,
        event_proxy,
        proxy_port: local_proxy.port,
        storage,
        download_dir_shared,
        passwords,
        adblock,
        dns_guard,
        dns_enabled,
        ui_web_context,
        tab_web_context,
        chrome_bar,
        chrome_ready: false,
        side_panel: None,
        side_panel_mode: None,
        popover: None,
        popover_rect: None,
        active_popover_kind: None,
        tabs: Vec::new(),
        active_tab_idx: 0,
        next_tab_id: 1,
        pending_password: None,
        launcher_webview: Some(launcher_webview),
        launcher_ready: false,
        in_launcher: true,
        video_fullscreen: false,
        pre_fullscreen_maximized: false,
        updating_now: false,
        pending_update: None,
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _keep_proxy_alive = &local_proxy;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                app.save_session_tabs();
                app.storage.save_settings();
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                app.close_popover();
                app.layout_views();
            }
            Event::UserEvent(ue) => match ue {
                UserEvent::LauncherReady => {
                    app.launcher_ready = true;
                    app.start_update_check();
                }
                UserEvent::UpdateChecked(bundle) => match bundle.result {
                    UpdateCheckResult::UpToDate { current, latest } => {
                        app.pending_update = None;
                        let auto = app.storage.settings.auto_launch_if_current;
                        app.set_launcher_state(serde_json::json!({
                            "dot": "ok",
                            "badge": "Güncel",
                            "badge_cls": "ok",
                            "title": format!("Sürüm güncel (v{current})"),
                            "sub": if auto {
                                format!("Son sürüm v{latest} doğrulandı. Tarayıcı otomatik başlatılıyor...")
                            } else {
                                format!("En güncel kararlı sürüm (v{latest}) yüklü.")
                            },
                            "show_progress": false,
                            "show_cfg_hint": false,
                            "checking": false,
                            "can_update": false,
                            "updating": false,
                            "releases": bundle.remote_releases,
                            "commits": bundle.remote_commits,
                        }));
                        if auto {
                            let ep = app.event_proxy.clone();
                            thread::spawn(move || {
                                thread::sleep(std::time::Duration::from_millis(900));
                                let _ = ep.send_event(UserEvent::AutoLaunchBrowser);
                            });
                        }
                    }
                    UpdateCheckResult::UpdateAvailable(info) => {
                        let has_asset = info.asset_browser_url.is_some();
                        let ver = info.version.clone();
                        let notes = if info.notes.trim().is_empty() {
                            format!("Yeni sürüm v{ver} indirilebilir durumda (mevcut: v{APP_VERSION}).")
                        } else {
                            info.notes.clone()
                        };
                        app.pending_update = Some(info);
                        app.set_launcher_state(serde_json::json!({
                            "dot": "upd",
                            "badge": format!("Yeni: v{ver}"),
                            "badge_cls": "upd",
                            "title": format!("Yeni sürüm mevcut: v{ver}"),
                            "sub": notes,
                            "show_progress": false,
                            "show_cfg_hint": false,
                            "checking": false,
                            "can_update": has_asset,
                            "updating": false,
                            "update_label": format!("v{ver} Sürümüne Güncelle"),
                            "releases": bundle.remote_releases,
                            "commits": bundle.remote_commits,
                        }));
                    }
                    UpdateCheckResult::Unavailable { current, reason } => {
                        app.pending_update = None;
                        app.set_launcher_state(serde_json::json!({
                            "dot": "warn",
                            "badge": "Yerel Mod",
                            "badge_cls": "",
                            "title": format!("Mevcut sürüm: v{current}"),
                            "sub": reason,
                            "show_progress": false,
                            "show_cfg_hint": true,
                            "checking": false,
                            "can_update": false,
                            "updating": false,
                            "releases": bundle.remote_releases,
                            "commits": bundle.remote_commits,
                        }));
                    }
                },
                UserEvent::UpdateProgress { percent, text } => {
                    app.set_launcher_state(serde_json::json!({
                        "dot": "upd",
                        "badge": format!("%{percent}"),
                        "badge_cls": "upd",
                        "title": "Güncelleme indiriliyor...",
                        "sub": text,
                        "show_progress": true,
                        "indeterminate": false,
                        "progress": percent,
                        "show_cfg_hint": false,
                        "can_update": true,
                        "updating": true,
                        "update_label": format!("İndiriliyor %{percent}")
                    }));
                }
                UserEvent::UpdateFinished(res) => {
                    app.updating_now = false;
                    match res {
                        Ok(true) => {
                            app.save_session_tabs();
                            app.storage.save_settings();
                            app.launcher_webview.take();
                            app.tabs.clear();
                            app.popover.take();
                            app.side_panel.take();
                            *control_flow = ControlFlow::Exit;
                        }
                        Ok(false) => {
                            app.set_launcher_state(serde_json::json!({
                                "dot": "ok",
                                "badge": "İndirildi",
                                "badge_cls": "ok",
                                "title": "Güncelleme paketi indirildi",
                                "sub": "İndirilen paket klasörde açıldı.",
                                "show_progress": false,
                                "show_cfg_hint": false,
                                "can_update": false,
                                "updating": false,
                            }));
                        }
                        Err(err) => {
                            app.set_launcher_state(serde_json::json!({
                                "dot": "warn",
                                "badge": "Hata",
                                "badge_cls": "",
                                "title": "Güncelleme tamamlanamadı",
                                "sub": err,
                                "show_progress": false,
                                "show_cfg_hint": true,
                                "can_update": app.pending_update.is_some(),
                                "updating": false,
                                "update_label": "Tekrar Dene"
                            }));
                        }
                    }
                }
                UserEvent::AutoLaunchBrowser => {
                    app.launch_browser_from_launcher();
                }
                UserEvent::CloseLauncher => {
                    app.save_session_tabs();
                    app.storage.save_settings();
                    *control_flow = ControlFlow::Exit;
                }
                UserEvent::ChromeReady => {
                    app.chrome_ready = true;
                    app.sync_chrome_bar();
                }
                UserEvent::Ipc { tab_id, raw } => {
                    app.handle_ipc(tab_id, &raw);
                }
                UserEvent::TabTitleChanged { tab_id, title } => {
                    if let Some(t) = app.tabs.iter_mut().find(|t| t.id == tab_id) {
                        if t.internal_route.is_none() && !title.trim().is_empty() {
                            t.title = title;
                        }
                    }
                    app.sync_chrome_bar();
                }
                UserEvent::TabPageLoad {
                    tab_id,
                    finished,
                    url,
                } => {
                    if !finished {
                        if url.starts_with("http") {
                            if let Some(pos) = app.tabs.iter().position(|t| t.id == tab_id) {
                                app.tabs[pos].internal_route = None;
                                if extract_host(&url) != extract_host(&app.tabs[pos].url) {
                                    app.tabs[pos].favicon_url.clear();
                                }
                                app.tabs[pos].url = url.clone();
                                app.sync_chrome_bar();
                            }
                        }
                    } else {
                        if let Some(pos) = app.tabs.iter().position(|t| t.id == tab_id) {
                            if app.tabs[pos].internal_route.is_some() && url.starts_with("http") {
                                app.tabs[pos].internal_route = None;
                            }
                            let is_internal = app.tabs[pos].internal_route.is_some();
                            if !is_internal && url.starts_with("http") {
                                if extract_host(&url) != extract_host(&app.tabs[pos].url) {
                                    app.tabs[pos].favicon_url.clear();
                                }
                                app.tabs[pos].url = url.clone();
                                let title = app.tabs[pos].title.clone();
                                app.storage.record_history(&url, &title);
                                let js = app.adblock.read().unwrap().build_page_script(&url);
                                if !js.is_empty() {
                                    let _ = app.tabs[pos].webview.evaluate_script(&js);
                                }
                                app.push_pw_hints_to_tab(pos);
                            }
                            let _ = app.tabs[pos].webview.zoom(app.storage.settings.effective_zoom());
                            if pos == app.active_tab_idx {
                                if let Ok(mut ab) = app.adblock.write() {
                                    ab.active_page_url = app.tabs[pos].url.clone();
                                }
                            }
                        }
                        app.save_session_tabs();
                        app.sync_chrome_bar();
                    }
                }
                UserEvent::OpenNewTab { url } => {
                    app.create_tab(&url, true);
                }
                UserEvent::Proxy(pe) => match pe {
                    ProxyEvent::Blocked(item) => {
                        if let Some(t) = app.tabs.get_mut(app.active_tab_idx) {
                            if t.blocked_items.len() < 250 {
                                t.blocked_items.push(item);
                            }
                        }
                        if let Ok(mut ab) = app.adblock.write() {
                            ab.total_blocked += 1;
                            app.storage.settings.total_blocked = ab.total_blocked;
                        }
                        app.sync_chrome_bar();
                    }
                    ProxyEvent::DnsBlocked { host, reason } => {
                        if let Some(t) = app.tabs.get_mut(app.active_tab_idx) {
                            if t.blocked_items.len() < 250 {
                                t.blocked_items.push(BlockedItem {
                                    resource_type: "dns".to_string(),
                                    request_url: format!("https://{host}/"),
                                    rule: format!("dns-guard: {reason}"),
                                });
                            }
                        }
                        app.sync_chrome_bar();
                    }
                },
                UserEvent::DownloadStarted { url, save_path } => {
                    let fname = PathBuf::from(&save_path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("dosya")
                        .to_string();
                    app.storage.downloads.insert(
                        0,
                        DownloadEntry {
                            id: format!("dl-{}", Local::now().timestamp_millis()),
                            filename: fname,
                            url,
                            save_path,
                            status: "İndiriliyor".to_string(),
                            started_at: Local::now().format("%Y-%m-%d %H:%M").to_string(),
                        },
                    );
                    app.storage.save_downloads();
                    app.reload_active_internal_if_matching();
                }
                UserEvent::DownloadFinished { url, ok } => {
                    if let Some(entry) = app.storage.downloads.iter_mut().find(|d| d.url == url) {
                        entry.status = if ok {
                            "Tamamlandı".to_string()
                        } else {
                            "İptal/Hata".to_string()
                        };
                    }
                    app.storage.save_downloads();
                    app.reload_active_internal_if_matching();
                }
                UserEvent::FilterListsUpdated => {
                    app.reload_active_internal_if_matching();
                    app.sync_chrome_bar();
                }
            },
            _ => {}
        }
    });
}
