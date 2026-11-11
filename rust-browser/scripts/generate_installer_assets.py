#!/usr/bin/env python3
"""Generate sleek installer artwork for macOS DMG and Windows Inno Setup."""

from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ASSETS_DIR = Path(__file__).resolve().parent.parent / "assets"
INSTALLER_DIR = ASSETS_DIR / "installer"
ICON_PATH = ASSETS_DIR / "icon.png"


def load_font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    candidates = (
        [
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/HelveticaNeue.ttc",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        ]
        if bold
        else [
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/HelveticaNeue.ttc",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ]
    )
    for path in candidates:
        p = Path(path)
        if p.exists():
            try:
                index = 1 if (bold and path.endswith(".ttc")) else 0
                return ImageFont.truetype(str(p), size=size, index=index)
            except Exception:
                try:
                    return ImageFont.truetype(str(p), size=size)
                except Exception:
                    pass
    return ImageFont.load_default()


def draw_vertical_gradient(w: int, h: int, top_rgb, bottom_rgb) -> Image.Image:
    base = Image.new("RGBA", (w, h))
    px = base.load()
    for y in range(h):
        t = y / max(1, h - 1)
        r = int(top_rgb[0] + (bottom_rgb[0] - top_rgb[0]) * t)
        g = int(top_rgb[1] + (bottom_rgb[1] - top_rgb[1]) * t)
        b = int(top_rgb[2] + (bottom_rgb[2] - top_rgb[2]) * t)
        for x in range(w):
            px[x, y] = (r, g, b, 255)
    return base


def add_radial_glow(canvas: Image.Image, cx: int, cy: int, radius: int, color_rgba) -> Image.Image:
    glow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(glow)
    d.ellipse((cx - radius, cy - radius, cx + radius, cy + radius), fill=color_rgba)
    glow = glow.filter(ImageFilter.GaussianBlur(radius=max(8, radius // 2)))
    return Image.alpha_composite(canvas, glow)


def build_dmg_background(scale: int) -> Image.Image:
    w, h = 660 * scale, 420 * scale
    img = draw_vertical_gradient(w, h, (16, 21, 31), (10, 13, 20))
    img = add_radial_glow(img, 180 * scale, 240 * scale, 150 * scale, (70, 165, 255, 55))
    img = add_radial_glow(img, 480 * scale, 240 * scale, 150 * scale, (78, 224, 164, 48))

    draw = ImageDraw.Draw(img)

    # Top title & subtitle
    title_font = load_font(24 * scale, bold=True)
    sub_font = load_font(13 * scale, bold=False)
    badge_font = load_font(11 * scale, bold=False)

    title = "AdBlock Tarayıcı"
    tb = draw.textbbox((0, 0), title, font=title_font)
    draw.text(((w - (tb[2] - tb[0])) // 2, 34 * scale), title, font=title_font, fill=(242, 246, 252, 255))

    sub = "Kurmak için uygulamayı Applications klasörüne sürükleyin"
    sb = draw.textbbox((0, 0), sub, font=sub_font)
    draw.text(((w - (sb[2] - sb[0])) // 2, 68 * scale), sub, font=sub_font, fill=(154, 168, 189, 255))

    # Translucent glass layer (must use alpha_composite; ImageDraw replaces pixels directly)
    glass = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glass)

    # Drop zone glass cards around icon centers (175, 235) and (485, 235)
    card_w, card_h = 184 * scale, 184 * scale
    card_y = 143 * scale
    for cx in (175 * scale, 485 * scale):
        x0 = cx - card_w // 2
        y0 = card_y
        gd.rounded_rectangle(
            (x0, y0, x0 + card_w, y0 + card_h),
            radius=24 * scale,
            fill=(255, 255, 255, 12),
            outline=(132, 184, 255, 48),
            width=max(1, scale),
        )

    # Sleek center arrow pill between the two cards (x: 296 -> 362, y: 235)
    ay = 235 * scale
    ax0, ax1 = 296 * scale, 362 * scale
    gd.rounded_rectangle(
        (ax0 - 10 * scale, ay - 20 * scale, ax1 + 10 * scale, ay + 20 * scale),
        radius=20 * scale,
        fill=(78, 224, 164, 28),
        outline=(95, 200, 235, 85),
        width=max(1, scale),
    )

    img = Image.alpha_composite(img, glass)
    draw = ImageDraw.Draw(img)

    # Arrow shaft & head
    stroke = max(2, 3 * scale)
    draw.line((ax0 + 8 * scale, ay, ax1 - 10 * scale, ay), fill=(120, 226, 210, 240), width=stroke)
    head = 10 * scale
    draw.line((ax1 - 18 * scale, ay - head, ax1 - 8 * scale, ay), fill=(120, 226, 210, 255), width=stroke)
    draw.line((ax1 - 18 * scale, ay + head, ax1 - 8 * scale, ay), fill=(120, 226, 210, 255), width=stroke)

    # Bottom footer
    footer = "v0.1.0  •  AdBlock Engine  •  DoH DNS Guard  •  AES-256 Şifre Kasası"
    fb = draw.textbbox((0, 0), footer, font=badge_font)
    draw.text(((w - (fb[2] - fb[0])) // 2, 376 * scale), footer, font=badge_font, fill=(108, 122, 145, 255))

    return img


def build_windows_sidebar(scale: int) -> Image.Image:
    # Inno Setup standard: 164x314 (100%), 328x628 (200%)
    w, h = 164 * scale, 314 * scale
    img = draw_vertical_gradient(w, h, (15, 20, 30), (9, 12, 18))
    img = add_radial_glow(img, w // 2, 92 * scale, 68 * scale, (74, 174, 255, 72))
    img = add_radial_glow(img, w // 2, 112 * scale, 62 * scale, (78, 224, 164, 52))

    icon = Image.open(ICON_PATH).convert("RGBA")
    icon_sz = 92 * scale
    icon_resized = icon.resize((icon_sz, icon_sz), Image.Resampling.LANCZOS)
    img.alpha_composite(icon_resized, ((w - icon_sz) // 2, 42 * scale))

    draw = ImageDraw.Draw(img)
    title_font = load_font(15 * scale, bold=True)
    sub_font = load_font(10 * scale, bold=False)

    t1 = "AdBlock"
    t2 = "Tarayıcı"
    for idx, line in enumerate((t1, t2)):
        bb = draw.textbbox((0, 0), line, font=title_font)
        draw.text(
            ((w - (bb[2] - bb[0])) // 2, (150 + idx * 20) * scale),
            line,
            font=title_font,
            fill=(242, 246, 252, 255),
        )

    # Subtle accent divider bar
    bar_w = 48 * scale
    draw.rounded_rectangle(
        ((w - bar_w) // 2, 202 * scale, (w + bar_w) // 2, 205 * scale),
        radius=2 * scale,
        fill=(92, 212, 210, 220),
    )

    lines = ["Reklamsız Gezinti", "DoH DNS Koruma", "AES-256 Kasa"]
    for idx, line in enumerate(lines):
        bb = draw.textbbox((0, 0), line, font=sub_font)
        draw.text(
            ((w - (bb[2] - bb[0])) // 2, (222 + idx * 17) * scale),
            line,
            font=sub_font,
            fill=(152, 168, 190, 255),
        )

    ver = "v0.1.0  x64"
    vb = draw.textbbox((0, 0), ver, font=sub_font)
    draw.text(((w - (vb[2] - vb[0])) // 2, 288 * scale), ver, font=sub_font, fill=(96, 112, 134, 255))

    return img.convert("RGB")


def build_windows_header(scale: int) -> Image.Image:
    # Inno Setup WizardSmallImageFile: 55x58 (100%), 110x116 (200%)
    w, h = 55 * scale, 58 * scale
    bg = Image.new("RGBA", (w, h), (255, 255, 255, 255))
    icon = Image.open(ICON_PATH).convert("RGBA")
    icon_sz = 46 * scale
    icon_resized = icon.resize((icon_sz, icon_sz), Image.Resampling.LANCZOS)
    bg.alpha_composite(icon_resized, ((w - icon_sz) // 2, (h - icon_sz) // 2))
    return bg.convert("RGB")


def main() -> None:
    INSTALLER_DIR.mkdir(parents=True, exist_ok=True)

    dmg_1x = build_dmg_background(1)
    dmg_2x = build_dmg_background(2)
    dmg_1x_path = INSTALLER_DIR / "dmg_background.png"
    dmg_2x_path = INSTALLER_DIR / "dmg_background@2x.png"
    dmg_1x.save(dmg_1x_path, format="PNG")
    dmg_2x.save(dmg_2x_path, format="PNG")

    build_windows_sidebar(1).save(INSTALLER_DIR / "wizard_sidebar.bmp", format="BMP")
    build_windows_sidebar(2).save(INSTALLER_DIR / "wizard_sidebar_200.bmp", format="BMP")
    build_windows_header(1).save(INSTALLER_DIR / "wizard_header.bmp", format="BMP")
    build_windows_header(2).save(INSTALLER_DIR / "wizard_header_200.bmp", format="BMP")

    print("Generated installer assets in:", INSTALLER_DIR)


if __name__ == "__main__":
    main()
