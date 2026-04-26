"""Canon Medical (incl. legacy Toshiba) DICOM conformance harvester.

Three formats encountered in Canon conformance statements:

Format A — plain text lines (not captured in pdfplumber tables):
    ``(GGGG,xxEE)[*]  Attribute Name  VR  VM``
    The creator declaration row ``(GGGG,00xx)`` carries no creator string;
    ``private_creator`` is emitted as ``None``.

Format B — 4-column table, tag-first:
    Tag | Attribute Name | VR | VM
    Creator declaration: ``(GGGG,00xx) | Private Creator Code | LO | 1``
    No creator string is documented in these PDFs; emitted as ``None``.

Format C — ≥5-column table, name-first (Toshiba legacy / older Canon):
    Attribute Name | Tag | VR | Value | Presence of Value | Source
    Creator row: ``Private Creator Code | (GGGG,00xx) | LO | "CREATOR_STR" …``
    Creator string is extracted from the Value column (quotes stripped).

Harvest strategy
----------------
Pass 1: scan the entire PDF for Format-C creator strings and build a
``creator_map: dict[group_int → creator_str]``.
Pass 2: emit ``RawTag`` entries for every private block-offset tag found
in tables (Formats B/C) and in plain text (Format A), looking up the
creator from ``creator_map`` (``None`` if absent).

All emitted tag_str values are already in ``(GGGG,xxEE)`` form; no
rewriting to block-offset form is needed (unlike Acuson).
"""

from __future__ import annotations

import re
from collections.abc import Iterator

import pdfplumber

from scraper.harvest.base import Harvester
from scraper.models import RawTag

# ---------------------------------------------------------------------------
# Compiled patterns
# ---------------------------------------------------------------------------

# Pre-filter for plain-text scanning: page has 'private' or an xx-element tag.
_FILTER_RE = re.compile(r"private|\(\s*[0-9a-f]{4}\s*,\s*[0-9a-f]{0,2}xx", re.I)

# Stricter pre-filter for table extraction: page must have an actual
# ``(GGGG,xxEE)`` or ``(GGGG,00xx)`` block-offset token (odd-group preferred,
# but we accept any private-creator-form tag first then screen in the parser).
# This avoids the expensive extract_tables() call on public-attribute pages.
_TABLE_FILTER_RE = re.compile(
    r"\(\s*[0-9A-Fa-f]{4}\s*,\s*(?:[0-9A-Fa-f]{0,2}xx|xx[0-9A-Fa-f]{2})\s*\)"
)

# Plain-text line: ``(GGGG,xxEE)[*]  Name  VR  VM``
# Group 1 = GGGG, group 2 = element (xxEE or 00xx), group 3 = name,
# group 4 = VR (2-char), group 5 = VM (digits + optional range suffix).
_PLAIN_LINE_RE = re.compile(
    r"^\(\s*([0-9A-Fa-f]{4})\s*,\s*(xx[0-9A-Fa-f]{2}|00xx)\s*\)\*?\s+"
    r"(.+?)\s+([A-Z][A-Z0-9])\s+(\S+)\s*$"
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _cell(val: object) -> str:
    """Normalise a pdfplumber cell value to a stripped single-line string."""
    return (val or "").strip().replace("\n", " ").strip()


def _strip_creator_quotes(raw: str) -> str | None:
    """Strip surrounding quotes/whitespace from a creator value cell."""
    s = raw.strip().strip('"').strip("'").strip()
    return s if len(s) >= 2 else None


def _group_from_tag(tag_s: str) -> int | None:
    """Parse ``(GGGG,…)`` → int, or None on failure."""
    m = re.match(r"^\(\s*([0-9A-Fa-f]{4})\s*,", tag_s)
    if m:
        return int(m.group(1), 16)
    return None


# ---------------------------------------------------------------------------
# Harvester
# ---------------------------------------------------------------------------

class CanonHarvester(Harvester):
    """Harvest private DICOM tags from Canon Medical conformance PDFs."""

    vendor = "canon"

    def harvest(self) -> Iterator[RawTag]:
        with pdfplumber.open(self.pdf_path) as pdf:
            pages = list(pdf.pages)
            creator_map = self._collect_creators(pages)
            for page_idx, page in enumerate(pages):
                page_num = page_idx + 1
                text = page.extract_text() or ""
                if not _FILTER_RE.search(text):
                    continue
                seen_tags: set[str] = set()
                # Table extraction is expensive — only call it when the page
                # text contains an actual ``(GGGG,xxEE)``-form tag token.
                if _TABLE_FILTER_RE.search(text):
                    for table_idx, table in enumerate(page.extract_tables()):
                        if not table or len(table) < 2:
                            continue
                        for raw in self._parse_table(
                            table, table_idx, page_num, creator_map
                        ):
                            key = (raw.tag_str, raw.name)
                            if key not in seen_tags:
                                seen_tags.add(key)
                                yield raw
                for raw in self._parse_plain_text(text, page_num, creator_map):
                    key = (raw.tag_str, raw.name)
                    if key not in seen_tags:
                        seen_tags.add(key)
                        yield raw

    # ------------------------------------------------------------------
    # Pass 1 — collect creator strings from the entire PDF
    # ------------------------------------------------------------------

    def _collect_creators(self, pages: list) -> dict[int, str]:
        """Return {group_int: creator_string} from Format-C tables."""
        creator_map: dict[int, str] = {}
        for page in pages:
            text = page.extract_text() or ""
            if not _TABLE_FILTER_RE.search(text):
                continue
            if "creator" not in text.lower() and "private" not in text.lower():
                continue
            for table in page.extract_tables():
                if not table or len(table) < 2:
                    continue
                header = [_cell(c).lower() for c in table[0]]
                if "tag" not in header:
                    continue
                tag_col = header.index("tag")
                if tag_col == 0:
                    continue  # Format B: no creator string available
                # Format C: creator string may be in "value" column
                val_col: int | None = next(
                    (i for i, h in enumerate(header) if h == "value"), None
                )
                if val_col is None:
                    continue
                for row in table[1:]:
                    if not row or len(row) <= tag_col:
                        continue
                    tag_s = _cell(row[tag_col])
                    name_s = _cell(row[0])
                    if "00xx" in tag_s.lower() and "creator" in name_s.lower():
                        raw_creator = _cell(row[val_col]) if val_col < len(row) else ""
                        creator = _strip_creator_quotes(raw_creator) if raw_creator else None
                        if creator:
                            g = _group_from_tag(tag_s)
                            if g is not None:
                                creator_map[g] = creator
        return creator_map

    # ------------------------------------------------------------------
    # Pass 2 — emit RawTag entries
    # ------------------------------------------------------------------

    def _parse_table(
        self,
        table: list,
        table_idx: int,
        page_num: int,
        creator_map: dict[int, str],
    ) -> Iterator[RawTag]:
        header = [_cell(c).lower() for c in table[0]]
        if "tag" not in header:
            return
        tag_col = header.index("tag")
        if tag_col == 0:
            yield from self._parse_fmt_b(table, header, table_idx, page_num, creator_map)
        else:
            yield from self._parse_fmt_c(table, header, tag_col, table_idx, page_num, creator_map)

    def _parse_fmt_b(
        self,
        table: list,
        header: list[str],
        table_idx: int,
        page_num: int,
        creator_map: dict[int, str],
    ) -> Iterator[RawTag]:
        """Format B: [Tag | Attribute Name | VR | VM] — no creator string."""
        # Locate columns
        name_col: int = next(
            (i for i, h in enumerate(header) if "attribute" in h or h == "name"), 1
        )
        vr_col: int | None = next((i for i, h in enumerate(header) if h == "vr"), None)
        vm_col: int | None = next((i for i, h in enumerate(header) if h == "vm"), None)

        for row in table[1:]:
            if not row:
                continue
            tag_s = _cell(row[0])
            if not tag_s.startswith("("):
                continue
            if "00xx" in tag_s.lower():
                continue  # creator-reservation row: no creator string to extract
            if "xx" not in tag_s.lower():
                continue  # concrete / public tag
            name_s = _cell(row[name_col]) if name_col < len(row) else ""
            if not name_s or name_s.lower() in ("attribute name", "key attribute"):
                continue
            vr_s = _cell(row[vr_col]) if vr_col is not None and vr_col < len(row) else None
            vm_s = _cell(row[vm_col]) if vm_col is not None and vm_col < len(row) else None
            g = _group_from_tag(tag_s)
            creator = creator_map.get(g) if g is not None else None
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page_num,
                source_table=table_idx,
                tag_str=tag_s,
                name=name_s,
                vr=vr_s or None,
                vm=vm_s or None,
                private_creator=creator,
            )

    def _parse_fmt_c(
        self,
        table: list,
        header: list[str],
        tag_col: int,
        table_idx: int,
        page_num: int,
        creator_map: dict[int, str],
    ) -> Iterator[RawTag]:
        """Format C: [Name | Tag | VR | Value | …] — creator from pass-1 map."""
        vr_col: int | None = next((i for i, h in enumerate(header) if h == "vr"), None)
        for row in table[1:]:
            if not row or len(row) <= tag_col:
                continue
            tag_s = _cell(row[tag_col])
            if not tag_s.startswith("("):
                continue
            if "00xx" in tag_s.lower():
                continue  # creator-reservation row
            if "xx" not in tag_s.lower():
                continue  # public tag
            name_s = _cell(row[0])
            # Strip sequence-depth markers (">", ">>", …)
            name_s = name_s.lstrip("> ").strip()
            # Skip macro-include lines and empty names
            if not name_s or name_s.lower().startswith("include"):
                continue
            vr_s = _cell(row[vr_col]) if vr_col is not None and vr_col < len(row) else None
            g = _group_from_tag(tag_s)
            creator = creator_map.get(g) if g is not None else None
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page_num,
                source_table=table_idx,
                tag_str=tag_s,
                name=name_s,
                vr=vr_s or None,
                vm=None,
                private_creator=creator,
            )

    def _parse_plain_text(
        self,
        text: str,
        page_num: int,
        creator_map: dict[int, str],
    ) -> Iterator[RawTag]:
        """Format A: ``(GGGG,xxEE)[*]  Name  VR  VM`` plain text lines."""
        for line in text.split("\n"):
            line = line.strip()
            m = _PLAIN_LINE_RE.match(line)
            if not m:
                continue
            group_hex = m.group(1)
            elem_hex = m.group(2)
            if elem_hex.lower() == "00xx":
                continue  # creator-reservation declaration
            tag_s = f"({group_hex},{elem_hex})"
            name_s = m.group(3).strip().rstrip("*").strip()
            vr_s = m.group(4)
            vm_s = m.group(5)
            g = int(group_hex, 16)
            creator = creator_map.get(g)
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page_num,
                source_table=None,
                tag_str=tag_s,
                name=name_s,
                vr=vr_s,
                vm=vm_s,
                private_creator=creator,
            )
