"""Normalization: RawTag (messy strings) → NormalizedTag (typed, validated)."""

from __future__ import annotations

import re
from collections.abc import Iterable, Iterator

from scraper.models import VR_CODES, NormalizedTag, RawTag, is_private_group

# Accept hex digits OR 'x'/'X' placeholders used by vendors.
_TAG_RE = re.compile(r"^\(\s*([0-9A-Fa-f]{4})\s*,\s*([0-9A-Fa-fxX]{4})\s*\)$")
_VR_RE = re.compile(r"[A-Z]{2}")
_NAME_TO_KEYWORD = re.compile(r"[^A-Za-z0-9]+")

# Trailing column-status suffixes produced by table extraction when the
# creator cell bleeds into the neighbouring "Usage"/"Type" column.
_CREATOR_SUFFIX_RE = re.compile(
    r"\s*[:;,]\s*"
    r"(USED|DEFINED|CONDITIONAL|ALWAYS|OPTIONAL|REQUIRED|GENERATED|FIXED|NO|YES)"
    r"(?:\s+(?:USED|DEFINED|CONDITIONAL|ALWAYS|OPTIONAL|REQUIRED|GENERATED|FIXED|NO|YES))*"
    r"\s*$",
    re.IGNORECASE,
)
# Characters we treat as quote characters when stripping.
_QUOTE_CHARS = "\"'\u201c\u201d\u2018\u2019\u00ab\u00bb"
# Tag-number-shaped strings accidentally captured as creators.
_TAG_LIKE_RE = re.compile(r"^\(?\s*[0-9A-Fa-f]{4}\s*,")


def _clean_creator(raw: str | None) -> str | None:
    """Scrub vendor creator strings of PDF table-extraction artefacts.

    Returns None if the string is clearly not a valid creator (empty,
    tag-number-shaped, too short/long).
    """
    if raw is None:
        return None
    s = raw.strip()
    # Strip surrounding quotes repeatedly.
    while s and s[0] in _QUOTE_CHARS and s[-1] in _QUOTE_CHARS:
        s = s[1:-1].strip()
    # Drop trailing comma/semicolon (but preserve dots in abbreviations
    # like "Forcare B.V.").
    s = s.rstrip(",;").strip()
    # Strip "USED"/"DEFINED" etc. status suffixes (possibly repeated).
    while True:
        new = _CREATOR_SUFFIX_RE.sub("", s).strip()
        if new == s:
            break
        s = new
    # Strip remaining quote chars anywhere at the edge.
    s = s.strip(_QUOTE_CHARS + " ")
    if not s:
        return None
    if _TAG_LIKE_RE.match(s):
        return None
    if len(s) < 3 or len(s) > 64:
        return None
    return s


def _parse_tag(raw: str) -> tuple[int, int, bool] | None:
    """Return (group, element_low8_if_placeholder_else_full, is_block_offset)."""
    m = _TAG_RE.match(raw.strip())
    if not m:
        return None
    g = int(m.group(1), 16)
    elem_s = m.group(2).lower()
    if "x" in elem_s:
        # Vendor format like "xx0C" means the block offset is variable; we
        # store only the low byte (the stable part) and set the flag.
        low_s = elem_s[2:]
        if any(c == "x" for c in low_s):
            return None  # fully wildcard - we can't use it
        return g, int(low_s, 16), True
    return g, int(elem_s, 16), False


def _to_keyword(name: str) -> str:
    # "Auto Window Flag" → "AutoWindowFlag"
    parts = _NAME_TO_KEYWORD.split(name.strip())
    return "".join(p[:1].upper() + p[1:] for p in parts if p)


def _clean_vr(vr: str | None) -> str | None:
    if not vr:
        return None
    vr = vr.strip().upper()
    if vr in VR_CODES:
        return vr
    # Occasionally two VRs separated by ' or ' - take the first.
    m = _VR_RE.match(vr)
    if m and m.group(0) in VR_CODES:
        return m.group(0)
    return None


def normalize(raw: RawTag, vendor: str) -> NormalizedTag | None:
    parsed = _parse_tag(raw.tag_str)
    if parsed is None:
        return None
    group, element, is_block_offset = parsed

    creator = _clean_creator(raw.private_creator)

    # Private tags must live in odd groups. Warn-and-skip if not.
    if creator and not is_private_group(group):
        return None

    vr = _clean_vr(raw.vr)
    vr_inferred = False
    if vr is None:
        vr = "UN"
        vr_inferred = True

    name = (raw.name or "").strip()
    if not name:
        return None

    vm = (raw.vm or "1").strip() or "1"

    try:
        return NormalizedTag(
            group=group,
            element=element,
            element_is_block_offset=is_block_offset,
            private_creator=creator,
            keyword=_to_keyword(name),
            name=name,
            vr=vr,
            vr_inferred=vr_inferred,
            vm=vm,
            description=(raw.description or "").strip(),
            vendor=vendor,
            source_pdf=raw.source_pdf,
            source_page=raw.source_page,
        )
    except ValueError:
        return None


def normalize_all(
    raws: Iterable[RawTag], vendor: str
) -> tuple[list[NormalizedTag], int]:
    """Return (normalized, skipped_count)."""
    out: list[NormalizedTag] = []
    skipped = 0
    for r in raws:
        n = normalize(r, vendor)
        if n is None:
            skipped += 1
        else:
            out.append(n)
    return out, skipped


def iter_normalize(raws: Iterable[RawTag], vendor: str) -> Iterator[NormalizedTag]:
    for r in raws:
        n = normalize(r, vendor)
        if n is not None:
            yield n
