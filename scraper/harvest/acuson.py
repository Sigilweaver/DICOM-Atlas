"""Acuson (Siemens Healthineers ultrasound) DICOM conformance harvester.

Acuson conformance statements (Sequoia, Juniper, NX-series, Bonsai, Maple,
Origin, Freestyle, Redwood, etc.) document private tags using two main
table layouts:

Variant Sequoia — six logical columns:
    Attribute | Attribute | Tag | VR | VM | Value
Tags are concrete (e.g. ``(0029, 1041)``). Creator declared in the row
where ``Attribute == "Private Creator"`` with the creator string in the
``Value`` cell.

Variant Juniper / NX — four logical columns:
    Module | Attribute | Tag | Notes
Tags are concrete. Creator declared in the row where ``Attribute ==
"Private Creator"`` with the creator string in the ``Notes`` cell. VR/VM
are absent from the table; they are inferred or left blank.

Both formats include a ``Private Creator`` declaration row at
``(GGGG,00BB)`` in the DICOM block-reservation range (element high byte
== 0x00, low byte 0x10..0xFF). Data rows use a concrete ``(GGGG,BBEE)``
tag where ``BB`` is the block number; the harvester rewrites these to
the synthetic ``(GGGG,xxEE)`` form expected downstream.

Creator state persists across page boundaries within one PDF.
"""

from __future__ import annotations

import re
from collections.abc import Iterator

import pdfplumber

from scraper.harvest.base import Harvester, squeeze_row
from scraper.models import RawTag

# Concrete DICOM tag, e.g. "(0019,1041)" or "(0029, 1040)".
_TAG_CONCRETE_RE = re.compile(
    r"^\(\s*([0-9A-Fa-f]{4})\s*,\s*([0-9A-Fa-f]{4})\s*\)$"
)

# Cheap pre-filter: any odd-group concrete tag on the page. Only pages
# matching this run the (expensive) extract_tables() pass.
_PRIVATE_TAG_TEXT_RE = re.compile(
    r"\(\s*([0-9A-Fa-f]{4})\s*,\s*[0-9A-Fa-f]{4}\s*\)"
)

# Sequoia-family header: must contain Attribute + Tag + VR + VM.
_HEADER_MUST_SEQUOIA = {"attribute", "tag", "vr", "vm"}

# Juniper/NX-family header: must contain Module + Attribute + Tag.
# (No VR/VM columns — VR is inferred downstream or left UN.)
_HEADER_MUST_JUNIPER = {"module", "attribute", "tag"}

# Reservation-summary tables describe block reservations rather than naming
# a real Private Creator. Their "creator" cell is boilerplate, not a name.
_BOILERPLATE_CREATOR_HINTS = (
    "reserves tags",
    "reserved tags",
    "for use as private tags",
)


def _is_boilerplate_creator(s: str) -> bool:
    low = s.lower()
    return any(h in low for h in _BOILERPLATE_CREATOR_HINTS)


def _find_col(header: list[str], name: str) -> int | None:
    """Return first index where the cell equals `name` (case-insensitive)."""
    target = name.lower().strip()
    for i, c in enumerate(header):
        if c.lower().strip() == target:
            return i
    return None


def _find_col_after(header: list[str], name: str, after: int) -> int | None:
    """Return first index strictly greater than `after` matching `name`."""
    target = name.lower().strip()
    for i in range(after + 1, len(header)):
        if header[i].lower().strip() == target:
            return i
    return None


class AcusonHarvester(Harvester):
    vendor = "siemens"  # Acuson tags share the Siemens private dictionary

    def harvest(self) -> Iterator[RawTag]:
        # Persistent creator map keyed by (group_hex_upper, block_byte_int).
        creator_map: dict[tuple[str, int], str] = {}
        seen_keys: set[tuple[str, str]] = set()

        with pdfplumber.open(self.pdf_path) as pdf:
            for page_num, page in enumerate(pdf.pages, start=1):
                # Cheap pre-filter: skip pages with no private (odd-group)
                # concrete tag in the text. extract_text is much faster than
                # extract_tables and avoids loading layout objects we will
                # immediately discard. This is critical for multi-thousand
                # page PDFs (e.g. ACUSON_Sequoia_VB10 = 1043 pages).
                try:
                    text = page.extract_text() or ""
                except Exception:
                    text = ""

                has_private = False
                for m in _PRIVATE_TAG_TEXT_RE.finditer(text):
                    if int(m.group(1), 16) % 2 == 1:
                        has_private = True
                        break

                if has_private:
                    try:
                        tables = page.extract_tables() or []
                    except Exception:
                        tables = []

                    for tbl_idx, table in enumerate(tables):
                        for tag in self._parse_table(
                            table, page_num, tbl_idx, creator_map
                        ):
                            key = (tag.tag_str, tag.private_creator or "")
                            if key in seen_keys:
                                continue
                            seen_keys.add(key)
                            yield tag

                try:
                    page.flush_cache()
                    page.close()
                except Exception:
                    pass

    def _parse_table(
        self,
        table: list[list[str | None]],
        page: int,
        tbl_idx: int,
        creator_map: dict[tuple[str, int], str],
    ) -> Iterator[RawTag]:
        """Tag-anchored row parser.

        Acuson PDFs have inconsistent column counts after squeeze (the
        leading "Module" column is empty on most data rows and gets
        dropped). Instead of indexing by header position, we locate the
        cell that *contains* a concrete DICOM tag and use the cells
        around it: name = first non-empty cell before the tag, value/
        notes = first non-empty cell after the tag. VR/VM, when present,
        sit between tag and notes (Sequoia format).
        """
        if not table or len(table) < 2:
            return

        header = [c.lower().strip() if c else "" for c in squeeze_row(table[0])]
        header_set = set(header)

        # Skip tables that aren't tag tables at all.
        if not (
            _HEADER_MUST_SEQUOIA.issubset(header_set)
            or _HEADER_MUST_JUNIPER.issubset(header_set)
            or "tag" in header_set
        ):
            return

        has_vr = "vr" in header_set
        has_vm = "vm" in header_set

        for row in table[1:]:
            cells = squeeze_row(row)
            if len(cells) < 2:
                continue

            # Find the cell holding the concrete tag.
            tag_idx = None
            tag_match = None
            for i, c in enumerate(cells):
                m = _TAG_CONCRETE_RE.match(c.strip())
                if m:
                    tag_idx = i
                    tag_match = m
                    break
            if tag_idx is None or tag_match is None:
                continue

            group_int = int(tag_match.group(1), 16)
            elem_int = int(tag_match.group(2), 16)
            if group_int % 2 == 0:
                continue

            group_hex = f"{group_int:04X}"
            elem_high = (elem_int >> 8) & 0xFF
            elem_low = elem_int & 0xFF

            # Name is the first non-empty cell before the tag cell.
            name_cell = ""
            for j in range(tag_idx - 1, -1, -1):
                if cells[j].strip():
                    name_cell = cells[j].strip()
                    break
            # Strip leading sequence-nesting markers (">", ">>").
            name_cell = name_cell.lstrip(">").strip()

            # After-tag cells: [VR?, VM?, notes/value...]
            after = cells[tag_idx + 1 :]
            vr_cell = ""
            vm_cell = ""
            notes_cell = ""

            cursor = 0
            if has_vr and cursor < len(after):
                # VR is exactly two letters, all caps, optional case-insensitive.
                cand = after[cursor].strip().upper()
                if len(cand) == 2 and cand.isalpha():
                    vr_cell = cand
                    cursor += 1
            if has_vm and cursor < len(after):
                cand = after[cursor].strip()
                # VM is digits, "n", "1-n", "256", etc.
                if re.match(r"^[\d\-na\.\s]+$", cand):
                    vm_cell = cand
                    cursor += 1
            # Remaining cells join into notes.
            if cursor < len(after):
                notes_cell = " ".join(c.strip() for c in after[cursor:] if c.strip())

            # Creator-declaration row: (GGGG,00BB) where 0x10..0xFF.
            if elem_high == 0x00 and 0x10 <= elem_low <= 0xFF:
                if "private creator" in name_cell.lower():
                    # Sequoia: creator string is in the Value cell (after
                    # VR/VM columns) → notes_cell. Juniper: no VR/VM, so
                    # take the last non-empty post-tag cell.
                    if has_vr:
                        creator_str = notes_cell.strip()
                    else:
                        creator_str = ""
                        for c in reversed(after):
                            c = c.strip()
                            if c:
                                creator_str = c
                                break
                    # Skip placeholders like "SIEMENS <Manufacturer Model
                    # Name>" and reservation boilerplate.
                    if (
                        creator_str
                        and "<" not in creator_str
                        and not _is_boilerplate_creator(creator_str)
                    ):
                        creator_map[(group_hex, elem_low)] = creator_str
                continue

            if elem_high == 0x00:
                continue

            creator = creator_map.get((group_hex, elem_high))
            if not creator or not name_cell:
                continue

            synthetic = f"({group_hex},xx{elem_low:02X})"
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=tbl_idx,
                tag_str=synthetic,
                private_creator=creator,
                name=name_cell,
                vr=vr_cell or None,
                vm=vm_cell or None,
                description=notes_cell or None,
            )
