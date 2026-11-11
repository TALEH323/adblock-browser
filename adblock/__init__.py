"""AdBlock engine, URL request interceptor, and cosmetic filtering package."""

from .dns_guard import DOH_PROVIDERS, DnsCheckResult, DnsGuard
from .engine import AdBlockEngine, BlockResult
from .interceptor import AdBlockInterceptor
from .cosmetic import build_cosmetic_script, build_youtube_early_script

__all__ = [
    "AdBlockEngine",
    "BlockResult",
    "AdBlockInterceptor",
    "DOH_PROVIDERS",
    "DnsCheckResult",
    "DnsGuard",
    "build_cosmetic_script",
    "build_youtube_early_script",
]
