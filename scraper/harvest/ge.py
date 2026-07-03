"""GE DICOM conformance statement harvester.

GE conformance PDFs (e.g. Brightspeed, Optima, Discovery, Senographe...) document
private tags in several formats:

Variant A - 4-column table:
    Description | Type | Tag | Value
where ``Type == 'P'`` flags a private row. Creator strings are declared in rows
whose tag looks like ``GGGG,00xx`` and whose Value cell holds the creator
string (e.g. ``GEMS_IDEN_01``). VR is absent; we leave it ``None``.

Variant B - name-first text (DL / Discovery / Innova xr-mammo families):
    <attribute name>   (GGGG,xxEE)   VR   VM   <description>
Creator declared in a preceding section heading such as
"5.5.3 Private Group GEMS_XR3DCAL_01".

Variant C - 6-column table (Senographe mammography families, rev 40+):
    Attribute name | Tag | Type | Attribute description | VR | VM
Creator is declared in a "Private Creator" row within the same table whose tag
is (GGGG,00BB) (DICOM block-reservation range) and whose description cell holds
the creator string (e.g. ``GEMS_GDXE_FALCON_04``). Variant-C creator state
persists across page boundaries within one PDF.
"""

from __future__ import annotations

import re
from collections.abc import Iterator

import pdfplumber

from scraper.harvest.base import Harvester, squeeze_row
from scraper.models import RawTag

_CREATOR_DECL_RE = re.compile(
    r"^\s*([0-9A-Fa-f]{4})\s*,\s*00[xX]{2}\s*$"
)
_PRIVATE_ATTR_RE = re.compile(
    r"^\s*([0-9A-Fa-f]{4})\s*,\s*[xX]{2}([0-9A-Fa-f]{2})\s*$"
)
# Text fallback line: "Name (GGGG,xxEE) CREATOR_STRING rest-of-description"
# The creator token is ALL_CAPS with underscores/digits (e.g. QUASAR_INTERNAL_USE,
# GEMS_ACQU_01). The name comes before the tag.
_TEXT_LINE_RE = re.compile(
    r"^(?P<name>.+?)\s+"
    r"\(\s*(?P<group>[0-9A-Fa-f]{4})\s*,\s*[xX]{2}(?P<elem>[0-9A-Fa-f]{2})\s*\)\s+"
    r"(?P<creator>[A-Z][A-Z0-9_]{2,})"
    r"(?:\s+(?P<desc>.*))?$"
)

# Variant B (DL/Discovery/Innova xr-mammo family): name-first rows with explicit
# VR + VM columns, creator declared in a section heading above the table:
#     "5.5.3 Private Group GEMS_XR3DCAL_01"
#     "Table 42 Private Group GEMS_XR3DCAL_01"
#     "<name>   (GGGG,xxEE)   VR   VM   <description>"
_VR_ALTERNATION = (
    r"AE|AS|AT|CS|DA|DS|DT|FL|FD|IS|LO|LT|OB|OD|OF|OL|OV|OW|PN|SH|SL|SQ|SS|ST"
    r"|SV|TM|UC|UI|UL|UN|UR|US|UT|UV"
)
_TEXT_NAME_TAG_RE = re.compile(
    r"^(?P<name>\S.{1,100}?)\s+"
    r"\(\s*(?P<group>[0-9A-Fa-f]{4})\s*,\s*[a-zA-Z]{2}(?P<elem>[0-9A-Fa-f]{2})\s*\)\s+"
    rf"(?P<vr>{_VR_ALTERNATION})\s+"
    r"(?P<vm>\d+(?:[--](?:\d+|[nNN]))?)"
    r"(?:\s+(?P<desc>.*))?$"
)
_CREATOR_SECTION_RE = re.compile(
    r"Private\s+Group\s+(?P<creator>[A-Z][A-Z0-9_]{2,})(?:\b|\s)",
)

_HEADER_MUST = {"description", "type", "tag", "value"}

# Variant C: 6-column table header detection.
_HEADER_MUST_C = {"attribute name", "tag", "type", "attribute description", "vr", "vm"}

# Matches a concrete DICOM tag like "(0011,1003)" or "(7FDF,0010)".
_TAG_CONCRETE_RE = re.compile(
    r"^\(\s*([0-9A-Fa-f]{4})\s*,\s*([0-9A-Fa-f]{4})\s*\)$"
)


def _col(header: list[str], name: str) -> int | None:
    for i, c in enumerate(header):
        if c.lower().strip() == name:
            return i
    return None


class GEHarvester(Harvester):
    vendor = "ge"

    def harvest(self) -> Iterator[RawTag]:
        # creator map: group_hex (upper) -> creator string, reset per-table
        # to avoid cross-table leakage when two tables describe different groups.
        with pdfplumber.open(self.pdf_path) as pdf:
            # Harvester-wide creator map for variant-B section-titled tables
            # that span many pages.
            section_creator: str | None = None
            seen_keys: set[tuple[str, str]] = set()

            # Variant-C creator map: persists across pages within one PDF.
            # Keyed (group_hex_upper, block_byte_int) -> creator_str.
            c_creator_map: dict[tuple[str, int], str] = {}

            for page_num, page in enumerate(pdf.pages, start=1):
                try:
                    tables = page.extract_tables() or []
                except Exception:
                    tables = []

                emitted = False
                for tbl_idx, table in enumerate(tables):
                    for tag in self._parse_table(table, page_num, tbl_idx):
                        seen_keys.add((tag.tag_str, tag.private_creator or ""))
                        emitted = True
                        yield tag

                    for tag in self._parse_table_variant_c(
                        table, page_num, tbl_idx, c_creator_map
                    ):
                        key = (tag.tag_str, tag.private_creator or "")
                        if key in seen_keys:
                            continue
                        seen_keys.add(key)
                        emitted = True
                        yield tag

                try:
                    text = page.extract_text() or ""
                except Exception:
                    text = ""

                # Track latest section-title creator declaration.
                for m in _CREATOR_SECTION_RE.finditer(text):
                    section_creator = m.group("creator")

                # Variant B (name-first with VR/VM) always runs - these tables
                # are usually missed by pdfplumber.extract_tables.
                for tag in self._parse_text_variant_b(
                    text, page_num, section_creator
                ):
                    key = (tag.tag_str, tag.private_creator or "")
                    if key in seen_keys:
                        continue
                    seen_keys.add(key)
                    emitted = True
                    yield tag

                # Only run the original creator-inline text fallback when
                # neither the table nor variant-B produced anything on
                # this page.
                if not emitted:
                    yield from self._parse_text(text, page_num)

                try:
                    page.flush_cache()
                    page.close()
                except Exception:
                    pass

    def _parse_table(
        self, table: list[list[str | None]], page: int, tbl_idx: int
    ) -> Iterator[RawTag]:
        if not table or len(table) < 2:
            return
        header = [c.lower().strip() if c else "" for c in squeeze_row(table[0])]
        header_set = set(header)
        if not _HEADER_MUST.issubset(header_set):
            return

        idx_desc = _col(header, "description")
        idx_type = _col(header, "type")
        idx_tag = _col(header, "tag")
        idx_value = _col(header, "value")
        if None in (idx_desc, idx_type, idx_tag, idx_value):
            return

        creator_map: dict[str, str] = {}

        for row in table[1:]:
            cells = squeeze_row(row)
            if not cells or idx_tag >= len(cells):
                continue
            type_cell = cells[idx_type] if idx_type < len(cells) else ""
            if type_cell.strip().upper() != "P":
                continue
            tag_cell = cells[idx_tag].strip()

            m_decl = _CREATOR_DECL_RE.match(tag_cell)
            if m_decl:
                group = m_decl.group(1).upper()
                creator = (
                    cells[idx_value].strip()
                    if idx_value < len(cells) and cells[idx_value]
                    else ""
                )
                if creator:
                    creator_map[group] = creator
                continue

            m_attr = _PRIVATE_ATTR_RE.match(tag_cell)
            if not m_attr:
                continue
            group = m_attr.group(1).upper()
            elem_lo = m_attr.group(2).upper()
            synthetic = f"({group},xx{elem_lo})"
            name = cells[idx_desc].strip() if idx_desc < len(cells) else ""
            value = (
                cells[idx_value].strip()
                if idx_value < len(cells) and cells[idx_value]
                else ""
            )
            if not name:
                continue
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=tbl_idx,
                tag_str=synthetic,
                private_creator=creator_map.get(group),
                name=name,
                vr=None,
                vm=None,
                description=value,
            )

    def _parse_table_variant_c(
        self,
        table: list[list[str | None]],
        page: int,
        tbl_idx: int,
        creator_map: dict[tuple[str, int], str],
    ) -> Iterator[RawTag]:
        """Six-column private-tag table: Attribute name|Tag|Type|Attribute description|VR|VM.

        Seen in Senographe mammography conformance PDFs (rev 40+). The creator
        map is shared across all calls within one PDF harvest so that creator
        declarations from earlier pages are visible to later continuation tables.

        Creator rows are identified by Name == 'Private Creator' with a
        concrete tag (GGGG,00BB) in the DICOM block-reservation range
        (element high byte == 0x00, low byte in 0x10..0xFF). The creator
        string lives in the Attribute description cell.

        Private data rows have a concrete tag (GGGG,BBEE) where BB != 0x00;
        we map them to the synthetic (GGGG,xxEE) form using the block-number
        (BB) to look up the creator declared for that group+block.
        """
        if not table or len(table) < 2:
            return
        header = [c.lower().strip() if c else "" for c in squeeze_row(table[0])]
        if not _HEADER_MUST_C.issubset(set(header)):
            return

        try:
            idx_name = header.index("attribute name")
            idx_tag  = header.index("tag")
            idx_desc = header.index("attribute description")
            idx_vr   = header.index("vr")
            idx_vm   = header.index("vm")
        except ValueError:
            return

        for row in table[1:]:
            cells = squeeze_row(row)
            if not cells:
                continue
            if idx_tag >= len(cells):
                continue

            tag_cell = (cells[idx_tag] or "").strip()
            m = _TAG_CONCRETE_RE.match(tag_cell)
            if not m:
                continue

            group_int = int(m.group(1), 16)
            elem_int  = int(m.group(2), 16)

            # Only process private (odd) groups.
            if group_int % 2 == 0:
                continue

            group_hex = f"{group_int:04X}"
            elem_high = (elem_int >> 8) & 0xFF
            elem_low  = elem_int & 0xFF

            # Creator-declaration row: element is in DICOM block-reservation
            # range (high byte == 0x00, low byte 0x10-0xFF).
            if elem_high == 0x00 and 0x10 <= elem_low <= 0xFF:
                name_cell = (cells[idx_name] if idx_name < len(cells) else "").strip()
                if name_cell.lower() == "private creator":
                    desc_cell = (cells[idx_desc] if idx_desc < len(cells) else "").strip()
                    if desc_cell:
                        creator_map[(group_hex, elem_low)] = desc_cell
                # Whether or not it's a recognized creator row, skip emitting.
                continue

            # Skip elements still in the creator-reservation range (non-standard
            # vendor usage; we can't reliably assign a block).
            if elem_high == 0x00:
                continue

            # Look up creator for this group+block.
            creator = creator_map.get((group_hex, elem_high))
            if not creator:
                continue

            name_cell = (cells[idx_name] if idx_name < len(cells) else "").strip()
            if not name_cell:
                continue

            vr_cell  = (cells[idx_vr]  if idx_vr  < len(cells) else "").strip().upper()
            vm_cell  = (cells[idx_vm]  if idx_vm  < len(cells) else "").strip()
            desc_cell = (cells[idx_desc] if idx_desc < len(cells) else "").strip()

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
                description=desc_cell or None,
            )

    def _parse_text_variant_b(
        self, text: str, page: int, section_creator: str | None
    ) -> Iterator[RawTag]:
        """Name-first text with explicit VR/VM columns; creator from section heading.

        Seen in many ``ge_xr-mammo_*.pdf`` (Discovery/Innova IGS DL families).
        Each row looks like::

            <attribute name>   (GGGG,xxEE)   VR   VM   <description>

        and the owner string is declared above the block in a section title
        such as ``5.5.3 Private Group GEMS_XR3DCAL_01``.
        """
        if not section_creator:
            return
        for line in text.split("\n"):
            m = _TEXT_NAME_TAG_RE.match(line.strip())
            if not m:
                continue
            name = m.group("name").strip(" \t|:•·")
            # Reject obvious prose that happens to end with a tag reference.
            if len(name) < 2 or len(name) > 120:
                continue
            group = m.group("group").upper()
            elem_lo = m.group("elem").upper()
            synthetic = f"({group},xx{elem_lo})"
            vr = m.group("vr").upper()
            vm = m.group("vm").strip().replace("-", "-")
            desc = (m.group("desc") or "").strip() or None
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=None,
                tag_str=synthetic,
                private_creator=section_creator,
                name=name,
                vr=vr,
                vm=vm,
                description=desc,
            )

    def _parse_text(self, text: str, page: int) -> Iterator[RawTag]:
        """Text fallback: ``Name (GGGG,xxEE) CREATOR description``.

        Seen in GE NM, MR and some interoperability manuals where the tag
        appendix is laid out as running text rather than a table.
        """
        for line in text.split("\n"):
            m = _TEXT_LINE_RE.match(line.strip())
            if not m:
                continue
            name = m.group("name").strip(" \t|:•·")
            if len(name) < 2 or len(name) > 120:
                continue
            group = m.group("group").upper()
            elem_lo = m.group("elem").upper()
            creator = m.group("creator").strip()
            desc = (m.group("desc") or "").strip()
            synthetic = f"({group},xx{elem_lo})"
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=None,
                tag_str=synthetic,
                private_creator=creator,
                name=name,
                vr=None,
                vm=None,
                description=desc or None,
            )
