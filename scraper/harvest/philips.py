"""Philips DICOM conformance statement harvester.

Philips conformance PDFs use several related private-tag table layouts:

A. ``Attribute Name | Tag | Type | Attribute Description``
B. ``Key Attribute   | Tag | Directory Record Type | Type | Notes``
C. ``Key Attribute   | Tag | Module | Type | Notes``

Tag cells are already written as ``(GGGG,xxEE)``. VR and private-creator
strings do not appear in the tables; they are mentioned in prose. We emit
``vr=None`` (normalizer defaults to ``UN``) and ``private_creator=None``.

Continuation tables (same columns repeated on a later page without a header
row) are common — we keep the last matched header on the harvester instance
and treat headerless tag-heavy tables as continuations.

A second format seen in multi-vendor conformance statements (e.g. the Cloud
Solutions / DICOM Gateway appendix) writes the dictionary as running text:

    (GGGG,xxEE)  <creator string>  VR  VM  <description>

This is parsed as a text fallback when table extraction yields no rows on
the page. The creator is any non-VR run of text; VR is restricted to the
DICOM standard 2-letter set.
"""

from __future__ import annotations

import re
from collections.abc import Iterator

import pdfplumber

from scraper.harvest.base import Harvester, squeeze_row
from scraper.models import RawTag

_TAG_RE = re.compile(
    r"^\(\s*([0-9A-Fa-f]{4})\s*,\s*[a-zA-Z]{2}([0-9A-Fa-f]{2})\s*\)$"
)

# In-table creator-declaration tag, e.g. "(2003,00XX)" or "(0009,00xx)"
_CREATOR_TAG_RE = re.compile(
    r"^\(\s*([0-9A-Fa-f]{4})\s*,\s*00[a-zA-Z]{2}\s*\)$"
)

_NAME_HEADERS = ("attribute name", "key attribute")
_DESC_HEADERS = ("attribute description", "notes", "description", "value", "comments")

# Valid DICOM VR codes (PS3.5 6.2).
_VR_ALTERNATION = (
    "AE|AS|AT|CS|DA|DS|DT|FL|FD|IS|LO|LT|OB|OD|OF|OL|OW|PN|"
    "SH|SL|SQ|SS|ST|TM|UC|UI|UL|UN|UR|US|UT"
)
# Running-text line, variant A: tag-first
#     (GGGG,xxEE)   <creator>   VR   VM   description
_TEXT_LINE_RE = re.compile(
    r"^\s*\(\s*(?P<group>[0-9A-Fa-f]{4})\s*,\s*[a-zA-Z]{2}(?P<elem>[0-9A-Fa-f]{2})\s*\)\s+"
    r"(?P<creator>.+?)\s+"
    rf"(?P<vr>{_VR_ALTERNATION})\s+"
    r"(?P<vm>\d+(?:-(?:\d+|[nN]))?)"
    r"(?:\s+(?P<desc>.*))?$"
)

# Running-text line, variant B: name-first (Forcare-style)
#     <name>   (GGGG,xxEE)   VR   VM   description
# Creator is established separately (prose sentence preceding the dictionary).
_NAME_TAG_LINE_RE = re.compile(
    r"^\s*(?P<name>\S.{1,100}?)\s+"
    r"\(\s*(?P<group>[0-9A-Fa-f]{4})\s*,\s*[a-zA-Z]{2}(?P<elem>[0-9A-Fa-f]{2})\s*\)\s+"
    rf"(?P<vr>{_VR_ALTERNATION})\s+"
    r"(?P<vm>\d+(?:-(?:\d+|[nN]))?)"
    r"(?:\s+(?P<desc>.*))?$"
)


def _col(header: list[str], needle: str) -> int | None:
    for i, c in enumerate(header):
        if c.lower().strip() == needle:
            return i
    return None


def _col_any(header: list[str], needles: tuple[str, ...]) -> int | None:
    for n in needles:
        i = _col(header, n)
        if i is not None:
            return i
    return None


class PhilipsHarvester(Harvester):
    vendor = "philips"

    def __init__(self, pdf_path):  # type: ignore[no-untyped-def]
        super().__init__(pdf_path)
        # Sticky column indices set by the most recent recognised header.
        self._last_idx_name: int | None = None
        self._last_idx_tag: int | None = None
        self._last_idx_desc: int | None = None
        self._last_idx_vr: int | None = None
        self._last_idx_vm: int | None = None
        self._last_use_raw: bool = False
        # Sticky (group-hex -> creator) map populated by in-table creator rows
        # "(GGGG,00XX)" and reused by later tables/pages for the same group.
        self._creator_map: dict[str, str] = {}

    def harvest(self) -> Iterator[RawTag]:
        with pdfplumber.open(self.pdf_path) as pdf:
            for page_num, page in enumerate(pdf.pages, start=1):
                try:
                    tables = page.extract_tables() or []
                except Exception:
                    tables = []

                seen_tags: set[tuple[str, str]] = set()
                for tbl_idx, table in enumerate(tables):
                    for tag in self._parse_table(table, page_num, tbl_idx):
                        seen_tags.add((tag.tag_str, tag.private_creator or ""))
                        yield tag

                try:
                    text = page.extract_text() or ""
                except Exception:
                    text = ""
                # Scan text for running-text dictionary lines. Always run this
                # in addition to table extraction so Forcare-style one-row
                # dictionaries in-prose are not missed. We de-dupe against
                # anything already emitted by table extraction on this page.
                for tag in self._parse_text(text, page_num):
                    key = (tag.tag_str, tag.private_creator or "")
                    if key in seen_tags:
                        continue
                    seen_tags.add(key)
                    yield tag

                try:
                    page.flush_cache()
                    page.close()
                except Exception:
                    pass

    def _parse_table(
        self, table: list[list[str | None]], page: int, tbl_idx: int
    ) -> Iterator[RawTag]:
        if not table:
            return
        # Some Philips PDFs (e.g. Hemodynamic Application) render a multi-line
        # header that pdfplumber splits across 2-3 rows with ragged blanks.
        # Merge raw rows column-wise (before squeeze) until the merged header
        # contains both "Tag" and a name column, or we hit a data row.
        def _merge_raw(
            a: list[str | None], b: list[str | None]
        ) -> list[str | None]:
            w = max(len(a), len(b))
            out: list[str | None] = []
            for i in range(w):
                ca = (a[i] if i < len(a) else None) or ""
                cb = (b[i] if i < len(b) else None) or ""
                joined = " ".join(s for s in (ca.strip(), cb.strip()) if s)
                out.append(joined or None)
            return out

        raw_hdr = list(table[0])
        header_rows = 1
        use_raw = False
        while header_rows < min(len(table), 4):
            merged_sq = squeeze_row(raw_hdr)
            if (
                _col(merged_sq, "tag") is not None
                and _col_any(merged_sq, _NAME_HEADERS) is not None
            ):
                break
            nxt_raw = list(table[header_rows])
            nxt_sq = squeeze_row(nxt_raw)
            # Stop if the next row looks like a data row (has a tag cell).
            if any(_TAG_RE.match(c.strip()) for c in nxt_sq if c):
                break
            raw_hdr = _merge_raw(raw_hdr, nxt_raw)
            header_rows += 1
            use_raw = True  # Multi-row header → must use raw column indices.

        if use_raw:
            header = [(c or "").strip() for c in raw_hdr]
        else:
            header = squeeze_row(raw_hdr)
        idx_name = _col_any(header, _NAME_HEADERS)
        idx_tag = _col(header, "tag")
        idx_desc = _col_any(header, _DESC_HEADERS)
        idx_vr = _col(header, "vr")
        idx_vm = _col(header, "vm")

        if idx_tag is not None and idx_name is not None:
            # Fresh header — remember it for continuation tables.
            self._last_idx_name = idx_name
            self._last_idx_tag = idx_tag
            self._last_idx_desc = idx_desc
            self._last_idx_vr = idx_vr
            self._last_idx_vm = idx_vm
            self._last_use_raw = use_raw
            body = table[header_rows:]
        else:
            # Candidate continuation: must have a previous header, and the
            # first body row's tag cell must look like a Philips private tag.
            if (
                self._last_idx_name is None
                or self._last_idx_tag is None
            ):
                return
            use_raw = self._last_use_raw
            first_cells = (
                [(c or "").strip() for c in table[0]]
                if use_raw
                else squeeze_row(table[0])
            )
            if (
                self._last_idx_tag >= len(first_cells)
                or not _TAG_RE.match(first_cells[self._last_idx_tag].strip())
            ):
                return
            idx_name = self._last_idx_name
            idx_tag = self._last_idx_tag
            idx_desc = self._last_idx_desc
            idx_vr = self._last_idx_vr
            idx_vm = self._last_idx_vm
            body = table  # entire block is data

        # Sticky creator map (harvester-wide) — populated by in-table creator
        # rows and also by text-level "Private creator code" declarations.
        creator_map = self._creator_map

        for row in body:
            cells = (
                [(c or "").strip() for c in row] if use_raw else squeeze_row(row)
            )
            if idx_tag is None or idx_tag >= len(cells):
                continue
            tag_cell = cells[idx_tag].strip()

            m_creator = _CREATOR_TAG_RE.match(tag_cell)
            if m_creator:
                group = m_creator.group(1).upper()
                # Creator string may be in the description/value column or
                # in the name column itself ("Private Creator Group 2003").
                creator_val = None
                if (
                    idx_desc is not None
                    and idx_desc < len(cells)
                    and cells[idx_desc]
                ):
                    creator_val = cells[idx_desc].strip()
                # Fall back to any cell that looks like a creator string
                # (not "Private Creator ...").
                if not creator_val or creator_val.lower().startswith(
                    "private creator"
                ):
                    for c in cells:
                        cs = c.strip()
                        if (
                            cs
                            and not cs.lower().startswith("private creator")
                            and not _TAG_RE.match(cs)
                            and not _CREATOR_TAG_RE.match(cs)
                            and 3 <= len(cs) <= 64
                            and not cs.isdigit()
                        ):
                            creator_val = cs
                            break
                if creator_val:
                    creator_map[group] = creator_val
                continue

            m = _TAG_RE.match(tag_cell)
            if not m:
                continue
            group = m.group(1).upper()
            elem_lo = m.group(2).upper()
            synthetic = f"({group},xx{elem_lo})"
            name = (
                cells[idx_name].strip().lstrip(">").strip()
                if idx_name is not None and idx_name < len(cells)
                else ""
            )
            if not name or len(name) > 200:
                continue
            desc = (
                cells[idx_desc].strip()
                if idx_desc is not None
                and idx_desc < len(cells)
                and cells[idx_desc]
                else None
            )
            vr = None
            if idx_vr is not None and idx_vr < len(cells):
                vr_cell = cells[idx_vr].strip().upper()
                if re.fullmatch(r"[A-Z]{2}", vr_cell):
                    vr = vr_cell
            vm = None
            if idx_vm is not None and idx_vm < len(cells):
                vm_cell = cells[idx_vm].strip()
                if re.fullmatch(r"\d+(?:-(?:\d+|[nN]))?", vm_cell):
                    vm = vm_cell
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=tbl_idx,
                tag_str=synthetic,
                private_creator=creator_map.get(group),
                name=name,
                vr=vr,
                vm=vm,
                description=desc,
            )

    def _parse_text(self, text: str, page: int) -> Iterator[RawTag]:
        """Running-text fallback for two formats.

        Variant A — tag-first (Cloud / Gateway-style appendices)::

            (7053,xx00)   Philips PET Private Group   DS   1   SUV Factor - ...
            (0019,xx23)   GEMS_ACQU_01                DS   1   Table Speed ...

        Variant B — name-first (Forcare-style 1-row dictionaries)::

            Forcare defines private attributes with Block Descriptor (0067, 00xx) = "Forcare B.V.".
            AdditionalSeriesInfo (0067,xx01) SQ 1 Additional Series Information.

        For variant B the creator string is stated in prose on a line nearby;
        we extract the most recent declaration of the form
        ``Block Descriptor (GGGG, 00xx) = "<creator>"`` or variants.
        """
        creator_decl_re = re.compile(
            r"Block\s+Descriptor\s+\(\s*(?P<group>[0-9A-Fa-f]{4})\s*,\s*00[xX]{2}\s*\)"
            r"\s*=?\s*[\"'\u201c\u201d]?(?P<creator>[^\"'\n\u201c\u201d]{3,64}?)[\"'\u201c\u201d\.]",
            re.IGNORECASE,
        )
        # EasyAccess / SECTRA-style: "Private creator code (GGGG,00xx) LO 1 Value: <creator>"
        # Philips sometimes uses yy or zz as placeholder letters for different groups.
        creator_code_re = re.compile(
            r"Private\s+creator\s+code\s+\(\s*(?P<group>[0-9A-Fa-f]{4})\s*,\s*00[a-zA-Z]{2}\s*\)"
            r"\s+[A-Z]{2}\s+\d+(?:-(?:\d+|[nN]))?\s+"
            r"(?:Value\s*[:=]\s*)?(?P<creator>\S[^\n]{1,63}?)\s*$",
            re.IGNORECASE | re.MULTILINE,
        )
        declared_creators: dict[str, str] = {}
        for m in creator_decl_re.finditer(text):
            declared_creators[m.group("group").upper()] = m.group("creator").strip()
        for m in creator_code_re.finditer(text):
            declared_creators.setdefault(
                m.group("group").upper(), m.group("creator").strip()
            )

        for line in text.split("\n"):
            line = line.strip()
            if not line:
                continue

            # Variant A first (tag-first). It requires the line to start with
            # an opening paren so it unambiguously wins over variant B.
            m = _TEXT_LINE_RE.match(line)
            if m:
                group = m.group("group").upper()
                elem_lo = m.group("elem").upper()
                creator = m.group("creator").strip()
                if len(creator) < 3 or len(creator) > 64:
                    continue
                vr = m.group("vr").upper()
                vm = m.group("vm").strip()
                desc = (m.group("desc") or "").strip() or None
                synthetic = f"({group},xx{elem_lo})"
                yield RawTag(
                    source_pdf=self.pdf_path.name,
                    source_page=page,
                    source_table=None,
                    tag_str=synthetic,
                    private_creator=creator,
                    name=desc or creator,
                    vr=vr,
                    vm=vm,
                    description=desc,
                )
                continue

            # Variant B (name-first).
            m = _NAME_TAG_LINE_RE.match(line)
            if not m:
                continue
            name = m.group("name").strip()
            # Drop lines that look like public tag references or prose.
            if name.lower().startswith(("see ", "note", "table")):
                continue
            group = m.group("group").upper()
            elem_lo = m.group("elem").upper()
            vr = m.group("vr").upper()
            vm = m.group("vm").strip()
            desc = (m.group("desc") or "").strip() or None
            creator = declared_creators.get(group)
            if not creator:
                # Without a creator declaration the row is not safely
                # attributable; skip rather than guess.
                continue
            synthetic = f"({group},xx{elem_lo})"
            yield RawTag(
                source_pdf=self.pdf_path.name,
                source_page=page,
                source_table=None,
                tag_str=synthetic,
                private_creator=creator,
                name=name,
                vr=vr,
                vm=vm,
                description=desc,
            )
