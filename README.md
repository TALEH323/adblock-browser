# 🛡️ AdBlock Tarayıcı (Python + PyQt6 WebEngine)

Dahili reklam engelleme (AdBlock), kozmetik CSS gizleme, sekme yönetimi, yer imleri ve tarama geçmişi özelliklerine sahip masaüstü web tarayıcısı.

## Özellikler

- **Çift Katmanlı Reklam Engelleme:**
  - **Ağ Seviyesi Engelleme (`QWebEngineUrlRequestInterceptor`):** EasyList / Adblock Plus uyumlu kural motoru (`||domain^`, `@@||istisna^`, `$third-party`, `$script`, `$image`, `$xmlhttprequest`, `domain=`, hosts biçimi).
  - **Kozmetik Filtreleme (`##` / `#@#`):** Sayfa yüklenirken reklam kutularını ve boşluklarını CSS enjeksiyonu ile gizler; YouTube reklam katmanları ve atlama düğmeleri için yardımcı içerir.
- **AdBlock Kalkan Paneli (`🛡️` düğmesi):**
  - Aktif sekmede engellenen reklam sayısı ve toplam engellenen sayacı
  - Site bazlı aç/kapa (**Beyaz Liste**)
  - Engellenen isteklerin canlı listesi (URL, tür, eşleşen kural)
  - Özel EasyList kuralı düzenleyicisi
  - Tek tıkla internetten filtre listesi güncelleme (Peter Lowe & AdGuard Simplified Domain List)
- **Tarayıcı Özellikleri:**
  - Çoklu sekme (`+`, sürükle-bırak, kapatma, `target="_blank"` yeni sekmede açma)
  - Sekme sağ tık menüsü: Sekmeyi Çoğalt, Sekme Sesini Kapat/Aç (`🔇`/`🔊`), Diğerlerini Kapat
  - Kapatılan sekmeyi geri açma (`Cmd/Ctrl + Shift + T`)
  - **Gizli Mod (`Cmd/Ctrl + Shift + N`):** Diskte geçmiş, çerez, önbellek ve şifre izi bırakmayan izole oturum
  - Akıllı adres çubuğu: Yer imleri + geçmişten otomatik tamamlama ve HTTPS/HTTP güvenlik rozeti (`🔒` / `⚠️`)
  - Sayfada Bul (`Cmd/Ctrl + F`), İndirilenler Yöneticisi (`Cmd/Ctrl + J`), Şifreli Parola Yöneticisi (`🔑`)
  - PDF olarak dışa aktarma (`Cmd/Ctrl + P`), Sayfayı HTML/MHTML kaydetme (`Cmd/Ctrl + S`), Sayfa Kaynağı (`Cmd/Ctrl + U`) ve DevTools (`F12` / `Cmd/Ctrl + Shift + I`)

## Kurulum ve Çalıştırma

`PyQt6` ve `PyQt6-WebEngine` gereklidir:

```bash
pip3 install -r requirements.txt
python3 main.py
```

## Klavye Kısayolları (macOS'ta `Cmd`, diğer sistemlerde `Ctrl`)

- `Cmd/Ctrl + T`: Yeni sekme
- `Cmd/Ctrl + W`: Sekmeyi kapat
- `Cmd/Ctrl + L`: Adres çubuğuna odaklan
- `Cmd/Ctrl + R`: Sayfayı yenile
- `Cmd/Ctrl + D`: Yer imine ekle / çıkar
- `Cmd/Ctrl + H`: Tarama geçmişini aç
- `Cmd/Ctrl + Shift + B`: Yer imleri çubuğunu gizle / göster
- `Cmd/Ctrl + Shift + T`: Kapatılan son sekmeyi geri aç
- `Cmd/Ctrl + Shift + N`: Yeni Gizli Pencere aç
- `Cmd/Ctrl + F`: Sayfada bul
- `Cmd/Ctrl + J`: İndirilenler penceresini aç
- `Cmd/Ctrl + P`: Sayfayı PDF olarak kaydet
- `Cmd/Ctrl + S`: Sayfayı HTML/MHTML olarak kaydet
- `Cmd/Ctrl + U`: Sayfa kaynağını görüntüle
- `F12` / `Cmd/Ctrl + Shift + I`: Geliştirici Araçları (DevTools)
- `Cmd/Ctrl + [` / `]`: Geri / İleri
- `Cmd/Ctrl + =` / `-` / `0`: Yakınlaştır / Uzaklaştır / Sıfırla

## Testleri Çalıştırma

```bash
python3 -m unittest discover -s tests -v
```

## Windows İçin Build Alma (`.exe`)

PyInstaller çapraz derleme (cross-compile) yapamadığı için Windows `.exe` çıktısının Windows ortamında üretilmesi gerekir. Bunun için iki hazır seçenek eklendi:

1. **Windows Bilgisayarda Doğrudan Build:**
   Projeyi Windows bir bilgisayara kopyalayıp `build_windows.bat` dosyasını çalıştırın.
   Çıktı `dist\AdBlockTarayici\AdBlockTarayici.exe` olarak oluşturulur (ayarlar ve yer imleri `%LOCALAPPDATA%\AdBlockTarayici` altında saklanır).
2. **Mac'ten Bulut Üzerinde Build (GitHub Actions):**
   Projeyi bir GitHub deposuna yüklediğinizde [build-windows.yml](.github/workflows/build-windows.yml) iş akışı `windows-latest` üzerinde otomatik olarak `AdBlockTarayici-Windows-x64.zip` paketini derleyip indirilebilir artifact olarak üretir.
