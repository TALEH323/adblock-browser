# AdBlock Tarayıcı (Rust — macOS & Windows)

Mevcut Python/PyQt6 tarayıcısının **Rust (`tao` + `wry` + `adblock-rust`)** ile yeniden yazılmış çapraz platform sürümüdür.

## Özellikler
- **Yerel Güvenlik Proxy'si (`127.0.0.1`)**: Brave'in `adblock-rust` motoru + Cloudflare DoH (`1.1.1.2`) DNS Koruması ile alan adı seviyesinde reklam ve zararlı domain engelleme.
- **Kozmetik & YouTube Filtreleme**: Sayfa içi `fetch`/`XHR` reklam engelleme, YouTube JSON reklam temizleyici, otomatik reklam atlama ve CSS gizleme.
- **HTTPS-First Modu**: Şifresiz `http://` bağlantılarını otomatik `https://`'e yükseltir, güvensiz bağlantılar için site bazlı istisna yönetir.
- **Şifreli Parola Kasası**: PBKDF2-HMAC-SHA256 + AES-256-GCM ile şifrelenmiş şifre saklama, form algılama ve tek tıkla doldurma.
- **Chrome Tarzı Koyu Arayüz**: Sekme çubuğu, SVG ikonlu araç çubuğu, yer imleri çubuğu, sağdan açılan Geçmiş/Yer İmleri yan paneli, açılır baloncuklar (AdBlock Kalkanı, Site Güvenliği, İndirilenler, Şifreler, Menü) ve `adblock://` dahili yönetim sekmeleri.

## Çalıştırma ve Derleme

### macOS
```bash
cd rust-browser
cargo test
cargo run --release
# Uygulama paketi (.app) oluşturmak için:
bash scripts/build_macos.sh
```

### Windows (Windows 10/11 + Edge WebView2)
```bat
cd rust-browser
cargo test
scripts\build_windows.bat
```
Çıktı: `rust-browser\dist\windows\AdBlockBrowser\AdBlockBrowser.exe`
