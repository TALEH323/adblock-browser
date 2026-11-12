use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use chrono::Local;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedCredential {
    pub origin: String,
    pub username: String,
    pub password: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct VaultDisk {
    version: u8,
    salt: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct VaultPlain {
    credentials: Vec<SavedCredential>,
    never_save: Vec<String>,
}

pub struct PasswordManager {
    file_path: PathBuf,
    master_secret: Vec<u8>,
    pub credentials: Vec<SavedCredential>,
    pub never_save: Vec<String>,
    pub last_import_notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeImportReport {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpsertOutcome {
    Added,
    Updated,
    Skipped,
}

impl PasswordManager {
    pub fn load(data_dir: &PathBuf) -> Self {
        let _ = fs::create_dir_all(data_dir);
        let key_path = data_dir.join(".vault.key");
        let master_secret = Self::load_or_create_key(&key_path);
        let file_path = data_dir.join("passwords_vault.json");
        let mut mgr = Self {
            file_path,
            master_secret,
            credentials: Vec::new(),
            never_save: Vec::new(),
            last_import_notice: None,
        };
        mgr.reload();
        mgr
    }

    fn load_or_create_key(key_path: &PathBuf) -> Vec<u8> {
        if let Ok(raw) = fs::read_to_string(key_path) {
            if let Ok(bytes) = B64.decode(raw.trim()) {
                if bytes.len() == 32 {
                    return bytes;
                }
            }
        }
        let mut key = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        let _ = fs::write(key_path, B64.encode(&key));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(key_path, fs::Permissions::from_mode(0o600));
        }
        key
    }

    fn derive_key(&self, salt: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        pbkdf2_hmac::<Sha256>(&self.master_secret, salt, 100_000, &mut out);
        out
    }

    pub fn reload(&mut self) {
        let Ok(raw) = fs::read_to_string(&self.file_path) else {
            return;
        };
        let Ok(disk) = serde_json::from_str::<VaultDisk>(&raw) else {
            return;
        };
        let (Ok(salt), Ok(nonce_bytes), Ok(ct)) = (
            B64.decode(&disk.salt),
            B64.decode(&disk.nonce),
            B64.decode(&disk.ciphertext),
        ) else {
            return;
        };
        if nonce_bytes.len() != 12 {
            return;
        }
        let dk = self.derive_key(&salt);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dk));
        let nonce = Nonce::from_slice(&nonce_bytes);
        if let Ok(plain) = cipher.decrypt(nonce, ct.as_ref()) {
            if let Ok(parsed) = serde_json::from_slice::<VaultPlain>(&plain) {
                self.credentials = parsed.credentials;
                self.never_save = parsed.never_save;
            }
        }
    }

    pub fn save(&self) {
        let plain = VaultPlain {
            credentials: self.credentials.clone(),
            never_save: self.never_save.clone(),
        };
        let Ok(plain_bytes) = serde_json::to_vec(&plain) else {
            return;
        };
        let mut salt = [0u8; 16];
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut salt);
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let dk = self.derive_key(&salt);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dk));
        let Ok(ct) = cipher.encrypt(Nonce::from_slice(&nonce_bytes), plain_bytes.as_ref()) else {
            return;
        };
        let disk = VaultDisk {
            version: 1,
            salt: B64.encode(salt),
            nonce: B64.encode(nonce_bytes),
            ciphertext: B64.encode(ct),
        };
        if let Ok(json) = serde_json::to_string_pretty(&disk) {
            let _ = fs::write(&self.file_path, json);
        }
    }

    fn upsert_in_memory(
        &mut self,
        origin: &str,
        username: &str,
        password: &str,
        now: &str,
    ) -> UpsertOutcome {
        let orig = origin.trim().to_lowercase();
        let user = username.trim().to_string();
        if orig.is_empty() || password.is_empty() {
            return UpsertOutcome::Skipped;
        }
        let outcome = if let Some(existing) = self
            .credentials
            .iter_mut()
            .find(|c| c.origin == orig && c.username == user)
        {
            existing.password = password.to_string();
            existing.updated_at = now.to_string();
            UpsertOutcome::Updated
        } else {
            self.credentials.insert(
                0,
                SavedCredential {
                    origin: orig.clone(),
                    username: user,
                    password: password.to_string(),
                    updated_at: now.to_string(),
                },
            );
            UpsertOutcome::Added
        };
        self.never_save.retain(|d| d != &orig);
        outcome
    }

    pub fn add_or_update(&mut self, origin: &str, username: &str, password: &str) {
        let now = Local::now().format("%Y-%m-%d %H:%M").to_string();
        if self.upsert_in_memory(origin, username, password, &now) != UpsertOutcome::Skipped {
            self.save();
        }
    }

    /// Parse a Google Chrome Passwords CSV export (`name,url,username,password[,note]`)
    /// and batch-upsert entries into the encrypted vault with a single PBKDF2/AES save.
    pub fn import_chrome_csv(&mut self, csv_text: &str) -> ChromeImportReport {
        let rows = parse_csv_rows(csv_text);
        if rows.is_empty() {
            self.last_import_notice =
                Some("CSV dosyası boş veya okunamadı.".to_string());
            return ChromeImportReport {
                added: 0,
                updated: 0,
                skipped: 0,
            };
        }

        let headers: Vec<String> = rows[0]
            .iter()
            .map(|h| h.trim().to_lowercase())
            .collect();

        let url_col = find_col(&headers, &["url", "origin", "website", "login_uri"]);
        let name_col = find_col(&headers, &["name", "title"]);
        let user_col = find_col(&headers, &["username", "user", "login_username", "email"]);
        let pass_col = find_col(&headers, &["password", "login_password"]);

        let (Some(pass_i), Some(user_i)) = (pass_col, user_col) else {
            self.last_import_notice = Some(
                "Geçersiz Chrome CSV biçimi: 'url, username, password' sütunları bulunamadı."
                    .to_string(),
            );
            return ChromeImportReport {
                added: 0,
                updated: 0,
                skipped: 0,
            };
        };

        let mut report = ChromeImportReport {
            added: 0,
            updated: 0,
            skipped: 0,
        };
        let now = Local::now().format("%Y-%m-%d %H:%M").to_string();

        for row in rows.iter().skip(1) {
            if row.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            let raw_url = url_col.and_then(|i| row.get(i)).map(String::as_str).unwrap_or("");
            let raw_name = name_col.and_then(|i| row.get(i)).map(String::as_str).unwrap_or("");
            let username = row.get(user_i).map(String::as_str).unwrap_or("");
            let password = row.get(pass_i).map(String::as_str).unwrap_or("");

            let origin = normalize_chrome_origin(raw_url, raw_name);
            match self.upsert_in_memory(&origin, username, password, &now) {
                UpsertOutcome::Added => report.added += 1,
                UpsertOutcome::Updated => report.updated += 1,
                UpsertOutcome::Skipped => report.skipped += 1,
            }
        }

        if report.added > 0 || report.updated > 0 {
            self.save();
        }
        self.last_import_notice = Some(format!(
            "Chrome içe aktarma tamamlandı: {} yeni şifre eklendi, {} şifre güncellendi{}.",
            report.added,
            report.updated,
            if report.skipped > 0 {
                format!(" ({} satır atlandı)", report.skipped)
            } else {
                String::new()
            }
        ));
        report
    }

    pub fn delete_at(&mut self, idx: usize) {
        if idx < self.credentials.len() {
            self.credentials.remove(idx);
            self.save();
        }
    }

    pub fn mark_never_save(&mut self, origin: &str) {
        let orig = origin.trim().to_lowercase();
        if !orig.is_empty() && !self.never_save.contains(&orig) {
            self.never_save.push(orig);
            self.save();
        }
    }

    pub fn is_never_save(&self, origin: &str) -> bool {
        let orig = origin.trim().to_lowercase();
        self.never_save.iter().any(|x| x == &orig)
    }

    pub fn for_origin(&self, origin: &str) -> Vec<&SavedCredential> {
        let orig = origin.trim().to_lowercase();
        let orig_bare = orig.strip_prefix("www.").unwrap_or(&orig);
        self.credentials
            .iter()
            .filter(|c| {
                let c_bare = c.origin.strip_prefix("www.").unwrap_or(&c.origin);
                c.origin == orig || (!orig_bare.is_empty() && c_bare == orig_bare)
            })
            .collect()
    }

    pub fn is_already_saved(&self, origin: &str, username: &str, password: &str) -> bool {
        if password.is_empty() {
            return false;
        }
        let uname = username.trim();
        self.for_origin(origin).iter().any(|c| {
            c.password == password && (uname.is_empty() || c.username.eq_ignore_ascii_case(uname))
        })
    }
}

fn find_col(headers: &[String], candidates: &[&str]) -> Option<usize> {
    candidates
        .iter()
        .find_map(|c| headers.iter().position(|h| h == c))
}

fn normalize_chrome_origin(raw_url: &str, raw_name: &str) -> String {
    let trimmed = raw_url.trim();
    if let Some(rest) = trimmed.strip_prefix("android://") {
        // Format: android://<cert_hash>@<package_id>[/]
        if let Some((_, pkg)) = rest.split_once('@') {
            let pkg_clean = pkg.trim_matches('/').trim().to_lowercase();
            if !pkg_clean.is_empty() {
                return pkg_clean;
            }
        }
    }
    if !trimmed.is_empty() {
        let candidate = if trimmed.contains("://") {
            trimmed.to_string()
        } else {
            format!("https://{trimmed}")
        };
        if let Ok(parsed) = url::Url::parse(&candidate) {
            if let Some(host) = parsed.host_str() {
                let h = host.trim().to_lowercase();
                if !h.is_empty() {
                    return h;
                }
            }
        }
    }
    raw_name.trim().to_lowercase()
}

/// Minimal RFC 4180 CSV parser supporting UTF-8 BOM, quoted fields,
/// escaped quotes (`""`), and embedded newlines.
fn parse_csv_rows(input: &str) -> Vec<Vec<String>> {
    let clean = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = clean.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else {
            match ch {
                '"' if field.is_empty() => {
                    in_quotes = true;
                }
                ',' => {
                    current_row.push(std::mem::take(&mut field));
                }
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    current_row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut current_row));
                }
                '\n' => {
                    current_row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut current_row));
                }
                other => field.push(other),
            }
        }
    }
    if !field.is_empty() || !current_row.is_empty() {
        current_row.push(field);
        rows.push(current_row);
    }
    rows
}

/// Page script injected into websites to detect login form submission, show inline account suggestions & autofill credentials.
pub const PASSWORD_INJECT_JS: &str = r##"
(() => {
  if (window.__adblockPwInit) return;
  window.__adblockPwInit = true;
  if (!location.protocol.startsWith('http')) return;

  const IS_TOP = window.top === window.self;
  const USER_SELECTORS = [
    'input[autocomplete="username"]',
    'input[autocomplete="email"]',
    'input[type="email"]',
    'input[name*="user" i]',
    'input[name*="mail" i]',
    'input[name*="login" i]',
    'input[id*="user" i]',
    'input[id*="mail" i]',
    'input[type="text"]',
    'input[type="tel"]'
  ].join(',');

  function isVisible(el) {
    if (!el || !el.getBoundingClientRect) return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && el.offsetParent !== null;
  }

  function inputMeta(el) {
    return (
      (el.name || '') + ' ' +
      (el.id || '') + ' ' +
      (el.placeholder || '') + ' ' +
      (el.getAttribute('aria-label') || '') + ' ' +
      (el.getAttribute('autocomplete') || '')
    ).toLowerCase();
  }

  function isSearchBox(el) {
    if (!el || el.type === 'password') return false;
    const ac = (el.getAttribute('autocomplete') || '').toLowerCase();
    if (/username|current-password|new-password|email/.test(ac)) return false;
    if (el.type === 'search' || el.getAttribute('role') === 'searchbox') return true;
    const meta = inputMeta(el);
    return /(?:^|\s)(q|s|query|search|search_query|keyword|keywords|filter|ara)(?:\s|$)/.test(meta) ||
      /\b(search|ara\b|filter)\b/.test(meta);
  }

  function isEligibleLoginInput(el) {
    if (!el || el.tagName !== 'INPUT' || el.disabled || el.readOnly) return false;
    const t = (el.type || 'text').toLowerCase();
    if (/^(hidden|checkbox|radio|submit|button|reset|file|image|range|color|date|datetime-local|month|time|week|search)$/.test(t)) {
      return false;
    }
    if (t === 'password' || t === 'email') return !isSearchBox(el);
    const ac = (el.getAttribute('autocomplete') || '').toLowerCase();
    if (/username|current-password|new-password|email/.test(ac)) return true;
    if (isSearchBox(el)) return false;
    const meta = inputMeta(el);
    if (/(otp|one-time|two-factor|2fa|captcha|dogrulama|pin\b)/.test(meta)) return false;
    const scope = el.form || (el.closest && el.closest('[role="dialog"], main, section, form'));
    if (scope && scope.querySelector('input[type="password"]')) return true;
    return /(user|mail|login|account|auth|phone|tel|telefon|nick|handle|kullanici|eposta|e-posta|hesap|giris|oturum|identifier|session|uye|member|kimlik)/.test(meta);
  }

  function findPasswordInput(scope, preferred) {
    if (preferred && preferred.type === 'password' && document.contains(preferred)) return preferred;
    const list = Array.from((scope || document).querySelectorAll('input[type="password"]'));
    return list.find(isVisible) || list[0] || null;
  }

  function findUserInput(scope, pw, preferred) {
    if (preferred && preferred !== pw && preferred.type !== 'password' && document.contains(preferred) && !isSearchBox(preferred)) {
      return preferred;
    }
    const candidates = Array.from((scope || document).querySelectorAll(USER_SELECTORS))
      .filter((el) => el !== pw && !isSearchBox(el) && isVisible(el));
    const nonEmpty = candidates.find((el) => (el.value || '').trim().length > 0);
    return nonEmpty || candidates[0] || null;
  }

  function findCredentials(scope) {
    const root = scope && scope.querySelectorAll ? scope : document;
    const pws = Array.from(root.querySelectorAll('input[type="password"]'));
    const pw = pws.find((el) => el.value && isVisible(el)) || pws.find((el) => !!el.value);
    if (!pw || !pw.value) return null;
    const userEl = findUserInput(root, pw, null) || (root !== document ? findUserInput(document, pw, null) : null);
    return {
      origin: location.hostname,
      username: userEl ? userEl.value : '',
      password: pw.value
    };
  }

  let lastCaptureSig = '';
  let lastCaptureAt = 0;
  function reportCredentials(scope) {
    const creds = findCredentials(scope);
    if (!creds || !creds.password || !window.ipc) return;
    const sig = creds.origin + '\0' + creds.username + '\0' + creds.password;
    const now = Date.now();
    if (sig === lastCaptureSig && now - lastCaptureAt < 2000) return;
    lastCaptureSig = sig;
    lastCaptureAt = now;
    window.ipc.postMessage(JSON.stringify({ type: 'pw_capture', ...creds }));
  }

  document.addEventListener('submit', (e) => {
    reportCredentials(e.target);
  }, true);

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && e.target && e.target.tagName === 'INPUT' && e.target.type === 'password') {
      const scope = e.target.form || e.target.closest('[role="dialog"], main, section') || document;
      reportCredentials(scope);
    }
  }, true);

  document.addEventListener('click', (e) => {
    const btn = e.target && e.target.closest ? e.target.closest('button, input[type="submit"], [role="button"]') : null;
    if (!btn) return;
    const txt = ((btn.innerText || btn.value || '') + ' ' + (btn.getAttribute('aria-label') || '')).toLowerCase();
    const looksSubmit = btn.type === 'submit' ||
      /(giriş|giris|sign\s*in|log\s*in|devam|continue|submit|next|ileri|oturum)/.test(txt);
    if (!looksSubmit) return;
    const scope = btn.form || btn.closest('[role="dialog"], main, section') || document;
    reportCredentials(scope);
  }, true);

  let activeInput = null;
  let suppressOpenUntil = 0;

  // --- Inline Autofill Suggestions (Top Frame Only) ---
  let hints = [];
  let hintsHost = '';
  let lastQueryAt = 0;
  let visibleItems = [];
  let selectedIdx = -1;
  let hostEl = null;
  let shadowRoot = null;
  let cardEl = null;
  let listEl = null;

  function ensureUi() {
    if (hostEl && document.contains(hostEl)) return true;
    if (!document.documentElement) return false;
    hostEl = document.createElement('div');
    hostEl.setAttribute('data-ab-pw-autofill', '1');
    hostEl.style.cssText = 'position:fixed;top:0;left:0;width:0;height:0;z-index:2147483647;pointer-events:none;';
    shadowRoot = hostEl.attachShadow({ mode: 'closed' });
    const style = document.createElement('style');
    style.textContent = `
      :host { all: initial; }
      .card {
        position: fixed;
        display: none;
        pointer-events: auto;
        box-sizing: border-box;
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
        background: #ffffff;
        color: #0f172a;
        border: 1px solid rgba(15, 23, 42, 0.16);
        border-radius: 10px;
        box-shadow: 0 12px 30px rgba(0, 0, 0, 0.22);
        overflow: hidden;
        user-select: none;
      }
      @media (prefers-color-scheme: dark) {
        .card {
          background: #1f2430;
          color: #eef2ff;
          border-color: rgba(255, 255, 255, 0.14);
          box-shadow: 0 14px 34px rgba(0, 0, 0, 0.55);
        }
      }
      .hdr {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 7px 11px 5px;
        font-size: 11px;
        font-weight: 600;
        letter-spacing: 0.02em;
        opacity: 0.72;
        border-bottom: 1px solid rgba(128, 128, 128, 0.16);
      }
      .list {
        max-height: 224px;
        overflow-y: auto;
        padding: 4px;
      }
      .item {
        display: flex;
        align-items: center;
        gap: 9px;
        padding: 7px 9px;
        border-radius: 7px;
        cursor: pointer;
      }
      .item:hover, .item.sel {
        background: rgba(59, 130, 246, 0.16);
      }
      .ico {
        width: 26px;
        height: 26px;
        border-radius: 999px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(59, 130, 246, 0.16);
        color: #3b82f6;
        flex-shrink: 0;
      }
      .meta {
        min-width: 0;
        flex: 1;
      }
      .usr {
        font-size: 13px;
        font-weight: 600;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
      }
      .sub {
        font-size: 11px;
        opacity: 0.68;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        margin-top: 1px;
      }
    `;
    cardEl = document.createElement('div');
    cardEl.className = 'card';
    const hdr = document.createElement('div');
    hdr.className = 'hdr';
    hdr.innerHTML = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><circle cx="7.5" cy="15.5" r="4.5"/><path d="m21 2-9.6 9.6"/><path d="m15.5 7.5 3 3L22 7l-3-3"/></svg><span>Kayıtlı Hesaplar</span>';
    listEl = document.createElement('div');
    listEl.className = 'list';
    cardEl.appendChild(hdr);
    cardEl.appendChild(listEl);
    shadowRoot.appendChild(style);
    shadowRoot.appendChild(cardEl);
    document.documentElement.appendChild(hostEl);
    return true;
  }

  function hideDropdown() {
    selectedIdx = -1;
    visibleItems = [];
    if (cardEl) cardEl.style.display = 'none';
  }

  function selectHint(item) {
    if (!item || !window.ipc) return;
    suppressOpenUntil = Date.now() + 500;
    hideDropdown();
    window.ipc.postMessage(JSON.stringify({
      type: 'ui_cmd',
      cmd: 'pw_fill_inline',
      origin: item.origin || '',
      username: item.username || ''
    }));
  }

  function syncSelectionStyles() {
    if (!listEl) return;
    const rows = listEl.querySelectorAll('.item');
    rows.forEach((row, idx) => {
      if (idx === selectedIdx) {
        row.classList.add('sel');
        row.scrollIntoView({ block: 'nearest' });
      } else {
        row.classList.remove('sel');
      }
    });
  }

  function filterHintsFor(el) {
    if (!hints.length) return [];
    if (!el || el.type === 'password') return hints;
    const q = (el.value || '').trim().toLowerCase();
    if (!q || hints.some((h) => (h.username || '').toLowerCase() === q)) return hints;
    return hints.filter((h) => (h.username || '').toLowerCase().includes(q));
  }

  function positionDropdown() {
    if (!cardEl || !activeInput || !document.contains(activeInput)) return;
    const r = activeInput.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0 || r.bottom < 0 || r.top > window.innerHeight) {
      hideDropdown();
      return;
    }
    const width = Math.min(Math.max(r.width, 250), Math.min(360, window.innerWidth - 16));
    const left = Math.max(8, Math.min(r.left, window.innerWidth - width - 8));
    cardEl.style.width = width + 'px';
    cardEl.style.left = left + 'px';
    cardEl.style.top = Math.round(r.bottom + 4) + 'px';
    const ch = cardEl.offsetHeight || 160;
    if (r.bottom + 4 + ch > window.innerHeight - 8 && r.top - 4 - ch >= 8) {
      cardEl.style.top = Math.round(r.top - 4 - ch) + 'px';
    }
  }

  function showDropdown() {
    if (!IS_TOP) return;
    if (Date.now() < suppressOpenUntil) return;
    if (!activeInput || !document.contains(activeInput) || document.activeElement !== activeInput) {
      hideDropdown();
      return;
    }
    const items = filterHintsFor(activeInput);
    if (!items.length || !ensureUi()) {
      hideDropdown();
      return;
    }
    visibleItems = items;
    if (selectedIdx >= visibleItems.length) selectedIdx = -1;
    listEl.textContent = '';
    items.forEach((item, idx) => {
      const row = document.createElement('div');
      row.className = 'item' + (idx === selectedIdx ? ' sel' : '');

      const ico = document.createElement('div');
      ico.className = 'ico';
      ico.innerHTML = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>';

      const meta = document.createElement('div');
      meta.className = 'meta';

      const usr = document.createElement('div');
      usr.className = 'usr';
      usr.textContent = item.username || '(Kullanıcı adı yok)';

      const sub = document.createElement('div');
      sub.className = 'sub';
      sub.textContent = '••••••••  ·  ' + (item.origin || location.hostname);

      meta.appendChild(usr);
      meta.appendChild(sub);
      row.appendChild(ico);
      row.appendChild(meta);

      row.addEventListener('mouseenter', () => {
        selectedIdx = idx;
        syncSelectionStyles();
      });
      row.addEventListener('mousedown', (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        selectHint(item);
      });
      listEl.appendChild(row);
    });
    cardEl.style.display = 'block';
    positionDropdown();
  }

  function requestHintsIfNeeded() {
    if (!IS_TOP || !window.ipc) return;
    const now = Date.now();
    if (hintsHost !== location.hostname || now - lastQueryAt > 1500) {
      lastQueryAt = now;
      window.ipc.postMessage(JSON.stringify({ type: 'ui_cmd', cmd: 'pw_query_hints' }));
    }
  }

  if (IS_TOP) {
    window.__adblockSetPwHints = (list) => {
      hints = Array.isArray(list) ? list : [];
      hintsHost = location.hostname;
      if (activeInput && document.activeElement === activeInput) {
        showDropdown();
      }
    };

    document.addEventListener('focusin', (e) => {
      if (isEligibleLoginInput(e.target)) {
        activeInput = e.target;
        selectedIdx = -1;
        requestHintsIfNeeded();
        showDropdown();
      } else {
        hideDropdown();
      }
    }, true);

    document.addEventListener('click', (e) => {
      if (isEligibleLoginInput(e.target)) {
        activeInput = e.target;
        requestHintsIfNeeded();
        showDropdown();
      } else if (e.target !== hostEl) {
        hideDropdown();
      }
    }, true);

    document.addEventListener('input', (e) => {
      if (e.target === activeInput) {
        selectedIdx = -1;
        showDropdown();
      }
    }, true);

    document.addEventListener('focusout', () => {
      setTimeout(() => {
        if (!activeInput || document.activeElement !== activeInput) {
          hideDropdown();
        }
      }, 90);
    }, true);

    document.addEventListener('keydown', (e) => {
      if (!cardEl || cardEl.style.display === 'none' || !visibleItems.length) return;
      if (e.key === 'Escape') {
        e.preventDefault();
        hideDropdown();
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        selectedIdx = (selectedIdx + 1) % visibleItems.length;
        syncSelectionStyles();
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        selectedIdx = selectedIdx <= 0 ? visibleItems.length - 1 : selectedIdx - 1;
        syncSelectionStyles();
      } else if (e.key === 'Enter' && selectedIdx >= 0 && visibleItems[selectedIdx]) {
        e.preventDefault();
        e.stopPropagation();
        selectHint(visibleItems[selectedIdx]);
      }
    }, true);

    window.addEventListener('scroll', () => {
      if (cardEl && cardEl.style.display !== 'none') positionDropdown();
    }, true);
    window.addEventListener('resize', () => {
      if (cardEl && cardEl.style.display !== 'none') positionDropdown();
    }, true);
  }

  window.__adblockFillPassword = (username, password) => {
    suppressOpenUntil = Date.now() + 500;
    hideDropdown();
    const anchor = activeInput && document.contains(activeInput) ? activeInput : null;
    const anchorForm = anchor && anchor.form ? anchor.form : null;
    const pw = findPasswordInput(anchorForm || document, anchor);
    const scope = (pw && pw.form) || anchorForm || document;
    const userEl = findUserInput(scope, pw, anchor) ||
      (scope !== document ? findUserInput(document, pw, anchor) : null);
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    if (userEl && username) {
      setter.call(userEl, username);
      userEl.dispatchEvent(new Event('input', { bubbles: true }));
      userEl.dispatchEvent(new Event('change', { bubbles: true }));
    }
    if (pw && password) {
      setter.call(pw, password);
      pw.dispatchEvent(new Event('input', { bubbles: true }));
      pw.dispatchEvent(new Event('change', { bubbles: true }));
    }
  };
})();
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_vault_roundtrip_and_never_save() {
        let tmp = std::env::temp_dir().join(format!("ab_test_pw_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        {
            let mut pm = PasswordManager::load(&tmp);
            pm.add_or_update("github.com", "alice", "s3cr3t!");
            pm.add_or_update("github.com", "alice", "new-pass!");
            pm.mark_never_save("bank.local");
            assert_eq!(pm.for_origin("github.com").len(), 1);
            assert_eq!(pm.for_origin("github.com")[0].password, "new-pass!");
        }
        // Raw file must not store plaintext password
        let raw = fs::read_to_string(tmp.join("passwords_vault.json")).unwrap();
        assert!(!raw.contains("new-pass!"));
        assert!(!raw.contains("github.com"));

        let mut reloaded = PasswordManager::load(&tmp);
        assert_eq!(reloaded.credentials.len(), 1);
        assert_eq!(reloaded.credentials[0].password, "new-pass!");
        assert!(reloaded.is_never_save("bank.local"));
        reloaded.delete_at(0);
        assert!(reloaded.credentials.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn imports_chrome_passwords_csv_with_quotes_bom_and_dedup() {
        let tmp = std::env::temp_dir().join(format!("ab_test_csv_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut pm = PasswordManager::load(&tmp);
        pm.add_or_update("github.com", "alice", "oldpass");

        let csv = "\u{feff}name,url,username,password,note\r\n\
                   GitHub,https://github.com/login,alice,\"p@ss,with\"\"quotes\",line1\n\
                   Amazon,https://www.amazon.com/ap/signin,bob@example.com,Hunter2!,\"multi\nline note\"\n\
                   EmptyPw,https://skip.me,user,,\n\
                   Android,android://hash123@com.netflix.mediaclient/,carl,Stream99,\n";

        let rep = pm.import_chrome_csv(csv);
        assert_eq!(rep.added, 2);
        assert_eq!(rep.updated, 1);
        assert_eq!(rep.skipped, 1);

        assert_eq!(pm.for_origin("github.com")[0].password, "p@ss,with\"quotes");
        // www.amazon.com matches bare amazon.com
        assert_eq!(pm.for_origin("amazon.com").len(), 1);
        assert_eq!(pm.for_origin("amazon.com")[0].username, "bob@example.com");
        assert_eq!(pm.for_origin("com.netflix.mediaclient")[0].password, "Stream99");

        // Persisted encrypted to disk
        let reloaded = PasswordManager::load(&tmp);
        assert_eq!(reloaded.credentials.len(), 3);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn inject_script_supports_inline_hints_and_two_step_fill() {
        assert!(PASSWORD_INJECT_JS.contains("__adblockSetPwHints"));
        assert!(PASSWORD_INJECT_JS.contains("pw_query_hints"));
        assert!(PASSWORD_INJECT_JS.contains("pw_fill_inline"));
        assert!(PASSWORD_INJECT_JS.contains("attachShadow({ mode: 'closed' })"));
        assert!(PASSWORD_INJECT_JS.contains("__adblockFillPassword"));
        assert!(PASSWORD_INJECT_JS.contains("dispatchEvent(new Event('change'"));
    }

    #[test]
    fn suppresses_save_prompt_when_credential_already_saved() {
        let tmp = std::env::temp_dir().join(format!("ab_test_pw_dup_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut pm = PasswordManager::load(&tmp);
        pm.add_or_update("github.com", "Ali@Example.com", "secret-123");

        assert!(pm.is_already_saved("www.github.com", "ali@example.com", "secret-123"));
        assert!(pm.is_already_saved("github.com", "", "secret-123"));
        assert!(!pm.is_already_saved("github.com", "ali@example.com", "new-pass-456"));
        assert!(!pm.is_already_saved("github.com", "other@example.com", "secret-123"));
        let _ = fs::remove_dir_all(&tmp);
    }
}
