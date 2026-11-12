use adblock::lists::{FilterFormat, FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use url::Url;

pub struct FilterListDef {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
}

pub const BUILTIN_FILTER_LISTS: &[FilterListDef] = &[
    FilterListDef {
        id: "peter_lowe",
        name: "Peter Lowe's Ad and tracking server list",
        url: "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=adblockplus&showintro=0&mimetype=plaintext",
    },
    FilterListDef {
        id: "easylist_domains",
        name: "EasyList Adservers",
        url: "https://easylist.to/easylist/easylist.txt",
    },
];

pub const DEFAULT_RULES: &str = r#"
! Core Ad & Tracker Blocking Rules
||doubleclick.net^
||googlesyndication.com^
||googleadservices.com^
||adservice.google.com^
||adservice.google.com.tr^
||pagead2.googlesyndication.com^
||tpc.googlesyndication.com^
||google-analytics.com^
||analytics.google.com^
||googletagmanager.com^
||googletagservices.com^
||connect.facebook.net^$third-party
||pixel.facebook.com^
||an.facebook.com^
||ads.twitter.com^
||ads-twitter.com^
||static.ads-twitter.com^
||analytics.tiktok.com^
||ads.tiktok.com^
||taboola.com^
||outbrain.com^
||criteo.com^
||criteo.net^
||adnxs.com^
||rubiconproject.com^
||pubmatic.com^
||openx.net^
||casalemedia.com^
||scorecardresearch.com^
||quantserve.com^
||hotjar.com^
||mixpanel.com^
||segment.io^
||segment.com^
||sentry.io^
||ads.yahoo.com^
||advertising.com^
||amazon-adsystem.com^
||adroll.com^
||zedo.com^
||popads.net^
||popcash.net^
||propellerads.com^
||mgid.com^
||revcontent.com^
||yandexadexchange.net^

! YouTube Specific Ad & Telemetry Endpoints
||youtube.com/pagead/
||youtube.com/api/stats/ads
||youtube.com/ptracking
||youtube.com/get_midroll_info
||youtube.com/youtubei/v1/player/ad_break
||googlevideo.com/videoplayback?*oad=
||googlevideo.com/videoplayback?*ctier=L
@@||www.gstatic.com^
@@||jnn-pa.googleapis.com^
@@||accounts.google.com^
@@||fonts.googleapis.com^
@@||fonts.gstatic.com^

! Cosmetic Hiding Rules
##.adsbygoogle
##ins.adsbygoogle
##[id^="google_ads_iframe"]
##div[id^="div-gpt-ad"]
##.ad-banner
##.ad-container
##.advertisement
youtube.com##ytd-ad-slot-renderer
youtube.com##ytd-banner-promo-renderer
youtube.com##ytd-promoted-sparkles-web-renderer
youtube.com##ytd-display-ad-renderer
youtube.com##ytd-companion-slot-renderer
youtube.com##ytd-player-legacy-desktop-watch-ads-renderer
youtube.com##ytd-action-companion-ad-renderer
youtube.com##ytd-in-feed-ad-layout-renderer
youtube.com###masthead-ad
youtube.com###player-ads
youtube.com##.ytp-ad-module
youtube.com##.ytp-ad-overlay-container
"#;

#[derive(Debug, Clone)]
pub struct BlockedItem {
    pub resource_type: String,
    pub request_url: String,
    pub rule: String,
}

#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub blocked: bool,
    pub rule: String,
}

pub fn extract_host(raw: &str) -> String {
    if let Ok(parsed) = Url::parse(raw) {
        return parsed.host_str().unwrap_or("").to_lowercase();
    }
    raw.trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

pub fn iter_parent_domains(host: &str) -> Vec<String> {
    let clean = host.trim().trim_matches('.').to_lowercase();
    if clean.is_empty() {
        return Vec::new();
    }
    let parts: Vec<&str> = clean.split('.').collect();
    let mut out = Vec::with_capacity(parts.len());
    for i in 0..parts.len() {
        out.push(parts[i..].join("."));
    }
    out
}

pub struct AdBlockService {
    pub enabled: bool,
    pub cosmetic_enabled: bool,
    pub total_blocked: u64,
    pub total_rules_count: usize,
    pub whitelisted_domains: HashSet<String>,
    pub custom_rules_text: String,
    pub custom_rules_path: PathBuf,
    pub filter_dir: PathBuf,
    pub active_page_url: String,
    engine: Engine,
}

/// Early document-start ad & tracker hook (network interception + YouTube player JSON pruner & non-stalling skipper).
pub const EARLY_ADBLOCK_INIT_JS: &str = r##"
(() => {
  if (window.__abEarlyInit) return;
  window.__abEarlyInit = true;
  if (!location.protocol.startsWith('http')) return;
  if (typeof window.__abDisabled !== 'boolean') window.__abDisabled = false;

  // 1. Document-start fetch / XHR path-level ad blocking + reporting
  if (!window.__abNetHook) {
    window.__abNetHook = true;
    const adPatterns = [
      'doubleclick.net', 'googlesyndication.com', 'googleadservices.com',
      '/pagead/', '/api/stats/ads', '/ptracking', '/get_midroll_info',
      '/youtubei/v1/player/ad_break', 'adservice.google.', '/pcs/activeview'
    ];
    const isAdUrl = (u) => !window.__abDisabled && typeof u === 'string' && adPatterns.some((p) => u.includes(p));
    const reportBlocked = (rtype, url, rule) => {
      try {
        if (window.ipc) window.ipc.postMessage(JSON.stringify({ type: 'blocked_req', rtype, url, rule }));
      } catch (_) {}
    };

    const origFetch = window.fetch;
    window.fetch = function(input) {
      const u = typeof input === 'string' ? input : (input && input.url) || '';
      if (isAdUrl(u)) {
        reportBlocked('fetch', u, 'in-page-ad-rule');
        return Promise.resolve(new Response('', { status: 204, statusText: 'No Content' }));
      }
      return origFetch.apply(this, arguments);
    };

    const origOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function(_method, url) {
      this.__abUrl = String(url || '');
      return origOpen.apply(this, arguments);
    };
    const origSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function() {
      if (isAdUrl(this.__abUrl)) {
        reportBlocked('xhr', this.__abUrl, 'in-page-ad-rule');
        try { this.abort(); } catch (_) {}
        return;
      }
      return origSend.apply(this, arguments);
    };
  }

  // 2. YouTube document-start JSON pruner, instant ad CSS, and MSE-safe skipper
  const h = location.hostname.toLowerCase();
  if ((h.endsWith('youtube.com') || h.endsWith('youtu.be')) && !window.__abYtInit) {
    window.__abYtInit = true;
    const pruneKeys = ['adPlacements', 'playerAds', 'adSlots', 'adBreakHeartbeatParams'];
    const pruneObj = (obj, depth = 0) => {
      if (window.__abDisabled || !obj || typeof obj !== 'object' || depth > 4) return obj;
      for (const k of pruneKeys) {
        if (k in obj) delete obj[k];
      }
      if (obj.playerResponse && typeof obj.playerResponse === 'object') {
        pruneObj(obj.playerResponse, depth + 1);
      }
      if (obj.args && typeof obj.args === 'object' && obj.args.raw_player_response) {
        pruneObj(obj.args.raw_player_response, depth + 1);
      }
      return obj;
    };

    let _ytInitPr;
    try {
      if ('ytInitialPlayerResponse' in window) {
        _ytInitPr = pruneObj(window.ytInitialPlayerResponse);
      }
      Object.defineProperty(window, 'ytInitialPlayerResponse', {
        configurable: true,
        get() { return _ytInitPr; },
        set(v) { _ytInitPr = pruneObj(v); }
      });
    } catch (_) {}

    const origParse = JSON.parse;
    JSON.parse = function(text, reviver) {
      const parsed = origParse.call(this, text, reviver);
      return window.__abDisabled ? parsed : pruneObj(parsed);
    };
    const origRespJson = Response.prototype.json;
    Response.prototype.json = function() {
      return origRespJson.call(this).then((d) => (window.__abDisabled ? d : pruneObj(d)));
    };

    const injectEarlyYtCss = () => {
      if (window.__abDisabled || document.getElementById('__ab_yt_early_css')) return;
      const st = document.createElement('style');
      st.id = '__ab_yt_early_css';
      st.textContent = [
        'ytd-ad-slot-renderer, ytd-banner-promo-renderer, ytd-promoted-sparkles-web-renderer,',
        'ytd-display-ad-renderer, ytd-companion-slot-renderer, ytd-player-legacy-desktop-watch-ads-renderer,',
        'ytd-action-companion-ad-renderer, ytd-in-feed-ad-layout-renderer, #masthead-ad, #player-ads,',
        '.ytp-ad-module, .ytp-ad-overlay-container, .ytp-ad-player-overlay, .ytp-ad-image-overlay, .video-ads',
        '{ display: none !important; }',
        '.html5-video-player.ad-showing video, .html5-video-player.ad-interrupting video',
        '{ opacity: 0 !important; }'
      ].join(' ');
      (document.head || document.documentElement).appendChild(st);
    };
    if (document.documentElement) injectEarlyYtCss();
    else document.addEventListener('DOMContentLoaded', injectEarlyYtCss, { once: true });

    let wasMuted = null;
    const skipTick = () => {
      if (window.__abDisabled) return;
      try {
        const player = document.querySelector('#movie_player');
        const video = document.querySelector('video.html5-main-video');
        const adShowing = !!(player && (player.classList.contains('ad-showing') || player.classList.contains('ad-interrupting')));
        if (adShowing) {
          const skip = document.querySelector('.ytp-skip-ad-button, .ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-ad-skip-button-slot button, button[id^="skip-button"]');
          if (skip) skip.click();
          if (video) {
            if (wasMuted === null) wasMuted = video.muted;
            video.muted = true;
            if (video.playbackRate < 16) video.playbackRate = 16;
            const sk = video.seekable;
            const seekEnd = sk && sk.length ? sk.end(sk.length - 1) : 0;
            if (isFinite(video.duration) && video.duration > 0.3 && seekEnd >= video.duration - 0.5 && video.currentTime < video.duration - 0.25) {
              video.currentTime = video.duration - 0.1;
            }
          }
        } else if (video) {
          if (wasMuted !== null) {
            video.muted = wasMuted;
            wasMuted = null;
          }
          if (video.playbackRate > 4) video.playbackRate = 1;
        }
      } catch (_) {}
    };
    setInterval(skipTick, 150);
    const attachSkipObserver = () => {
      if (window.__abYtMo || !window.MutationObserver || !document.documentElement) return;
      window.__abYtMo = true;
      const mo = new MutationObserver(skipTick);
      mo.observe(document.documentElement, { attributes: true, subtree: true, attributeFilter: ['class'] });
    };
    if (document.documentElement) attachSkipObserver();
    else document.addEventListener('DOMContentLoaded', attachSkipObserver, { once: true });
  }
})();
"##;

impl AdBlockService {
    pub fn load(
        data_dir: &PathBuf,
        enabled: bool,
        cosmetic_enabled: bool,
        total_blocked: u64,
        whitelist: Vec<String>,
    ) -> Self {
        let custom_rules_path = data_dir.join("custom_filters.txt");
        let filter_dir = data_dir.join("filter_lists");
        let _ = fs::create_dir_all(&filter_dir);
        let custom_rules_text = fs::read_to_string(&custom_rules_path).unwrap_or_default();
        let whitelisted_domains: HashSet<String> = whitelist
            .into_iter()
            .map(|d| d.trim().to_lowercase())
            .filter(|d| !d.is_empty())
            .collect();

        let (engine, count) = Self::compile_engine(&filter_dir, &custom_rules_text);
        Self {
            enabled,
            cosmetic_enabled,
            total_blocked,
            total_rules_count: count,
            whitelisted_domains,
            custom_rules_text,
            custom_rules_path,
            filter_dir,
            active_page_url: String::new(),
            engine,
        }
    }

    fn compile_engine(filter_dir: &PathBuf, custom_rules: &str) -> (Engine, usize) {
        let mut filter_set = FilterSet::new(true);
        let mut count = 0usize;

        let mut add_chunk = |text: &str| {
            let line_cnt = text
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    !t.is_empty() && !t.starts_with('!') && !t.starts_with('[')
                })
                .count();
            count += line_cnt;
            let mut opts = ParseOptions::default();
            opts.format = FilterFormat::Standard;
            filter_set.add_filter_list(text.to_string(), opts);
        };

        add_chunk(DEFAULT_RULES);
        for list in BUILTIN_FILTER_LISTS {
            let p = filter_dir.join(format!("{}.txt", list.id));
            if let Ok(content) = fs::read_to_string(p) {
                add_chunk(&content);
            }
        }
        if !custom_rules.trim().is_empty() {
            add_chunk(custom_rules);
        }

        (Engine::new_with_filter_set(filter_set), count)
    }

    pub fn reload_rules(&mut self) -> usize {
        let (eng, cnt) = Self::compile_engine(&self.filter_dir, &self.custom_rules_text);
        self.engine = eng;
        self.total_rules_count = cnt;
        cnt
    }

    pub fn set_custom_rules(&mut self, text: String) -> usize {
        self.custom_rules_text = text.clone();
        if let Some(parent) = self.custom_rules_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&self.custom_rules_path, &self.custom_rules_text);
        self.reload_rules()
    }

    pub fn is_site_whitelisted(&self, url_or_host: &str) -> bool {
        let host = if url_or_host.contains("://") {
            extract_host(url_or_host)
        } else {
            url_or_host.trim().to_lowercase()
        };
        if host.is_empty() {
            return false;
        }
        iter_parent_domains(&host)
            .iter()
            .any(|p| self.whitelisted_domains.contains(p))
    }

    pub fn set_site_whitelisted(&mut self, host: &str, paused: bool) {
        let clean = extract_host(host);
        if clean.is_empty() {
            return;
        }
        if paused {
            self.whitelisted_domains.insert(clean);
        } else {
            self.whitelisted_domains.remove(&clean);
        }
    }

    pub fn should_block(&self, request_url: &str, source_url: &str, resource_type: &str) -> CheckOutcome {
        if !self.enabled {
            return CheckOutcome {
                blocked: false,
                rule: String::new(),
            };
        }
        let eff_source = if source_url.is_empty() {
            &self.active_page_url
        } else {
            source_url
        };
        if !eff_source.is_empty() && self.is_site_whitelisted(eff_source) {
            return CheckOutcome {
                blocked: false,
                rule: String::new(),
            };
        }
        let req_host = extract_host(request_url);
        if !req_host.is_empty() && self.is_site_whitelisted(&req_host) {
            return CheckOutcome {
                blocked: false,
                rule: String::new(),
            };
        }

        let src = if eff_source.is_empty() {
            request_url
        } else {
            eff_source
        };
        let Ok(req) = Request::new(request_url, src, resource_type, "GET") else {
            return CheckOutcome {
                blocked: false,
                rule: String::new(),
            };
        };
        let res = self.engine.check_network_request(&req);
        let blocked = res.should_block();
        let rule = res
            .filter
            .and_then(|f| f.raw_line)
            .unwrap_or_else(|| if blocked { "adblock-rule".to_string() } else { String::new() });
        CheckOutcome { blocked, rule }
    }

    pub fn update_filter_lists(&mut self) -> Vec<(String, Result<usize, String>)> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(12))
            .build();
        let mut results = Vec::new();
        for item in BUILTIN_FILTER_LISTS {
            let dest = self.filter_dir.join(format!("{}.txt", item.id));
            match agent.get(item.url).call() {
                Ok(resp) => match resp.into_string() {
                    Ok(body) => {
                        let lines = body.lines().count();
                        if let Err(e) = fs::write(&dest, &body) {
                            results.push((item.name.to_string(), Err(e.to_string())));
                        } else {
                            results.push((item.name.to_string(), Ok(lines)));
                        }
                    }
                    Err(e) => results.push((item.name.to_string(), Err(e.to_string()))),
                },
                Err(e) => results.push((item.name.to_string(), Err(e.to_string()))),
            }
        }
        self.reload_rules();
        results
    }

    /// Generates per-URL cosmetic CSS injection script when AdBlock is active.
    pub fn build_page_script(&self, page_url: &str) -> String {
        if !self.enabled || self.is_site_whitelisted(page_url) {
            return "window.__abDisabled = true; const s = document.getElementById('__ab_yt_early_css'); if (s) s.remove();".to_string();
        }
        if !self.cosmetic_enabled {
            return "window.__abDisabled = false;".to_string();
        }
        let resources = self.engine.url_cosmetic_resources(page_url);
        let mut selectors: Vec<String> = resources.hide_selectors.into_iter().collect();
        selectors.sort();
        let css_block = if selectors.is_empty() {
            String::new()
        } else {
            format!(
                "{} {{ display: none !important; visibility: hidden !important; }}",
                selectors.join(", ")
            )
        };
        let css_json = serde_json::to_string(&css_block).unwrap_or_else(|_| "\"\"".to_string());
        format!(
            r#"
(() => {{
  window.__abDisabled = false;
  const css = {css_json};
  if (css) {{
    const applyCss = () => {{
      if (document.getElementById('__ab_cosmetic_css')) return;
      const st = document.createElement('style');
      st.id = '__ab_cosmetic_css';
      st.textContent = css;
      (document.head || document.documentElement).appendChild(st);
    }};
    if (document.documentElement) applyCss();
    else document.addEventListener('DOMContentLoaded', applyCss, {{ once: true }});
  }}
}})();
"#
        )
    }
}

pub type SharedAdBlock = Arc<RwLock<AdBlockService>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_doubleclick_and_allows_whitelisted_site() {
        let tmp = std::env::temp_dir().join(format!("ab_test_eng_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut svc = AdBlockService::load(&tmp, true, true, 0, vec![]);
        assert!(svc.total_rules_count > 50);

        let blocked = svc.should_block(
            "https://static.doubleclick.net/instream/ad_status.js",
            "https://www.youtube.com/watch?v=1",
            "script",
        );
        assert!(blocked.blocked);

        let allowed = svc.should_block(
            "https://www.gstatic.com/images/logo.png",
            "https://www.google.com/",
            "image",
        );
        assert!(!allowed.blocked);

        // BotGuard / WebPO endpoint required for fast YouTube video playback must not be blocked
        let webpo = svc.should_block(
            "https://jnn-pa.googleapis.com/$rpc/google.internal.waa.v1.Waa/Create",
            "https://www.youtube.com/watch?v=1",
            "xhr",
        );
        assert!(!webpo.blocked);
        assert!(EARLY_ADBLOCK_INIT_JS.contains("ytInitialPlayerResponse"));
        assert!(EARLY_ADBLOCK_INIT_JS.contains("video.seekable"));

        svc.set_site_whitelisted("www.youtube.com", true);
        let after_wl = svc.should_block(
            "https://static.doubleclick.net/instream/ad_status.js",
            "https://www.youtube.com/watch?v=1",
            "script",
        );
        assert!(!after_wl.blocked);

        svc.set_custom_rules("||evil-tracker.example^".to_string());
        let custom = svc.should_block(
            "https://sub.evil-tracker.example/pixel.js",
            "https://news.example/",
            "script",
        );
        assert!(custom.blocked);

        let _ = fs::remove_dir_all(&tmp);
    }
}
