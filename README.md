# AdBlock Tarayıcı (Rust — macOS & Windows)

Brave `adblock-rust` motoru, yerel güvenlik proxy'si, Cloudflare DoH DNS koruması ve Chrome Material You koyu arayüzü ile **tamamen Rust (`tao` + `wry`)** dilinde geliştirilmiş hafif masaüstü web tarayıcısı.

## Özellikler

- **Çift Katmanlı Reklam & İzleyici Engelleme:**
  - Yerel HTTPS/HTTP proxy (`127.0.0.1`) + `adblock-rust` motoru + Cloudflare DoH (`1.1.1.2`) DNS Koruması.
  - Sayfa içi kozmetik CSS gizleme, YouTube reklam temizleyici ve tek tıkla site beyaz listesi (`AdBlock Kalkanı`).
- **AES-256-GCM Şifre Kasası & Google Chrome İçe Aktarma:**
  - PBKDF2-HMAC-SHA256 + AES-256-GCM şifreli yerel parola kasası (`passwords_vault.json`).
  - `adblock://passwords` sayfasından **Google Chrome (`chrome://password-manager/settings` → Dışa Aktar `.csv`)** şifrelerini sürükle-bırak veya dosya seçimi ile tek seferde içe aktarma.
  - Login formlarını otomatik algılama ve tek tıkla doldurma.
- **Chrome Tarzı Koyu Arayüz ve Dahili Sayfalar:**
  - Sekme çubuğu, SVG araç çubuğu, yer imleri çubuğu, sağdan açılan Geçmiş/Yer İmleri yan paneli.
  - Açılır baloncuklar (AdBlock Kalkanı, Site Güvenliği, İndirilenler, Şifreler, Menü).
  - `adblock://home`, `adblock://settings`, `adblock://history`, `adblock://bookmarks`, `adblock://downloads`, `adblock://passwords`, `adblock://filters`.

## Yerel Derleme ve Test

```bash
cd rust-browser
cargo test
cargo run --release
```

### macOS `.app` ve `.dmg` Installer
```bash
cd rust-browser
bash scripts/build_macos.sh
bash scripts/build_dmg.sh
```
Çıktı: `rust-browser/dist/AdBlockBrowser-0.1.0-macOS.dmg`

### Windows `.exe` ve Setup Installer (Inno Setup 6)
Windows bilgisayarda veya GitHub Actions üzerinden:
1. `cd rust-browser && scripts\build_windows.bat`
2. Inno Setup 6 (`ISCC.exe`) ile `rust-browser\installer\windows\AdBlockBrowser.iss` dosyasını derleyin.

Çıktı: `rust-browser/dist/installer/AdBlockBrowser-Setup-0.1.0-x64.exe`
