"""Unit tests for the AdBlock engine, cosmetic filtering, and browser storage."""

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from adblock.cosmetic import build_cosmetic_script, build_youtube_early_script
from adblock.dns_guard import DOH_PROVIDERS, DnsGuard, is_local_or_ip_host
from adblock.engine import AdBlockEngine, extract_host, is_third_party
from browser.downloads import format_bytes
from browser.icons import _SVG_PATHS, svg_icon
from browser.internal_pages import (
    render_adblock_page,
    render_bookmarks_page,
    render_downloads_page,
    render_history_page,
    render_passwords_page,
    render_settings_page,
)
from browser.passwords import PasswordManager
from browser.security_dialog import (
    build_cert_error_html,
    build_dns_blocked_html,
    build_https_failed_html,
)
from browser.storage import BookmarksManager, HistoryManager, SettingsManager


class TestAdBlockEngine(unittest.TestCase):
    def setUp(self) -> None:
        self.tmpdir = tempfile.TemporaryDirectory()
        self.data_dir = Path(self.tmpdir.name)
        self.engine = AdBlockEngine(
            filter_dir=self.data_dir / "filter_lists",
            custom_rules_path=self.data_dir / "custom_filters.txt",
        )

    def tearDown(self) -> None:
        self.tmpdir.cleanup()

    def test_domain_and_subdomain_blocking(self) -> None:
        res = self.engine.should_block(
            "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
            first_party_url="https://example.com/article",
            resource_type="script",
        )
        self.assertTrue(res.blocked)
        self.assertEqual(res.reason, "rule")

        res_sub = self.engine.should_block(
            "https://stats.g.doubleclick.net/r/collect",
            first_party_url="https://news.example.com/",
            resource_type="xmlhttprequest",
        )
        self.assertTrue(res_sub.blocked)

    def test_exception_rules_allow_legit_google_assets(self) -> None:
        res = self.engine.should_block(
            "https://fonts.googleapis.com/css2?family=Inter",
            first_party_url="https://example.com/",
            resource_type="stylesheet",
        )
        self.assertFalse(res.blocked)
        self.assertEqual(res.reason, "exception")

    def test_third_party_option(self) -> None:
        self.assertTrue(is_third_party("connect.facebook.net", "example.com"))
        self.assertFalse(is_third_party("connect.facebook.net", "apps.facebook.net"))
        self.assertFalse(is_third_party("www.googletagmanager.com", "googletagmanager.com"))

        # connect.facebook.net$third-party,domain=~facebook.com|~instagram.com blocks on example.com, allows on facebook.com
        blocked_3p = self.engine.should_block(
            "https://connect.facebook.net/en_US/fbevents.js",
            first_party_url="https://example.com/",
            resource_type="script",
        )
        self.assertTrue(blocked_3p.blocked)

        allowed_fb = self.engine.should_block(
            "https://connect.facebook.net/en_US/fbevents.js",
            first_party_url="https://www.facebook.com/",
            resource_type="script",
        )
        self.assertFalse(allowed_fb.blocked)

        allowed_1p = self.engine.should_block(
            "https://www.googletagmanager.com/gtm.js?id=GTM-123",
            first_party_url="https://googletagmanager.com/",
            resource_type="script",
        )
        self.assertFalse(allowed_1p.blocked)

    def test_pattern_wildcard_and_resource_type(self) -> None:
        res = self.engine.should_block(
            "https://cdn.example.com/assets/prebid.min.js",
            first_party_url="https://example.com/",
            resource_type="script",
        )
        self.assertTrue(res.blocked)

        # Same URL as an image should NOT match $script rule
        res_img = self.engine.should_block(
            "https://cdn.example.com/assets/prebid.min.js",
            first_party_url="https://example.com/",
            resource_type="image",
        )
        self.assertFalse(res_img.blocked)

    def test_site_whitelist_disables_network_and_cosmetic_blocking(self) -> None:
        url = "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js"
        site = "https://haber-sitesi.com/detay"

        self.assertTrue(self.engine.should_block(url, site, "script").blocked)
        self.assertGreater(len(self.engine.get_cosmetic_selectors(site)), 0)

        now_whitelisted = self.engine.toggle_site_whitelist("haber-sitesi.com")
        self.assertTrue(now_whitelisted)

        res = self.engine.should_block(url, site, "script")
        self.assertFalse(res.blocked)
        self.assertEqual(res.reason, "whitelisted")
        self.assertEqual(self.engine.get_cosmetic_selectors(site), [])

    def test_custom_rules_and_hosts_syntax_and_cosmetic_exceptions(self) -> None:
        custom = """
        0.0.0.0 tracker-custom.test
        ||special-ads.test/banner*$image
        example.com##.custom-ad-box
        example.com#@#.adsbygoogle
        """
        self.engine.set_custom_rules(custom)

        self.assertTrue(
            self.engine.should_block("https://tracker-custom.test/ping.js", "https://example.com/", "script").blocked
        )
        self.assertTrue(
            self.engine.should_block("https://special-ads.test/banner1.png", "https://example.com/", "image").blocked
        )
        self.assertFalse(
            self.engine.should_block("https://special-ads.test/banner1.png", "https://example.com/", "script").blocked
        )

        selectors = self.engine.get_cosmetic_selectors("https://example.com/page")
        self.assertIn(".custom-ad-box", selectors)
        self.assertNotIn(".adsbygoogle", selectors)

        js = build_cosmetic_script(selectors, "https://example.com/page")
        self.assertIn("__adblock_cosmetic_style", js)
        self.assertIn(".custom-ad-box", js)

    def test_global_disable(self) -> None:
        self.engine.enabled = False
        res = self.engine.should_block(
            "https://doubleclick.net/ad.js",
            first_party_url="https://example.com/",
            resource_type="script",
        )
        self.assertFalse(res.blocked)
        self.assertEqual(res.reason, "disabled")
        self.assertEqual(self.engine.get_cosmetic_selectors("https://youtube.com/watch?v=1"), [])

    def test_extract_host(self) -> None:
        self.assertEqual(extract_host("https://Sub.Example.COM:8080/path"), "sub.example.com")

    def test_youtube_early_script_contains_pruner_and_skipper(self) -> None:
        script = build_youtube_early_script()
        self.assertIn("ytInitialPlayerResponse", script)
        self.assertIn("adPlacements", script)
        self.assertIn("Response.prototype.json", script)
        self.assertIn("playbackRate = 16", script)


class TestBrowserStorage(unittest.TestCase):
    def setUp(self) -> None:
        self.tmpdir = tempfile.TemporaryDirectory()
        self.data_dir = Path(self.tmpdir.name)

    def tearDown(self) -> None:
        self.tmpdir.cleanup()

    def test_bookmarks_persistence(self) -> None:
        bm = BookmarksManager(self.data_dir)
        initial_count = len(bm.items)
        self.assertGreater(initial_count, 0)

        added = bm.toggle("Örnek Site", "https://ornek.com")
        self.assertTrue(added)
        self.assertTrue(bm.is_bookmarked("https://ornek.com"))

        bm_reloaded = BookmarksManager(self.data_dir)
        self.assertTrue(bm_reloaded.is_bookmarked("https://ornek.com"))

        removed = bm_reloaded.toggle("Örnek Site", "https://ornek.com")
        self.assertFalse(removed)
        self.assertFalse(bm_reloaded.is_bookmarked("https://ornek.com"))

    def test_history_persistence_and_search(self) -> None:
        hist = HistoryManager(self.data_dir)
        hist.record("Python Docs", "https://docs.python.org")
        hist.record("Wikipedia", "https://tr.wikipedia.org")
        hist.record("Ignored", "adblock://home")

        self.assertEqual(len(hist.items), 2)
        found = hist.search("python")
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0].url, "https://docs.python.org")

        hist_reloaded = HistoryManager(self.data_dir)
        self.assertEqual(len(hist_reloaded.items), 2)
        hist_reloaded.clear()
        self.assertEqual(len(hist_reloaded.items), 0)

    def test_settings_persistence(self) -> None:
        s = SettingsManager(self.data_dir)
        s.set("search_engine", "duckduckgo")
        s.set("whitelisted_domains", ["example.com"])

        s2 = SettingsManager(self.data_dir)
        self.assertEqual(s2.get("search_engine"), "duckduckgo")
        self.assertEqual(s2.get("whitelisted_domains"), ["example.com"])
        self.assertIn("duckduckgo.com/?q=test", s2.get_search_url("test"))


class TestPasswordManager(unittest.TestCase):
    def setUp(self) -> None:
        self.tmpdir = tempfile.TemporaryDirectory()
        self.data_dir = Path(self.tmpdir.name)

    def tearDown(self) -> None:
        self.tmpdir.cleanup()

    def test_encrypted_persistence_and_update(self) -> None:
        pm = PasswordManager(self.data_dir)
        pm.save_credential("https://github.com/login", "taleh", "S3cretPass!")

        # Plaintext password must never appear in passwords.json
        raw_json = (self.data_dir / "passwords.json").read_text(encoding="utf-8")
        self.assertNotIn("S3cretPass!", raw_json)

        pm2 = PasswordManager(self.data_dir)
        creds = pm2.get_credentials_for_url("https://github.com/session")
        self.assertEqual(len(creds), 1)
        self.assertEqual(creds[0].username, "taleh")
        self.assertEqual(creds[0].password, "S3cretPass!")

        # Never auto-fill HTTPS-saved credential onto insecure HTTP page
        self.assertEqual(pm2.get_credentials_for_url("http://github.com/login"), [])

        # Updating password for same host + username replaces old password
        pm2.save_credential("https://github.com", "taleh", "NewPass!2026")
        pm3 = PasswordManager(self.data_dir)
        creds3 = pm3.get_credentials_for_url("https://github.com")
        self.assertEqual(len(creds3), 1)
        self.assertEqual(creds3[0].password, "NewPass!2026")

    def test_never_save_and_delete(self) -> None:
        pm = PasswordManager(self.data_dir)
        pm.add_never_save("https://bank.example.com/auth")
        self.assertTrue(pm.is_never_saved("https://bank.example.com"))

        pm.save_credential("https://example.org", "user1", "pass1")
        pm.delete_credential("https://example.org", "user1")
        self.assertEqual(pm.get_credentials_for_url("https://example.org"), [])

    def test_format_bytes_helper(self) -> None:
        self.assertEqual(format_bytes(0), "0 B")
        self.assertEqual(format_bytes(512), "512 B")
        self.assertEqual(format_bytes(2048), "2.0 KB")
        self.assertEqual(format_bytes(5 * 1024 * 1024), "5.0 MB")

    def test_svg_icons_catalog_non_empty(self) -> None:
        self.assertGreaterEqual(len(_SVG_PATHS), 20)
        self.assertTrue(callable(svg_icon))


class _FakeHttpResponse:
    def __init__(self, payload: dict) -> None:
        self._raw = json.dumps(payload).encode("utf-8")

    def read(self) -> bytes:
        return self._raw

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False


class TestHttpsAndDnsSecurity(unittest.TestCase):
    def setUp(self) -> None:
        self.tmpdir = tempfile.TemporaryDirectory()
        self.data_dir = Path(self.tmpdir.name)

    def tearDown(self) -> None:
        self.tmpdir.cleanup()

    def test_local_and_ip_hosts_detected(self) -> None:
        self.assertTrue(is_local_or_ip_host("localhost"))
        self.assertTrue(is_local_or_ip_host("127.0.0.1"))
        self.assertTrue(is_local_or_ip_host("myprinter.local"))
        self.assertFalse(is_local_or_ip_host("example.com"))

    def test_settings_security_defaults(self) -> None:
        s = SettingsManager(self.data_dir)
        self.assertTrue(s.get("https_only_enabled"))
        self.assertEqual(s.get("doh_provider"), "cloudflare_security")
        self.assertEqual(s.get("http_allowed_domains"), [])
        self.assertIn("cloudflare_security", DOH_PROVIDERS)

    def test_dns_guard_sinkhole_and_cache(self) -> None:
        guard = DnsGuard(provider_id="cloudflare_security", timeout=1.0)
        self.assertFalse(guard.check_domain("127.0.0.1").blocked)

        with patch(
            "urllib.request.urlopen",
            return_value=_FakeHttpResponse(
                {"Status": 0, "Answer": [{"type": 1, "data": "0.0.0.0"}]}
            ),
        ) as mock_open:
            res = guard.check_domain("malware.test")
            self.assertTrue(res.blocked)
            self.assertEqual(guard.total_blocked, 1)
            # Second lookup hits TTL cache rather than repeating network call
            res_cached = guard.check_domain("malware.test")
            self.assertTrue(res_cached.blocked)
            self.assertEqual(mock_open.call_count, 1)

        # User override unblocks host
        guard.allow_host_override("malware.test")
        res_override = guard.check_domain("malware.test")
        self.assertFalse(res_override.blocked)
        self.assertEqual(res_override.reason, "override")

    def test_dns_guard_nxdomain_blocked_comment_and_offline_failopen(self) -> None:
        guard = DnsGuard(provider_id="adguard", timeout=1.0)
        with patch(
            "urllib.request.urlopen",
            return_value=_FakeHttpResponse(
                {"Status": 3, "Comment": ["Blocked phishing domain"]}
            ),
        ):
            res = guard.check_domain("phish.example")
            self.assertTrue(res.blocked)
            self.assertIn("phish", res.reason.lower())

        with patch("urllib.request.urlopen", side_effect=OSError("network down")):
            res_err = guard.check_domain("offline.example")
            self.assertFalse(res_err.blocked)
            self.assertEqual(res_err.reason, "offline_fallback")

    def test_interceptor_https_upgrade_and_dns_block(self) -> None:
        from PyQt6.QtCore import QUrl
        from PyQt6.QtWebEngineCore import QWebEngineUrlRequestInfo
        from adblock.interceptor import AdBlockInterceptor

        engine = AdBlockEngine(
            filter_dir=self.data_dir / "filter_lists",
            custom_rules_path=self.data_dir / "custom_filters.txt",
        )
        guard = DnsGuard(provider_id="cloudflare_security")
        interceptor = AdBlockInterceptor(engine, dns_guard=guard, https_only_enabled=True)

        class FakeInfo:
            def __init__(self, url: str, rtype):
                self._url = QUrl(url)
                self._fp = QUrl(url)
                self._rtype = rtype
                self.redirected_to = None
                self.blocked = False

            def requestUrl(self):
                return self._url

            def firstPartyUrl(self):
                return self._fp

            def resourceType(self):
                return self._rtype

            def redirect(self, qurl):
                self.redirected_to = qurl.toString()

            def block(self, flag):
                self.blocked = flag

        main_type = QWebEngineUrlRequestInfo.ResourceType.ResourceTypeMainFrame

        # External HTTP gets upgraded to HTTPS
        info_http = FakeInfo("http://example.com/page", main_type)
        interceptor.interceptRequest(info_http)
        self.assertEqual(info_http.redirected_to, "https://example.com/page")
        self.assertEqual(interceptor.https_upgraded_count, 1)

        # Allowed HTTP exception bypasses upgrade
        interceptor.allow_http_for_host("example.com")
        self.assertTrue(interceptor.is_http_allowed_for_host("example.com"))
        info_allowed = FakeInfo("http://example.com/page", main_type)
        with patch.object(guard, "check_domain", return_value=guard.check_domain("localhost")):
            interceptor.interceptRequest(info_allowed)
        self.assertIsNone(info_allowed.redirected_to)

        # Localhost skips upgrade
        info_local = FakeInfo("http://localhost:8000/", main_type)
        interceptor.interceptRequest(info_local)
        self.assertIsNone(info_local.redirected_to)

    def test_security_interstitial_html_escapes(self) -> None:
        dns_html = build_dns_blocked_html("https://bad.test/?a=1&b=2", "bad.test", "Cloudflare", "sinkhole")
        self.assertIn("DNS Koruması", dns_html)
        self.assertIn("adblock://allow-dns?url=", dns_html)

        cert_html = build_cert_error_html("https://self.test", "self.test", "<script>alert(1)</script>", True)
        self.assertNotIn("<script>alert(1)</script>", cert_html)
        self.assertIn("adblock://accept-cert?url=", cert_html)

        https_html = build_https_failed_html("https://legacy.test", "http://legacy.test", "legacy.test")
        self.assertIn("adblock://allow-http?url=", https_html)

    def test_internal_tab_pages_render_html(self) -> None:
        from PyQt6.QtWidgets import QApplication
        from browser.downloads import DownloadsManager

        _app = QApplication.instance() or QApplication(["adblock-browser"])
        s = SettingsManager(self.data_dir)
        b = BookmarksManager(self.data_dir)
        h = HistoryManager(self.data_dir)
        p = PasswordManager(self.data_dir)
        eng = AdBlockEngine(
            filter_dir=self.data_dir / "filter_lists",
            custom_rules_path=self.data_dir / "custom_filters.txt",
        )
        dl = DownloadsManager()

        s_html = render_settings_page(s, eng)
        self.assertIn("adblock-cmd://save-settings", s_html)
        self.assertIn("Cloudflare", s_html)

        d_html = render_downloads_page(dl)
        self.assertIn("adblock-cmd://dl-clear", d_html)

        p_html = render_passwords_page(p)
        self.assertIn("Şifre Yöneticisi", p_html)

        h_html = render_history_page(h)
        self.assertIn("adblock-cmd://hist-clear", h_html)

        b_html = render_bookmarks_page(b)
        self.assertIn("adblock-cmd://bm-add", b_html)

        ab_html = render_adblock_page(eng)
        self.assertIn("adblock-cmd://ab-save-rules", ab_html)



if __name__ == "__main__":
    unittest.main()
