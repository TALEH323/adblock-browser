"""Fast, thread-safe EasyList/ABP-compatible AdBlock engine."""

from __future__ import annotations

import os
import re
import threading
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple
from urllib.parse import urlparse

from .default_filters import BUILTIN_FILTER_LISTS, DEFAULT_FILTER_RULES

KNOWN_RESOURCE_TYPES = {
    "script",
    "image",
    "stylesheet",
    "xmlhttprequest",
    "xhr",
    "subdocument",
    "media",
    "font",
    "ping",
    "object",
    "websocket",
    "other",
}

_PURE_DOMAIN_RE = re.compile(r"^\|\|([a-z0-9.-]+)\^?$", re.IGNORECASE)
_HOSTS_LINE_RE = re.compile(r"^(?:0\.0\.0\.0|127\.0\.0\.1)\s+([a-z0-9.-]+)(?:\s+#.*)?$", re.IGNORECASE)


def extract_host(url: str) -> str:
    """Extract normalized lowercase hostname without port from a URL."""
    if not url:
        return ""
    if "://" not in url:
        url = "http://" + url
    try:
        parsed = urlparse(url)
        host = (parsed.hostname or "").lower().rstrip(".")
        return host
    except Exception:
        return ""


def iter_parent_domains(host: str) -> List[str]:
    """Return host and its parent domains (e.g. a.b.com -> ['a.b.com', 'b.com', 'com'])."""
    if not host:
        return []
    parts = host.split(".")
    return [".".join(parts[i:]) for i in range(len(parts))]


def get_registrable_domain(host: str) -> str:
    """Approximate eTLD+1 for third-party comparison without external dependencies."""
    if not host:
        return ""
    parts = host.split(".")
    if len(parts) <= 2:
        return host
    # Handle common multi-part ccTLDs like .co.uk, .com.tr, .org.tr, .com.au
    second_level = parts[-2]
    tld = parts[-1]
    if len(tld) == 2 and second_level in {"co", "com", "org", "net", "gov", "edu", "ac"} and len(parts) >= 3:
        return ".".join(parts[-3:])
    return ".".join(parts[-2:])


def is_third_party(request_host: str, page_host: str) -> bool:
    """Return True if request_host belongs to a different site than page_host."""
    if not request_host or not page_host:
        return False
    return get_registrable_domain(request_host) != get_registrable_domain(page_host)


@dataclass
class NetworkRule:
    """Parsed network filter or exception rule."""

    raw: str
    is_exception: bool = False
    domain_only: Optional[str] = None
    regex: Optional[re.Pattern[str]] = None
    literal_hint: str = ""
    third_party: Optional[bool] = None  # True=3p only, False=1p only, None=any
    include_types: Set[str] = field(default_factory=set)
    exclude_types: Set[str] = field(default_factory=set)
    include_domains: Set[str] = field(default_factory=set)
    exclude_domains: Set[str] = field(default_factory=set)
    important: bool = False

    def matches_options(self, req_host: str, page_host: str, resource_type: str) -> bool:
        """Check whether rule options match the request context."""
        if self.third_party is not None:
            tp = is_third_party(req_host, page_host)
            if self.third_party != tp:
                return False

        norm_type = "xmlhttprequest" if resource_type == "xhr" else resource_type
        if self.include_types:
            if norm_type not in self.include_types:
                return False
        if self.exclude_types and norm_type in self.exclude_types:
            return False

        if self.include_domains or self.exclude_domains:
            page_parents = set(iter_parent_domains(page_host))
            if self.exclude_domains and (page_parents & self.exclude_domains):
                return False
            if self.include_domains and not (page_parents & self.include_domains):
                return False

        return True

    def matches_url(self, url_lower: str) -> bool:
        """Check whether pattern matches the lowercased URL."""
        if self.literal_hint and self.literal_hint not in url_lower:
            return False
        if self.regex is not None:
            return bool(self.regex.search(url_lower))
        return True


@dataclass
class BlockResult:
    """Outcome of checking a request URL against the AdBlock engine."""

    blocked: bool
    rule: str = ""
    reason: str = ""  # "rule", "exception", "whitelisted", "disabled", "allowed"


def _compile_abp_pattern(pattern: str) -> Tuple[Optional[re.Pattern[str]], str]:
    """Convert an ABP pattern string into a compiled regex and a fast literal hint."""
    if not pattern:
        return None, ""

    # Regex literal /pattern/
    if len(pattern) >= 2 and pattern.startswith("/") and pattern.endswith("/"):
        try:
            return re.compile(pattern[1:-1], re.IGNORECASE), ""
        except re.error:
            return None, ""

    anchor_start = False
    anchor_domain = False
    anchor_end = False

    work = pattern
    if work.startswith("||"):
        anchor_domain = True
        work = work[2:]
    elif work.startswith("|"):
        anchor_start = True
        work = work[1:]

    if work.endswith("|"):
        anchor_end = True
        work = work[:-1]

    # Pick longest alphanumeric literal chunk >= 4 chars for quick prefilter
    chunks = re.findall(r"[a-z0-9.-]{4,}", work.lower())
    hint = max(chunks, key=len) if chunks else ""

    escaped = []
    for ch in work:
        if ch == "*":
            escaped.append(".*")
        elif ch == "^":
            # Separator character in ABP: anything except alphanumeric, _, -, ., %
            escaped.append(r"(?:[^a-zA-Z0-9_.\-%]|$)")
        else:
            escaped.append(re.escape(ch))

    body = "".join(escaped)
    if anchor_domain:
        prefix = r"^https?://(?:[^/]+\.)?"
    elif anchor_start:
        prefix = r"^"
    else:
        prefix = ""

    suffix = r"$" if anchor_end else ""
    try:
        compiled = re.compile(prefix + body + suffix, re.IGNORECASE)
        return compiled, hint
    except re.error:
        return None, ""


def _looks_like_options(opt_str: str) -> bool:
    """Heuristic to distinguish ABP $options from query parameters containing $."""
    if not opt_str:
        return False
    tokens = [t.strip().lower() for t in opt_str.split(",") if t.strip()]
    if not tokens:
        return False
    for tok in tokens:
        base = tok.lstrip("~")
        if base in KNOWN_RESOURCE_TYPES or base in {"third-party", "3p", "1p", "first-party", "important", "match-case", "document", "popup"}:
            continue
        if base.startswith("domain="):
            continue
        return False
    return True


def _parse_options(opt_str: str, rule: NetworkRule) -> bool:
    """Populate rule options; return False if rule uses unsupported blocking-only options like $document."""
    tokens = [t.strip() for t in opt_str.split(",") if t.strip()]
    for tok in tokens:
        low = tok.lower()
        neg = low.startswith("~")
        base = low[1:] if neg else low

        if base in {"third-party", "3p"}:
            rule.third_party = not neg
        elif base in {"first-party", "1p"}:
            rule.third_party = neg
        elif base == "important":
            rule.important = True
        elif base == "xhr":
            (rule.exclude_types if neg else rule.include_types).add("xmlhttprequest")
        elif base in KNOWN_RESOURCE_TYPES:
            (rule.exclude_types if neg else rule.include_types).add(base)
        elif base.startswith("domain="):
            dom_list = tok[len("domain="):].split("|")
            for dom in dom_list:
                d = dom.strip().lower().lstrip("*.")
                if not d:
                    continue
                if d.startswith("~"):
                    rule.exclude_domains.add(d[1:])
                else:
                    rule.include_domains.add(d)
        elif base in {"document", "elemhide", "generichide", "genericblock"}:
            # Skip document-level exception flags in simplified URL interceptor
            return False
    return True


class AdBlockEngine:
    """Manages EasyList/ABP network & cosmetic rules, per-site whitelisting, and stats."""

    def __init__(
        self,
        filter_dir: Optional[Path] = None,
        custom_rules_path: Optional[Path] = None,
    ) -> None:
        self._lock = threading.RLock()
        self.enabled: bool = True
        self.cosmetic_enabled: bool = True
        self.whitelisted_domains: Set[str] = set()

        self.filter_dir = Path(filter_dir) if filter_dir else None
        self.custom_rules_path = Path(custom_rules_path) if custom_rules_path else None

        # Network indices
        self._domain_block: Dict[str, List[NetworkRule]] = {}
        self._domain_allow: Dict[str, List[NetworkRule]] = {}
        self._pattern_block: List[NetworkRule] = []
        self._pattern_allow: List[NetworkRule] = []

        # Cosmetic indices
        self._generic_cosmetic: List[str] = []
        self._domain_cosmetic: Dict[str, List[str]] = {}
        self._excluded_cosmetic: Dict[str, Set[str]] = {}

        # Stats
        self.total_blocked: int = 0
        self.total_rules_count: int = 0
        self.custom_rules_text: str = ""

        self.reload_rules()

    def reload_rules(self) -> int:
        """Load built-in rules, cached filter lists, and custom user rules."""
        sources: List[str] = [DEFAULT_FILTER_RULES]

        if self.filter_dir and self.filter_dir.exists():
            for txt_file in sorted(self.filter_dir.glob("*.txt")):
                try:
                    sources.append(txt_file.read_text(encoding="utf-8", errors="ignore"))
                except OSError:
                    pass

        if self.custom_rules_path and self.custom_rules_path.exists():
            try:
                self.custom_rules_text = self.custom_rules_path.read_text(encoding="utf-8", errors="ignore")
                sources.append(self.custom_rules_text)
            except OSError:
                self.custom_rules_text = ""
        elif self.custom_rules_text:
            sources.append(self.custom_rules_text)

        domain_block: Dict[str, List[NetworkRule]] = {}
        domain_allow: Dict[str, List[NetworkRule]] = {}
        pattern_block: List[NetworkRule] = []
        pattern_allow: List[NetworkRule] = []
        generic_cosmetic_set: Set[str] = set()
        domain_cosmetic: Dict[str, List[str]] = {}
        excluded_cosmetic: Dict[str, Set[str]] = {}
        count = 0

        for src in sources:
            for raw_line in src.splitlines():
                line = raw_line.strip()
                if not line or line.startswith("!") or line.startswith("["):
                    continue

                # Cosmetic exception: domain#@#selector
                if "#@#" in line:
                    dom_part, sel = line.split("#@#", 1)
                    sel = sel.strip()
                    if sel and dom_part:
                        for d in dom_part.split(","):
                            d = d.strip().lower()
                            if d:
                                excluded_cosmetic.setdefault(d, set()).add(sel)
                                count += 1
                    continue

                # Ignore extended AdGuard/uBO scriptlet/procedural rules
                if "#?#" in line or "#$#" in line or "##+js(" in line:
                    continue

                # Cosmetic hiding rule: [domains]##selector
                if "##" in line:
                    dom_part, sel = line.split("##", 1)
                    sel = sel.strip()
                    if not sel:
                        continue
                    if not dom_part.strip():
                        generic_cosmetic_set.add(sel)
                        count += 1
                    else:
                        for d in dom_part.split(","):
                            d = d.strip().lower()
                            if not d or d.startswith("~"):
                                continue
                            domain_cosmetic.setdefault(d, []).append(sel)
                            count += 1
                    continue

                # Hosts-file style rule
                hosts_match = _HOSTS_LINE_RE.match(line)
                if hosts_match:
                    dom = hosts_match.group(1).lower()
                    if dom not in {"localhost", "localhost.localdomain", "broadcasthost", "local"}:
                        rule = NetworkRule(raw=line, is_exception=False, domain_only=dom)
                        domain_block.setdefault(dom, []).append(rule)
                        count += 1
                    continue

                # Network rule
                is_exc = line.startswith("@@")
                body = line[2:] if is_exc else line

                opt_str = ""
                if "$" in body and not (body.startswith("/") and body.endswith("/")):
                    pat_cand, opt_cand = body.rsplit("$", 1)
                    if _looks_like_options(opt_cand):
                        body = pat_cand
                        opt_str = opt_cand

                rule = NetworkRule(raw=line, is_exception=is_exc)
                if opt_str and not _parse_options(opt_str, rule):
                    continue

                # Pure domain rule: ||example.com^ or ||example.com
                dom_match = _PURE_DOMAIN_RE.match(body)
                if dom_match:
                    dom = dom_match.group(1).lower().rstrip(".")
                    if dom:
                        rule.domain_only = dom
                        target = domain_allow if is_exc else domain_block
                        target.setdefault(dom, []).append(rule)
                        count += 1
                    continue

                compiled, hint = _compile_abp_pattern(body)
                if compiled is None:
                    continue
                rule.regex = compiled
                rule.literal_hint = hint
                if is_exc:
                    pattern_allow.append(rule)
                else:
                    pattern_block.append(rule)
                count += 1

        with self._lock:
            self._domain_block = domain_block
            self._domain_allow = domain_allow
            self._pattern_block = pattern_block
            self._pattern_allow = pattern_allow
            self._generic_cosmetic = sorted(generic_cosmetic_set)
            self._domain_cosmetic = domain_cosmetic
            self._excluded_cosmetic = excluded_cosmetic
            self.total_rules_count = count
        return count

    def set_custom_rules(self, text: str) -> int:
        """Save custom rules text and reload engine."""
        with self._lock:
            self.custom_rules_text = text
            if self.custom_rules_path:
                self.custom_rules_path.parent.mkdir(parents=True, exist_ok=True)
                self.custom_rules_path.write_text(text, encoding="utf-8")
        return self.reload_rules()

    def is_site_whitelisted(self, url_or_host: str) -> bool:
        """Check if a site or any of its parent domains is whitelisted."""
        host = extract_host(url_or_host) if "://" in url_or_host else url_or_host.lower().strip()
        if not host:
            return False
        with self._lock:
            for parent in iter_parent_domains(host):
                if parent in self.whitelisted_domains:
                    return True
        return False

    def toggle_site_whitelist(self, url_or_host: str) -> bool:
        """Toggle whitelist state for the host. Returns True if now whitelisted."""
        host = extract_host(url_or_host) if "://" in url_or_host else url_or_host.lower().strip()
        if not host:
            return False
        with self._lock:
            if host in self.whitelisted_domains:
                self.whitelisted_domains.remove(host)
                return False
            # Also remove parent if user explicitly re-enables on exact host
            self.whitelisted_domains.add(host)
            return True

    def set_site_whitelisted(self, host: str, whitelisted: bool) -> None:
        host = extract_host(host) if "://" in host else host.lower().strip()
        if not host:
            return
        with self._lock:
            if whitelisted:
                self.whitelisted_domains.add(host)
            else:
                self.whitelisted_domains.discard(host)

    def should_block(
        self,
        request_url: str,
        first_party_url: str = "",
        resource_type: str = "other",
    ) -> BlockResult:
        """Evaluate whether a network request should be blocked."""
        if not request_url:
            return BlockResult(False, reason="allowed")

        # Never block non-network schemes (devtools, data, blob, file, about, qrc)
        low_url = request_url.lower()
        if not (low_url.startswith("http://") or low_url.startswith("https://") or low_url.startswith("ws://") or low_url.startswith("wss://")):
            return BlockResult(False, reason="allowed")

        req_host = extract_host(request_url)
        page_host = extract_host(first_party_url) or req_host

        with self._lock:
            if not self.enabled:
                return BlockResult(False, reason="disabled")

            # Never block top-level document navigation unless explicitly intended,
            # and skip whitelisted sites.
            if page_host and self.is_site_whitelisted(page_host):
                return BlockResult(False, reason="whitelisted")

            if resource_type == "main_frame":
                return BlockResult(False, reason="allowed")

            req_parents = iter_parent_domains(req_host)

            # 1. Check domain allow exceptions first
            for dom in req_parents:
                for rule in self._domain_allow.get(dom, ()):
                    if rule.matches_options(req_host, page_host, resource_type):
                        return BlockResult(False, rule=rule.raw, reason="exception")

            # 2. Check pattern allow exceptions
            for rule in self._pattern_allow:
                if rule.matches_url(low_url) and rule.matches_options(req_host, page_host, resource_type):
                    return BlockResult(False, rule=rule.raw, reason="exception")

            # 3. Check domain block rules (O(labels))
            for dom in req_parents:
                for rule in self._domain_block.get(dom, ()):
                    if rule.matches_options(req_host, page_host, resource_type):
                        self.total_blocked += 1
                        return BlockResult(True, rule=rule.raw, reason="rule")

            # 4. Check pattern block rules
            for rule in self._pattern_block:
                if rule.matches_url(low_url) and rule.matches_options(req_host, page_host, resource_type):
                    self.total_blocked += 1
                    return BlockResult(True, rule=rule.raw, reason="rule")

        return BlockResult(False, reason="allowed")

    def get_cosmetic_selectors(self, page_url: str) -> List[str]:
        """Return active CSS selectors to hide for a given page URL."""
        host = extract_host(page_url)
        with self._lock:
            if not self.enabled or not self.cosmetic_enabled:
                return []
            if host and self.is_site_whitelisted(host):
                return []

            excluded: Set[str] = set()
            selectors: List[str] = list(self._generic_cosmetic)

            for dom in iter_parent_domains(host):
                if dom in self._excluded_cosmetic:
                    excluded |= self._excluded_cosmetic[dom]
                if dom in self._domain_cosmetic:
                    selectors.extend(self._domain_cosmetic[dom])

            if excluded:
                selectors = [s for s in selectors if s not in excluded]

            # Deduplicate while preserving order
            seen: Set[str] = set()
            unique: List[str] = []
            for s in selectors:
                if s not in seen:
                    seen.add(s)
                    unique.append(s)
            return unique

    def update_filter_lists(self, timeout: int = 15) -> Dict[str, str]:
        """Download built-in external filter lists into filter_dir and reload rules."""
        if not self.filter_dir:
            raise ValueError("filter_dir is not configured")
        self.filter_dir.mkdir(parents=True, exist_ok=True)
        results: Dict[str, str] = {}

        headers = {"User-Agent": "AdBlockBrowser/1.0"}
        for item in BUILTIN_FILTER_LISTS:
            list_id = item["id"]
            url = item["url"]
            dest = self.filter_dir / f"{list_id}.txt"
            try:
                req = urllib.request.Request(url, headers=headers)
                with urllib.request.urlopen(req, timeout=timeout) as resp:
                    data = resp.read().decode("utf-8", errors="ignore")
                if len(data.splitlines()) > 10:
                    dest.write_text(data, encoding="utf-8")
                    results[item["name"]] = f"OK ({len(data.splitlines())} satır)"
                else:
                    results[item["name"]] = "Boş yanıt"
            except Exception as exc:
                results[item["name"]] = f"Hata: {exc}"

        self.reload_rules()
        return results
