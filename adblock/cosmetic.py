"""Cosmetic CSS element-hiding and YouTube ad-pruning script generator."""

from __future__ import annotations

import json
from typing import List


def build_youtube_early_script() -> str:
    """Return a DocumentCreation MainWorld script that strips YouTube ad payloads and fast-skips fallback ads."""
    return r"""
    (function() {
        'use strict';
        if (!/(^|\.)youtube\.com$/i.test(location.hostname)) return;
        if (window.__adblockYtEarlyInstalled) return;
        window.__adblockYtEarlyInstalled = true;

        const AD_KEYS = ['adPlacements', 'playerAds', 'adSlots', 'adBreakHeartbeatParams'];

        function pruneAdObject(obj) {
            if (!obj || typeof obj !== 'object') return obj;
            for (const key of AD_KEYS) {
                if (key in obj) {
                    if (Array.isArray(obj[key])) {
                        obj[key] = [];
                    } else {
                        delete obj[key];
                    }
                }
            }
            if (obj.playerResponse && typeof obj.playerResponse === 'object') {
                pruneAdObject(obj.playerResponse);
            }
            if (Array.isArray(obj)) {
                for (let i = 0; i < obj.length; i++) {
                    if (obj[i] && typeof obj[i] === 'object' && obj[i].playerResponse) {
                        pruneAdObject(obj[i].playerResponse);
                    }
                }
            }
            return obj;
        }

        // 1. Intercept window.ytInitialPlayerResponse assignment before player initializes
        let _ytInitialPlayerResponse = undefined;
        try {
            if ('ytInitialPlayerResponse' in window && window.ytInitialPlayerResponse) {
                _ytInitialPlayerResponse = pruneAdObject(window.ytInitialPlayerResponse);
            }
            Object.defineProperty(window, 'ytInitialPlayerResponse', {
                configurable: true,
                enumerable: true,
                get() {
                    return _ytInitialPlayerResponse;
                },
                set(val) {
                    _ytInitialPlayerResponse = pruneAdObject(val);
                }
            });
        } catch (e) {}

        // 2. Intercept JSON.parse for inline/XHR/SPA player responses
        const origParse = JSON.parse;
        JSON.parse = function(text, reviver) {
            const parsed = origParse.call(this, text, reviver);
            if (parsed && typeof parsed === 'object') {
                if ('adPlacements' in parsed || 'playerAds' in parsed || 'adSlots' in parsed || 'playerResponse' in parsed) {
                    pruneAdObject(parsed);
                }
            }
            return parsed;
        };

        // 3. Intercept Response.prototype.json for fetch()-based SPA navigations
        if (typeof Response !== 'undefined' && Response.prototype && Response.prototype.json) {
            const origJson = Response.prototype.json;
            Response.prototype.json = function() {
                return origJson.apply(this, arguments).then(data => pruneAdObject(data));
            };
        }

        // 4. Backup fallback: fast-skip & mute if any ad slot still triggers in #movie_player
        let savedMutedState = null;
        function skipActiveAd() {
            const player = document.querySelector('#movie_player');
            if (!player) return;

            const isAdShowing = player.classList.contains('ad-showing') || player.classList.contains('ad-interrupting');
            const video = player.querySelector('video.html5-main-video, video');

            if (isAdShowing) {
                const skipSelectors = [
                    '.ytp-skip-ad-button',
                    '.ytp-ad-skip-button',
                    '.ytp-ad-skip-button-modern',
                    '.ytp-ad-skip-button-container button',
                    '[id^="skip-button"] button'
                ];
                for (const sel of skipSelectors) {
                    const btn = document.querySelector(sel);
                    if (btn) {
                        try { btn.click(); } catch (e) {}
                    }
                }

                if (video) {
                    if (savedMutedState === null) {
                        savedMutedState = video.muted;
                    }
                    video.muted = true;
                    try { video.playbackRate = 16; } catch (e) {}
                    if (isFinite(video.duration) && video.duration > 0) {
                        try { video.currentTime = video.duration; } catch (e) {}
                    }
                }
            } else if (savedMutedState !== null && video) {
                video.muted = savedMutedState;
                try { video.playbackRate = 1; } catch (e) {}
                savedMutedState = null;
            }
        }

        const startObserver = () => {
            const root = document.documentElement || document;
            if (!root) return;
            const obs = new MutationObserver(skipActiveAd);
            obs.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['class'] });
            setInterval(skipActiveAd, 150);
        };

        if (document.documentElement) {
            startObserver();
        } else {
            document.addEventListener('DOMContentLoaded', startObserver, { once: true });
        }
    })();
    """


def build_cosmetic_script(selectors: List[str], page_url: str) -> str:
    """Build JavaScript that injects cosmetic ad-hiding CSS into the page."""
    if not selectors:
        return ""

    # Chunk selectors so one malformed CSS selector doesn't invalidate the entire stylesheet
    chunk_size = 25
    css_rules: List[str] = []
    for i in range(0, len(selectors), chunk_size):
        chunk = ", ".join(selectors[i : i + chunk_size])
        css_rules.append(
            f"{chunk} {{ display: none !important; visibility: hidden !important; "
            f"height: 0 !important; min-height: 0 !important; opacity: 0 !important; pointer-events: none !important; }}"
        )

    css_text_json = json.dumps("\n".join(css_rules))

    return f"""
    (function() {{
        const STYLE_ID = '__adblock_cosmetic_style';
        const cssText = {css_text_json};

        function injectStyle() {{
            if (document.getElementById(STYLE_ID)) return;
            const parent = document.head || document.documentElement;
            if (!parent) return;
            const style = document.createElement('style');
            style.id = STYLE_ID;
            style.type = 'text/css';
            style.textContent = cssText;
            parent.appendChild(style);
        }}

        if (document.head || document.documentElement) {{
            injectStyle();
        }} else {{
            document.addEventListener('DOMContentLoaded', injectStyle, {{ once: true }});
        }}
    }})();
    """


def remove_cosmetic_script() -> str:
    """JavaScript to remove previously injected cosmetic styles."""
    return """
    (function() {
        const el = document.getElementById('__adblock_cosmetic_style');
        if (el) el.remove();
    })();
    """
