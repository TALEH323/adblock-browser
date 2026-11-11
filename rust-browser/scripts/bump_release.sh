#!/usr/bin/env bash
set -euo pipefail

# Kullanım: bash rust-browser/scripts/bump_release.sh 0.2.0 "Yeni sürüm başlığı"
cd "$(dirname "$0")/.."

NEW_VER="${1:-}"
TITLE="${2:-Yeni Güncelleme}"
if [ -z "$NEW_VER" ]; then
  echo "Kullanım: $0 <yeni_surum> [baslik]"
  echo "Örnek:    $0 0.2.0 \"Performans ve arayüz iyileştirmeleri\""
  exit 1
fi
NEW_VER="${NEW_VER#v}"

python3 - "$NEW_VER" "$TITLE" <<'PY'
import datetime, json, re, subprocess, sys
from pathlib import Path

new_ver, title = sys.argv[1], sys.argv[2]
root = Path.cwd()

# 1. Cargo.toml version güncelle
cargo_path = root / "Cargo.toml"
cargo_txt = cargo_path.read_text(encoding="utf-8")
cargo_txt = re.sub(
    r'(?m)^version\s*=\s*"[^"]+"',
    f'version = "{new_ver}"',
    cargo_txt,
    count=1,
)
cargo_path.write_text(cargo_txt, encoding="utf-8")

# 2. Inno Setup fallback version güncelle
iss_path = root / "installer/windows/AdBlockBrowser.iss"
iss_txt = iss_path.read_text(encoding="utf-8-sig")
iss_txt = re.sub(
    r'#define MyAppVersion "[^"]+"',
    f'#define MyAppVersion "{new_ver}"',
    iss_txt,
    count=1,
)
iss_path.write_bytes(b"\xef\xbb\xbf" + iss_txt.encode("utf-8"))

# 3. Son tag'den bu yana git commit değişikliklerini topla
try:
    last_tag = subprocess.check_output(
        ["git", "describe", "--tags", "--abbrev=0"], stderr=subprocess.DEVNULL
    ).decode().strip()
    log_range = f"{last_tag}..HEAD"
except subprocess.CalledProcessError:
    log_range = "HEAD~10..HEAD"

try:
    raw_commits = subprocess.check_output(
        ["git", "log", log_range, "--no-merges", "--pretty=format:%s"],
        stderr=subprocess.DEVNULL,
    ).decode().splitlines()
except subprocess.CalledProcessError:
    raw_commits = []

changes = [c.strip() for c in raw_commits if c.strip() and not c.startswith("Release v")]
if not changes:
    changes = [f"v{new_ver} kararlı derleme ve hata düzeltmeleri."]

# 4. CHANGELOG.json başına ekle
cl_path = root / "CHANGELOG.json"
entries = json.loads(cl_path.read_text(encoding="utf-8")) if cl_path.exists() else []
entries = [e for e in entries if e.get("version") != new_ver]
entries.insert(
    0,
    {
        "version": new_ver,
        "date": datetime.date.today().isoformat(),
        "title": title,
        "tag": f"v{new_ver}",
        "changes": changes[:12],
    },
)
cl_path.write_text(json.dumps(entries, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"Updated Cargo.toml, AdBlockBrowser.iss, and CHANGELOG.json -> v{new_ver} ({len(changes)} change item(s)).")
PY

echo "=> Sonraki adım: değişiklikleri commit edip etiket (tag) gönderin:"
echo "   git add rust-browser/Cargo.toml rust-browser/CHANGELOG.json rust-browser/installer/windows/AdBlockBrowser.iss"
echo "   git commit -m \"Release v${NEW_VER}\""
echo "   git tag v${NEW_VER} && git push origin main v${NEW_VER}"
