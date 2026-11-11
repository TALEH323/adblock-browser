mod adblock_engine;
mod chrome_ui;
mod dns_guard;
mod internal_pages;
mod passwords;
mod proxy;
mod storage;

use crate::adblock_engine::{extract_host, AdBlockService, BlockedItem, SharedAdBlock};
use crate::chrome_ui::{
    chrome_bar_html, render_popover_html, render_side_panel_html, BarUiState, PopoverKind,
    TabUiItem,
};
use crate::dns_guard::DnsGuard;
use crate::passwords::{PasswordManager, PASSWORD_INJECT_JS};
use crate::proxy::{LocalSecurityProxy, ProxyEvent};
use crate::storage::{resolve_data_dir, BrowserStorage, DownloadEntry};
use chrono::Local;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use wry::dpi::{LogicalPosition, LogicalSize as WrySize};
use wry::{NewWindowResponse, PageLoadEvent, ProxyConfig, ProxyEndpoint, Rect, WebView, WebViewBuilder};

const TAB_SHORTCUTS_JS: &str = r###"
(() => {
  if (window.__abShortcuts) return;
  window.__abShortcuts = true;
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
    idx: Option<usize>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    rtype: String,
    #[serde(default)]
    rule: String,
    #[serde(default)]
    height: Option<f64>,
}

struct BrowserTab {
    id: usize,
    title: String,
    url: String,
    internal_route: Option<String>,
    blocked_items: Vec<BlockedItem>,
    webview: WebView,
}

struct BrowserApp {
    window: tao::window::Window,
    event_proxy: EventLoopProxy<UserEvent>,
    proxy_port: u16,
    storage: BrowserStorage,
    passwords: PasswordManager,
    adblock: SharedAdBlock,
    dns_guard: Arc<DnsGuard>,
    dns_enabled: Arc<AtomicBool>,
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

    fn layout_views(&self) {
        let (w, h) = self.logical_size();
        let bh = self.bar_height();
        let _ = self.chrome_bar.set_bounds(Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: WrySize::new(w, bh).into(),
        });

        let sp_w = if self.side_panel.is_some() { 340.0 } else { 0.0 };
        let content_w = (w - sp_w).max(200.0);
        let content_h = (h - bh).max(120.0);

        if let Some(sp) = &self.side_panel {
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
        let ep = self.event_proxy.clone();
        let ep_ipc = ep.clone();
        let ep_title = ep.clone();
        let ep_load = ep.clone();
        let ep_new = ep.clone();
        let ep_dl_start = ep.clone();
        let ep_dl_done = ep.clone();

        let (w, h) = self.logical_size();
        let bh = self.bar_height();
        let sp_w = if self.side_panel.is_some() { 340.0 } else { 0.0 };
        let rect = Rect {
            position: LogicalPosition::new(0.0, bh).into(),
            size: WrySize::new((w - sp_w).max(200.0), (h - bh).max(120.0)).into(),
        };

        let downloads_dir = dirs::download_dir().unwrap_or_else(|| self.storage.data_dir.join("downloads"));
        let _ = std::fs::create_dir_all(&downloads_dir);

        let webview = WebViewBuilder::new()
            .with_bounds(rect)
            .with_visible(activate)
            .with_devtools(true)
            .with_clipboard(true)
            .with_proxy_config(ProxyConfig::Http(ProxyEndpoint {
                host: "127.0.0.1".to_string(),
                port: self.proxy_port.to_string(),
            }))
            .with_initialization_script(TAB_SHORTCUTS_JS)
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
            .build_as_child(&self.window)
            .expect("Tab WebView oluşturulamadı");

        let tab = BrowserTab {
            id: tab_id,
            title: "Yeni Sekme".to_string(),
            url: "adblock://home".to_string(),
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
        self.layout_views();
        self.sync_chrome_bar();
    }

    fn close_tab_by_id(&mut self, id: usize) {
        if self.tabs.len() <= 1 {
            self.navigate_tab_idx(0, "adblock://home");
            return;
        }
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            self.tabs.remove(pos);
            if self.active_tab_idx >= self.tabs.len() {
                self.active_tab_idx = self.tabs.len() - 1;
            }
            self.close_popover();
            self.layout_views();
            self.sync_chrome_bar();
        }
    }

    fn switch_to_tab_id(&mut self, id: usize) {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
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
            t.title = match route_name {
                "settings" => "Ayarlar",
                "downloads" => "İndirilenler",
                "passwords" => "Şifreler",
                "history" => "Geçmiş",
                "bookmarks" => "Yer İmleri",
                "adblock" => "AdBlock Yönetimi",
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
                t.title = "Güvenlik Uyarısı".to_string();
                let _ = t.webview.load_html(&html);
                self.sync_chrome_bar();
                return;
            }
        }

        let t = &mut self.tabs[idx];
        t.internal_route = None;
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
                .map(|(i, t)| TabUiItem {
                    id: t.id,
                    title: t.title.clone(),
                    active: i == self.active_tab_idx,
                    blocked_count: t.blocked_items.len(),
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
        if let Ok(wv) = WebViewBuilder::new()
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
            if let Ok(wv) = WebViewBuilder::new()
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

    fn handle_ipc(&mut self, _source_tab_id: Option<usize>, raw: &str) {
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
                self.sync_chrome_bar();
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
                    self.sync_chrome_bar();
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
            _ => {}
        }
    }

    fn apply_setting(&mut self, key: &str, val: Option<serde_json::Value>) {
        let Some(v) = val else { return };
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
            _ => {}
        }
        self.storage.save_settings();
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

fn main() {
    let data_dir = resolve_data_dir();
    let storage = BrowserStorage::load(data_dir.clone());
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
        .with_title("AdBlock Tarayıcı (Rust)")
        .with_window_icon(window_icon)
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .build(&event_loop)
        .expect("Ana pencere oluşturulamadı");

    let ep_bar_ipc = event_proxy.clone();
    let ep_bar_load = event_proxy.clone();
    let chrome_bar = WebViewBuilder::new()
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

    let mut app = BrowserApp {
        window,
        event_proxy,
        proxy_port: local_proxy.port,
        storage,
        passwords,
        adblock,
        dns_guard,
        dns_enabled,
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
    };

    let home = app.storage.settings.home_url.clone();
    app.create_tab(&home, true);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _keep_proxy_alive = &local_proxy;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
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
                    if finished {
                        if let Some(pos) = app.tabs.iter().position(|t| t.id == tab_id) {
                            let is_internal = app.tabs[pos].internal_route.is_some();
                            if !is_internal && url.starts_with("http") {
                                app.tabs[pos].url = url.clone();
                                let title = app.tabs[pos].title.clone();
                                app.storage.record_history(&url, &title);
                                let js = app.adblock.read().unwrap().build_page_script(&url);
                                if !js.is_empty() {
                                    let _ = app.tabs[pos].webview.evaluate_script(&js);
                                }
                            }
                            if pos == app.active_tab_idx {
                                if let Ok(mut ab) = app.adblock.write() {
                                    ab.active_page_url = app.tabs[pos].url.clone();
                                }
                            }
                        }
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
