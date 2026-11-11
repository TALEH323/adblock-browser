import os

ROOT_DIR = os.path.abspath(defines.get("root", os.getcwd()))
APP_PATH = defines.get("app", os.path.join(ROOT_DIR, "dist", "AdBlockBrowser.app"))
APP_NAME = os.path.basename(APP_PATH)

format = "UDZO"
compression_level = 9
size = None

files = [APP_PATH]
symlinks = {"Applications": "/Applications"}

icon_icns = os.path.join(ROOT_DIR, "assets", "AppIcon.icns")
if os.path.exists(icon_icns):
    icon = icon_icns

background_tiff = os.path.join(ROOT_DIR, "assets", "installer", "dmg_background.tiff")
background_png = os.path.join(ROOT_DIR, "assets", "installer", "dmg_background.png")
background = background_tiff if os.path.exists(background_tiff) else background_png

show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
sidebar_width = 180

window_rect = ((220, 140), (660, 420))
default_view = "icon-view"
show_icon_preview = False
include_icon_view_settings = True
include_list_view_settings = False

arrange_by = None
grid_offset = (0, 0)
grid_spacing = 100
scroll_position = (0, 0)
label_pos = "bottom"
text_size = 13
icon_size = 112

icon_locations = {
    APP_NAME: (175, 222),
    "Applications": (485, 222),
}
