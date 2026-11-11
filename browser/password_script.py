"""Isolated-world (ApplicationWorld) QWebChannel script for password capture and autofill."""

from __future__ import annotations

from PyQt6.QtCore import QFile, QIODevice


def _load_qwebchannel_js() -> str:
    f = QFile(":/qtwebchannel/qwebchannel.js")
    if f.open(QIODevice.OpenModeFlag.ReadOnly):
        data = bytes(f.readAll()).decode("utf-8", errors="ignore")
        f.close()
        return data
    return ""


def build_password_manager_script() -> str:
    """Build the ApplicationWorld script combining qwebchannel.js and DOM password handlers."""
    qwebchannel_js = _load_qwebchannel_js()
    manager_js = r"""
    (function() {
        'use strict';
        if (!/^https?:$/i.test(location.protocol)) return;
        if (window.__adblockPwScriptInstalled) return;
        window.__adblockPwScriptInstalled = true;

        const LAST_USER_KEY = '__adblock_pw_last_user';
        let bridge = null;
        let lastSubmitKey = '';
        let lastSubmitAt = 0;

        function setNativeValue(input, value) {
            if (!input) return;
            const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
            if (setter) {
                setter.call(input, value);
            } else {
                input.value = value;
            }
            input.dispatchEvent(new Event('input', { bubbles: true }));
            input.dispatchEvent(new Event('change', { bubbles: true }));
        }

        function isVisible(el) {
            if (!el) return false;
            const style = getComputedStyle(el);
            if (style.display === 'none' || style.visibility === 'hidden') return false;
            const rect = el.getBoundingClientRect();
            return rect.width > 0 && rect.height > 0;
        }

        function findUsernameInput(pwInput) {
            const root = (pwInput && pwInput.form) || document;
            const candidates = Array.from(root.querySelectorAll(
                'input[type="email"], input[autocomplete*="user" i], input[autocomplete="email" i], ' +
                'input[name*="user" i], input[name*="email" i], input[name*="login" i], ' +
                'input[id*="user" i], input[id*="email" i], input[type="text"], input:not([type])'
            )).filter(el => el !== pwInput && !el.disabled && !el.readOnly && el.type !== 'password' && el.type !== 'hidden');

            // Prefer visible inputs that appear before the password field in DOM order
            const visible = candidates.filter(isVisible);
            const pool = visible.length ? visible : candidates;
            if (!pool.length) return null;

            if (pwInput) {
                for (let i = pool.length - 1; i >= 0; i--) {
                    const pos = pool[i].compareDocumentPosition(pwInput);
                    if (pos & Node.DOCUMENT_POSITION_FOLLOWING) {
                        return pool[i];
                    }
                }
            }
            return pool[0];
        }

        function rememberUsername(val) {
            if (!val || !val.trim()) return;
            try { sessionStorage.setItem(LAST_USER_KEY, val.trim()); } catch (e) {}
        }

        function getRememberedUsername() {
            try { return sessionStorage.getItem(LAST_USER_KEY) || ''; } catch (e) { return ''; }
        }

        function captureFromScope(scopeEl) {
            if (!bridge) return;
            const root = scopeEl || document;
            const pwInputs = Array.from(root.querySelectorAll('input[type="password"]'));
            for (const pwInput of pwInputs) {
                const pw = pwInput.value;
                if (!pw) continue;
                const userInput = findUsernameInput(pwInput);
                let username = userInput && userInput.value ? userInput.value.trim() : '';
                if (!username) {
                    username = getRememberedUsername();
                }
                const key = location.origin + '|' + username + '|' + pw;
                const now = Date.now();
                if (key === lastSubmitKey && (now - lastSubmitAt) < 1500) {
                    continue;
                }
                lastSubmitKey = key;
                lastSubmitAt = now;
                try {
                    bridge.onCredentialsSubmitted(location.origin, username, pw);
                } catch (e) {}
                break;
            }
        }

        function fillPair(pwInput, username, password) {
            const userInput = findUsernameInput(pwInput);
            if (userInput && username && !userInput.value) {
                setNativeValue(userInput, username);
            }
            if (pwInput && password) {
                setNativeValue(pwInput, password);
            }
        }

        window.__adblockFillCreds = function(username, password) {
            const pwInput = document.querySelector('input[type="password"]');
            if (pwInput) {
                fillPair(pwInput, username, password);
                return;
            }
            const userInput = findUsernameInput(null);
            if (userInput && username) {
                setNativeValue(userInput, username);
            }
        };

        let pickerEl = null;
        function removePicker() {
            if (pickerEl) {
                pickerEl.remove();
                pickerEl = null;
            }
        }

        function showMultiAccountPicker(anchorInput, creds) {
            removePicker();
            if (!anchorInput || !creds.length) return;
            const rect = anchorInput.getBoundingClientRect();
            pickerEl = document.createElement('div');
            pickerEl.style.cssText = [
                'position:fixed',
                'z-index:2147483647',
                'left:' + Math.max(8, rect.left) + 'px',
                'top:' + Math.min(window.innerHeight - 120, rect.bottom + 4) + 'px',
                'min-width:' + Math.max(210, rect.width) + 'px',
                'background:#161b22',
                'color:#e6edf3',
                'border:1px solid #30363d',
                'border-radius:8px',
                'box-shadow:0 8px 24px rgba(0,0,0,0.45)',
                'font-family:-apple-system,BlinkMacSystemFont,Segoe UI,sans-serif',
                'font-size:13px',
                'padding:4px'
            ].join(';');

            creds.forEach(c => {
                const row = document.createElement('div');
                row.style.cssText = 'display:flex;align-items:center;gap:8px;padding:7px 10px;border-radius:6px;cursor:pointer;';
                const iconWrap = document.createElement('span');
                iconWrap.innerHTML = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#8ab4f8" stroke-width="2"><circle cx="8" cy="14" r="4"/><path d="M11 11.2L19.5 3.5M17 5.8l2.5 2.5"/></svg>';
                const label = document.createElement('span');
                label.textContent = c.username || '(Kullanıcı adı yok)';
                row.appendChild(iconWrap);
                row.appendChild(label);
                row.addEventListener('mouseenter', () => { row.style.background = '#21262d'; });
                row.addEventListener('mouseleave', () => { row.style.background = 'transparent'; });
                row.addEventListener('mousedown', (ev) => {
                    ev.preventDefault();
                    const pwInput = anchorInput.type === 'password'
                        ? anchorInput
                        : (anchorInput.form || document).querySelector('input[type="password"]');
                    if (pwInput) {
                        fillPair(pwInput, c.username, c.password);
                    } else if (c.username) {
                        setNativeValue(anchorInput, c.username);
                    }
                    removePicker();
                });
                pickerEl.appendChild(row);
            });

            document.documentElement.appendChild(pickerEl);
            setTimeout(() => {
                document.addEventListener('mousedown', function onDocClick(ev) {
                    if (pickerEl && !pickerEl.contains(ev.target) && ev.target !== anchorInput) {
                        removePicker();
                        document.removeEventListener('mousedown', onDocClick, true);
                    }
                }, true);
            }, 0);
        }

        function tryAutofill() {
            if (!bridge) return;
            const pwInputs = Array.from(document.querySelectorAll('input[type="password"]')).filter(isVisible);
            if (!pwInputs.length) return;

            bridge.getSavedCredentialsJson(location.href, function(rawJson) {
                let creds = [];
                try { creds = JSON.parse(rawJson || '[]'); } catch (e) { creds = []; }
                if (!creds.length) return;

                for (const pwInput of pwInputs) {
                    if (pwInput.dataset.adblockAutofilled) continue;
                    if (creds.length === 1 && !pwInput.value) {
                        pwInput.dataset.adblockAutofilled = '1';
                        fillPair(pwInput, creds[0].username, creds[0].password);
                    } else if (creds.length > 1) {
                        pwInput.dataset.adblockAutofilled = '1';
                        pwInput.addEventListener('focus', () => showMultiAccountPicker(pwInput, creds));
                        const userInput = findUsernameInput(pwInput);
                        if (userInput) {
                            userInput.addEventListener('focus', () => showMultiAccountPicker(userInput, creds));
                        }
                    }
                }
            });
        }

        function attachDomListeners() {
            // Remember username on step-1 of multi-step logins
            document.addEventListener('input', (ev) => {
                const t = ev.target;
                if (t && t.tagName === 'INPUT' && t.type !== 'password' && t.type !== 'hidden') {
                    if (t.type === 'email' || /user|mail|login/i.test((t.name || '') + ' ' + (t.id || '') + ' ' + (t.autocomplete || ''))) {
                        rememberUsername(t.value);
                    }
                }
            }, true);

            // Standard form submission
            document.addEventListener('submit', (ev) => {
                captureFromScope(ev.target);
            }, true);

            // SPA button click / Enter key submission
            document.addEventListener('click', (ev) => {
                const btn = ev.target && ev.target.closest ? ev.target.closest('button, input[type="submit"], [role="button"]') : null;
                if (!btn) return;
                const scope = btn.form || document;
                if (scope.querySelector('input[type="password"]')) {
                    captureFromScope(scope);
                }
            }, true);

            document.addEventListener('keydown', (ev) => {
                if (ev.key === 'Enter' && ev.target && ev.target.tagName === 'INPUT') {
                    const scope = ev.target.form || document;
                    if (scope.querySelector('input[type="password"]')) {
                        captureFromScope(scope);
                    }
                }
            }, true);

            const obs = new MutationObserver(() => tryAutofill());
            if (document.documentElement) {
                obs.observe(document.documentElement, { childList: true, subtree: true });
            }
            tryAutofill();
        }

        function initChannel() {
            if (typeof QWebChannel === 'undefined' || !window.qt || !window.qt.webChannelTransport) {
                return;
            }
            new QWebChannel(window.qt.webChannelTransport, function(channel) {
                bridge = channel.objects.pwBridge;
                if (document.readyState === 'loading') {
                    document.addEventListener('DOMContentLoaded', attachDomListeners, { once: true });
                } else {
                    attachDomListeners();
                }
            });
        }

        initChannel();
    })();
    """
    return qwebchannel_js + "\n" + manager_js
