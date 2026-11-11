"""Built-in EasyList-compatible default network and cosmetic filter rules."""

DEFAULT_FILTER_RULES = """! Title: Built-in AdBlock Default Filters
! Syntax: Adblock Plus / EasyList compatible

! === Major Ad Networks & Ad Exchanges ===
||doubleclick.net^
||googlesyndication.com^
||googleadservices.com^
||adservice.google.com^
||adservice.google.com.tr^
||pagead2.googlesyndication.com^
||tpc.googlesyndication.com^
||partner.googleadservices.com^
||afs.googlesyndication.com^
||imasdk.googleapis.com^
||google-analytics.com^
||googletagmanager.com^$third-party
||googletagservices.com^
||adnxs.com^
||ib.adnxs.com^
||criteo.com^
||criteo.net^
||taboola.com^
||outbrain.com^
||outbrainimg.com^
||pubmatic.com^
||rubiconproject.com^
||openx.net^
||casalemedia.com^
||indexww.com^
||smartadserver.com^
||adform.net^
||adroll.com^
||advertising.com^
||adsafeprotected.com^
||moatads.com^
||doubleverify.com^
||media.net^
||bidswitch.net^
||sharethrough.com^
||triplelift.com^
||3lift.com^
||spotxchange.com^
||spotx.tv^
||teads.tv^
||yieldmo.com^
||sovrn.com^
||lijit.com^
||quantserve.com^
||scorecardresearch.com^
||amazon-adsystem.com^
||aax.amazon-adsystem.com^
||fls-na.amazon-adsystem.com^
||ads.yahoo.com^
||gemini.yahoo.com^
||ads.linkedin.com^
||px.ads.linkedin.com^
||ads-twitter.com^
||static.ads-twitter.com^
||analytics.twitter.com^
||an.facebook.com^
||pixel.facebook.com^
||connect.facebook.net^$third-party,domain=~facebook.com|~instagram.com
||analytics.tiktok.com^
||ads.tiktok.com^
||sc-static.net^$third-party
||tr.snapchat.com^
||ads.pinterest.com^
||ct.pinterest.com^
||bat.bing.com^
||c.bing.com^
||ads.reddit.com^
||events.redditmedia.com^
||mc.yandex.ru^$third-party
||an.yandex.ru^
||yadro.ru^
||hotjar.com^$third-party
||mixpanel.com^$third-party
||segment.io^$third-party
||segment.com^$third-party
||amplitude.com^$third-party
||clarity.ms^$third-party
||chartbeat.com^
||chartbeat.net^
||newrelic.com^$third-party
||nr-data.net^
||omtrdc.net^
||demdex.net^
||krxd.net^
||bluekai.com^
||rlcdn.com^
||exelator.com^
||tapad.com^
||turn.com^
||mathtag.com^
||everesttech.net^
||sizmek.com^
||serving-sys.com^
||flashtalking.com^
||innovid.com^
||celtra.com^
||revcontent.com^
||mgid.com^
||propellerads.com^
||popads.net^
||popcash.net^
||adsterra.com^
||exoclick.com^
||juicyads.com^
||trafficjunky.net^
||hilltopads.net^
||pushnami.com^
||onesignal.com^$third-party

! === Turkish & Regional Ad Networks ===
||admatic.com.tr^
||reklamstore.com^
||reklamz.com^
||virgul.com^
||gemius.pl^
||hit.gemius.pl^
||medyanetads.com^
||turkuvazads.com^
||adhood.com^
||wordego.com^
||admost.com^

! === YouTube Ad & Tracking Endpoints ===
||youtube.com/api/stats/ads$xmlhttprequest
||youtube.com/pagead/
||youtube.com/ptracking

! === Common Ad URL Patterns ===
*/ads/ad-*
*/ads/banner*
*/adserver/*
*/adsystem/*
*/pagead/*
*/doubleclick/*
*/prebid*.js$script
*/gpt.js$script
*/show_ads.js$script
*/adsbygoogle.js$script
*/adframe.*
*/popunder*.js$script
*/banner_ads/*
*/sponsored_links/*

! === Exception Rules (prevent false positives) ===
@@||apis.google.com^
@@||accounts.google.com^
@@||ssl.gstatic.com^
@@||fonts.googleapis.com^
@@||fonts.gstatic.com^
@@||ajax.googleapis.com^
@@||cdn.jsdelivr.net^
@@||cdnjs.cloudflare.com^

! === Cosmetic Filtering (Element Hiding) ===
##ins.adsbygoogle
##.adsbygoogle
##[id^="google_ads_iframe"]
##[id^="div-gpt-ad"]
##[data-google-query-id]
##iframe[src*="doubleclick.net"]
##iframe[src*="googlesyndication.com"]
##iframe[id^="google_ads_"]
##.taboola-widget
##[id^="taboola-"]
##.OUTBRAIN
##.ob-widget
##[data-widget-id^="FMS_"]
##.mgid-widget
##[id^="MarketGidScriptRoot"]
##.ad-banner
##.ad-container
##.ad-wrapper
##.ad-slot
##.ad-leaderboard
##.ad-sidebar
##.advertisement-box
##.sponsored-content-box
##[aria-label="advertisement"]
##[aria-label="Advertisement"]
##[aria-label="Reklam"]

! === YouTube Cosmetic Ad Hiding ===
youtube.com##ytd-ad-slot-renderer
youtube.com##ytd-banner-promo-renderer
youtube.com##ytd-promoted-sparkles-web-renderer
youtube.com##ytd-display-ad-renderer
youtube.com##ytd-companion-slot-renderer
youtube.com##ytd-action-companion-ad-renderer
youtube.com##ytd-in-feed-ad-layout-renderer
youtube.com##ytd-statement-banner-renderer
youtube.com###masthead-ad
youtube.com###player-ads
youtube.com##.ytd-merch-shelf-renderer
youtube.com##.ytp-ad-overlay-container
youtube.com##.ytp-ad-text-overlay
youtube.com##ytm-promoted-sparkles-web-renderer
"""

BUILTIN_FILTER_LISTS = [
    {
        "id": "peter_lowe",
        "name": "Peter Lowe's Ad and Tracking Server List",
        "url": "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=adblockplus&showintro=0&mimetype=plaintext",
    },
    {
        "id": "easylist_domains",
        "name": "AdGuard / EasyList Simplified Domain List",
        "url": "https://adguardteam.github.io/AdGuardSDNSFilter/Filters/filter.txt",
    },
]
