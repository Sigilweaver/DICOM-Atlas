"""Siemens DICOM conformance harvester.

Handles three layout variants found across Siemens conformance PDFs:

Format A  (older products, e.g. FLUOROSPOT):
    Header: Tag | Private Owner Code | Name | VR | VM
    Tag cell: (GGGG,xxEE)  — 'xx' is block-offset placeholder
    Creator: separate column

Format B  (newer products, e.g. MAGNETOM XA/MR VA12S):
    Header: DICOM Tag | Name | VR | VM
    Tag cell: (GGGG, CREATOR NAME, EE)  — creator embedded
    Creator: extracted from tag cell

Format C  (text-only / class-B pages):
    No table extracted by pdfplumber; raw page text contains lines like:
        (0021, SIEMENS MR SDS 01, 09) PATModeText LO 1
    Parser: regex over each text line.
"""

from __future__ import annotations

import re
from collections.abc import Iterator
from pathlib import Path

import pdfplumber

from scraper.harvest.base import Harvester, squeeze_row
from scraper.models import RawTag, VR_CODES

# ── Tag regexes ───────────────────────────────────────────────────────────────

# Format A: (GGGG,xxEE)  or  (GGGG,EEEE)
TAG_A_RE = re.compile(
    r"^\(\s*([0-9A-Fa-f]{4})\s*,\s*([0-9A-Fa-fxX]{4})\s*\)$"
)

# Format B/C: (GGGG, CREATOR STRING, HH)  — hex element is 1-2 digits
TAG_BC_RE = re.compile(
    r"\(\s*([0-9A-Fa-f]{4})\s*,\s*([^,()]+?)\s*,\s*([0-9A-Fa-f]{1,2})\s*\)"
)

# ── Valid VR codes (as pipe-separated string for regex) ───────────────────────
_VR_ALTS = "|".join(sorted(VR_CODES, key=len, reverse=True))
_VR_RE = re.compile(rf"\b({_VR_ALTS})\b")

# ── Text-line regex (Format C) ────────────────────────────────────────────────
# Matches: (GGGG, CREATOR, EE) Name  VR  VM
_TEXT_LINE_RE = re.compile(
    r"\(\s*([0-9A-Fa-f]{4})\s*,\s*([^,()]+?)\s*,\s*([0-9A-Fa-f]{1,2})\s*\)"  # tag
    r"\s+"
    r"(.+?)"  # name (lazy)
    r"\s+"
    rf"({_VR_ALTS})"  # VR
    r"\s+"
    r"([\dna\-]+)"  # VM
    r"\s*$",
    re.MULTILINE,
)

# ── Tag regexes for Format D (stateful creator tracking) ─────────────────────

# (GGGG,00xx) — creator declaration row
_TAG_CREATOR_DECL_RE = re.compile(
    r"^\(\s*([0-9A-Fa-f]{4})\s*,\s*00[xX]{2}\s*\)$"
)
# (GGGG,xxEE) — private attribute row (xx literal or XX)
_TAG_PRIVATE_ATTR_RE = re.compile(
    r"^\(\s*([0-9A-Fa-f]{4})\s*,\s*[xX]{2}([0-9A-Fa-f]{2})\s*\)$"
)

# ── Header detection ──────────────────────────────────────────────────────────
_FORMAT_A_MUST = {"tag", "private owner code", "name", "vr"}
_FORMAT_B_MUST = {"dicom tag", "name", "vr"}
# Format D: CT-style IOD tables  (Attribute Name | Tag | VR | VM | ... | Value ...)
_FORMAT_D_MUST = {"attribute name", "tag", "vr", "vm"}


def _detect_format(cells: list[str]) -> str | None:
    lowered = {c.lower() for c in cells}
    if _FORMAT_A_MUST.issubset(lowered):
        return "A"
    if _FORMAT_B_MUST.issubset(lowered):
        return "B"
    if _FORMAT_D_MUST.issubset(lowered):
        return "D"
    return None


def _col(cells: list[str], name: str) -> int | None:
    for i, c in enumerate(cells):
        if c.lower() == name.lower():
            return i
    return None


def _at(cells: list[str], i: int | None) -> str | None:
    if i is None or i >= len(cells):
        return None
    return cells[i].strip() or None


# ── Harvester ─────────────────────────────────────────────────────────────────

class SiemensHarvester(Harvester):
    vendor = "siemens"

    def harvest(self) -> Iterator[RawTag]:
        with pdfplumber.open(self.pdf_path) as pdf:
            for page_num, page in enumerate(pdf.pages, start=1):
                try:
                    tables = page.extract_tables() or []
                except Exception:
                    tables = []

                table_tags: list[RawTag] = []
                for tbl_idx, table in enumerate(tables):
                    table_tags.extend(self._parse_table(table, page_num, tbl_idx))

                if table_tags:
                    yield from table_tags
                else:
                    # Text fallback for pages where table extraction misses content
                    try:
                        text = page.extract_text() or ""
                    except Exception:
                        text = ""
                    yield from self._parse_text(text, page_num)

                # Free cached layout objects per page — pdfplumber retains a lot
                # of intermediate state per page, which otherwise accumulates
                # across hundred-page PDFs.
                try:
                    page.flush_cache()
                    page.close()
                except Exception:
                    pass

    # ── Format A / B (table-based) ────────────────────────────────────────────

    def _parse_table(
        self, table: list[list[str | None]], page: int, tbl_idx: int
    ) -> Iterator[RawTag]:
        if not table:
            return
        header_cells = squeeze_row(table[0])
        fmt = _detect_format(header_cells)
        if fmt is None:
            return

        if fmt == "A":
            yield from self._parse_format_a(table, header_cells, page, tbl_idx)
        elif fmt == "B":
            yield from self._parse_format_b(table, header_cells, page, tbl_idx)
        else:
            yield from self._parse_format_d(table, header_cells, page, tbl_idx)

    def _parse_format_a(
        self,
        table: list[list[str | None]],
        header: list[str],
        page: int,
        tbl_idx: int,
    ) -> Iterator[RawTag]:
        idx_tag = _col(header, "tag")
        idx_creator = _col(header, "private owner code")
        idx_name = _col(header, "name")
        idx_vr = _col(header, "vr")
        idx_vm = _col(header, "vm")
        idx_desc = _col(header, "description")

        for row in table[1:]:
            cells = squeeze_row(row)
            if not cells or idx_tag is None or idx_tag >= len(cells):
                continue
            tag_str = cells[idx_tag]
            if not TAG_A_RE.match(tag_str):
                continue
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=tbl_idx,
                tag_str=tag_str,
                private_creator=_at(cells, idx_creator),
                name=_at(cells, idx_name),
                vr=_at(cells, idx_vr),
                vm=_at(cells, idx_vm),
                description=_at(cells, idx_desc),
            )

    def _parse_format_b(
        self,
        table: list[list[str | None]],
        header: list[str],
        page: int,
        tbl_idx: int,
    ) -> Iterator[RawTag]:
        """Format B: tag cell contains (GGGG, CREATOR, EE)."""
        # header is "DICOM Tag | Name | VR | VM"
        _idx = _col(header, "dicom tag")
        idx_tag = _idx if _idx is not None else _col(header, "tag")
        idx_name = _col(header, "name")
        idx_vr = _col(header, "vr")
        idx_vm = _col(header, "vm")

        for row in table[1:]:
            cells = squeeze_row(row)
            if not cells or idx_tag is None or idx_tag >= len(cells):
                continue
            tag_str = cells[idx_tag]
            m = TAG_BC_RE.match(tag_str)
            if not m:
                continue
            group_hex, creator, elem_hex = m.group(1), m.group(2).strip(), m.group(3)
            # Reconstruct a synthetic tag_str in the normalizer-friendly form:
            # Use (GGGG,xxEE) so the normalizer correctly sets is_block_offset=True.
            synthetic = f"({group_hex},xx{elem_hex.upper().zfill(2)})"
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=tbl_idx,
                tag_str=synthetic,
                private_creator=creator,
                name=_at(cells, idx_name),
                vr=_at(cells, idx_vr),
                vm=_at(cells, idx_vm),
            )

    def _parse_format_d(
        self,
        table: list[list[str | None]],
        header: list[str],
        page: int,
        tbl_idx: int,
    ) -> Iterator[RawTag]:
        """Format D: CT/XA IOD tables with stateful creator tracking.

        Header: Attribute Name | Tag | VR | VM | ... | Value | ...
        Creator row:  'Private Creator' | (GGGG,00xx) | LO | 1 | ... | CREATOR_STRING
        Private rows: Name              | (GGGG,xxEE) | VR | VM | ...
        """
        # Locate required columns
        header_lower = [c.lower() for c in header]
        try:
            idx_name = header_lower.index("attribute name")
            idx_tag  = header_lower.index("tag")
            idx_vr   = header_lower.index("vr")
            idx_vm   = header_lower.index("vm")
        except ValueError:
            return

        # "Value" column holds the creator string in the creator declaration row
        idx_value = next(
            (i for i, h in enumerate(header_lower) if h == "value"), None
        )

        # group_hex → creator string (updated row-by-row)
        creator_map: dict[str, str] = {}

        for row in table[1:]:
            cells = squeeze_row(row)
            if len(cells) <= idx_tag:
                continue
            tag_str = cells[idx_tag].strip()

            # Creator declaration row: (GGGG,00xx)
            m_decl = _TAG_CREATOR_DECL_RE.match(tag_str)
            if m_decl:
                group_hex = m_decl.group(1).upper()
                # Creator string is in the "Value" column if present
                creator = None
                if idx_value is not None and idx_value < len(cells):
                    raw_val = cells[idx_value].strip().replace("\n", " ")
                    raw_val = re.sub(r"\s+", " ", raw_val)
                    if raw_val:
                        creator = raw_val
                # Fallback: last non-empty cell that looks like a creator
                if not creator:
                    for c in reversed(cells):
                        v = c.strip().replace("\n", " ")
                        if v and v not in {"LO", "1", "SAFE", "GENERATED",
                                           "ALWAYS", "CONDITIONAL", "ALWAYS"}:
                            creator = v
                            break
                if creator:
                    creator_map[group_hex] = creator
                continue

            # Private attribute row: (GGGG,xxEE)
            m_attr = _TAG_PRIVATE_ATTR_RE.match(tag_str)
            if not m_attr:
                continue
            group_hex = m_attr.group(1).upper()
            elem_hex  = m_attr.group(2).upper()
            creator = creator_map.get(group_hex)
            if not creator:
                continue  # unknown creator — skip
            name = _at(cells, idx_name)
            if not name:
                continue
            synthetic = f"({group_hex},xx{elem_hex})"
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=tbl_idx,
                tag_str=synthetic,
                private_creator=creator,
                name=name,
                vr=_at(cells, idx_vr),
                vm=_at(cells, idx_vm),
            )

    # ── Format C (text fallback) ──────────────────────────────────────────────

    def _parse_text(self, text: str, page: int) -> Iterator[RawTag]:
        for m in _TEXT_LINE_RE.finditer(text):
            group_hex = m.group(1)
            creator = m.group(2).strip()
            elem_hex = m.group(3)
            name = m.group(4).strip()
            vr = m.group(5)
            vm = m.group(6)
            synthetic = f"({group_hex},xx{elem_hex.upper().zfill(2)})"
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=-1,
                tag_str=synthetic,
                private_creator=creator,
                name=name,
                vr=vr,
                vm=vm,
            )
