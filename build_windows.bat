@echo off
setlocal enabledelayedexpansion

echo === AdBlock Tarayici Windows Build ===

where python >nul 2>nul
if errorlevel 1 (
    echo [HATA] Python bulunamadi. Lutfen Python 3.9+ yukleyin ve PATH'e ekleyin.
    exit /b 1
)

if not exist ".venv_win" (
    echo [1/4] Sanal ortam olusturuluyor...
    python -m venv .venv_win
)

call .venv_win\Scripts\activate.bat

echo [2/4] Bagimliliklar ve PyInstaller yukleniyor...
python -m pip install --upgrade pip
pip install -r requirements.txt pyinstaller

echo [3/4] Birim testleri calistiriliyor...
python -m unittest discover -s tests -v
if errorlevel 1 (
    echo [HATA] Testler basarisiz oldu, build durduruldu.
    exit /b 1
)

echo [4/4] Windows .exe paketi olusturuluyor...
pyinstaller ^
    --noconfirm ^
    --clean ^
    --windowed ^
    --onedir ^
    --name "AdBlockTarayici" ^
    --hidden-import PyQt6.QtWebEngineCore ^
    --hidden-import PyQt6.QtWebEngineWidgets ^
    --hidden-import PyQt6.QtWebChannel ^
    --hidden-import PyQt6.QtSvg ^
    main.py

if errorlevel 1 (
    echo [HATA] PyInstaller build basarisiz oldu.
    exit /b 1
)

echo.
echo [BASARILI] Windows build tamamlandi: dist\AdBlockTarayici\AdBlockTarayici.exe
endlocal
