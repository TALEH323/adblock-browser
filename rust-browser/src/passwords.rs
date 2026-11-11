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

/// Page script injected into websites to detect login form submission & autofill credentials.
pub const PASSWORD_INJECT_JS: &str = r#"
(() => {
  if (window.__adblockPwInit) return;
  window.__adblockPwInit = true;
  if (!location.protocol.startsWith('http')) return;

  function findCredentials(form) {
    const scope = form || document;
    const pw = scope.querySelector('input[type="password"]');
    if (!pw || !pw.value) return null;
    const userSel = [
      'input[autocomplete="username"]',
      'input[type="email"]',
      'input[name*="user" i]',
      'input[name*="mail" i]',
      'input[name*="login" i]',
      'input[type="text"]'
    ].join(',');
    let userEl = null;
    for (const el of scope.querySelectorAll(userSel)) {
      if (el !== pw && el.offsetParent !== null) { userEl = el; break; }
    }
    return { origin: location.hostname, username: userEl ? userEl.value : '', password: pw.value };
  }

  document.addEventListener('submit', (e) => {
    const creds = findCredentials(e.target);
    if (creds && creds.password && window.ipc) {
      window.ipc.postMessage(JSON.stringify({ type: 'pw_capture', ...creds }));
    }
  }, true);

  window.__adblockFillPassword = (username, password) => {
    const pw = document.querySelector('input[type="password"]');
    if (!pw) return;
    const userEl = document.querySelector('input[autocomplete="username"], input[type="email"], input[type="text"]');
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    if (userEl && username) {
      setter.call(userEl, username);
      userEl.dispatchEvent(new Event('input', { bubbles: true }));
    }
    setter.call(pw, password);
    pw.dispatchEvent(new Event('input', { bubbles: true }));
  };
})();
"#;

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
}
