"""Probe a DICOM conformance PDF to decide how to harvest it.

Prints a structured report: page count, text extractability, likely
location of the private-tag appendix, table layout, and observed private
creator strings. Use this BEFORE writing or tweaking a vendor harvester.

Usage:
    python -m scraper.inspect_pdf path/to/vendor.pdf [--pages 1-50]
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

import pdfplumber
from rich.console import Console
from rich.table import Table

console = Console()

# A DICOM tag in text form. Accept hex digits plus 'x' placeholders commonly
# used in vendor docs to denote the private-block offset byte, e.g. (0019,xx0C).
TAG_TEXT_RE = re.compile(r"\(([0-9A-Fa-fxX]{4})\s*,\s*([0-9A-Fa-fxX]{4})\)")

# Heuristic markers that a page likely contains a private tag appendix.
APPENDIX_HINTS = (
    "private",
    "private data element",
    "private attribute",
    "private creator",
    "private tag",
)

# Canonical VR codes (PS3.5 §6.2) — used to score table columns.
VR_CODES = {
    "AE", "AS", "AT", "CS", "DA", "DS", "DT", "FL", "FD", "IS",
    "LO", "LT", "OB", "OD", "OF", "OL", "OV", "OW", "PN", "SH",
    "SL", "SQ", "SS", "ST", "SV", "TM", "UC", "UI", "UL", "UN",
    "UR", "US", "UT", "UV",
}


@dataclass
class PageStats:
    index: int                      # 1-based
    chars: int
    tag_hits: int
    appendix_hint: bool
    has_tables: bool
    table_shapes: list[tuple[int, int]] = field(default_factory=list)  # (rows, cols)


@dataclass
class Report:
    path: Path
    n_pages: int
    text_pages: int
    tag_hit_pages: list[int]
    appendix_ranges: list[tuple[int, int]]
    table_col_counts: Counter
    table_headers: Counter
    private_creators: Counter
    sample_tag_lines: list[str]


# ---------------------------------------------------------------------------


def _contiguous_ranges(nums: list[int]) -> list[tuple[int, int]]:
    if not nums:
        return []
    nums = sorted(set(nums))
    out: list[tuple[int, int]] = []
    start = prev = nums[0]
    for n in nums[1:]:
        if n == prev + 1:
            prev = n
        else:
            out.append((start, prev))
            start = prev = n
    out.append((start, prev))
    return out


def _looks_like_header(row: list[str | None]) -> bool:
    cells = [c.strip().lower() for c in row if c]
    joined = " ".join(cells)
    return any(k in joined for k in ("tag", "group", "element")) and (
        "vr" in joined or "name" in joined or "description" in joined
    )


def _guess_private_creators(text: str) -> list[str]:
    # Private creator strings tend to be ALL-CAPS vendor-ish identifiers, often
    # quoted or near the phrase "Private Creator". Very heuristic.
    out: set[str] = set()
    for m in re.finditer(r'"([A-Z][A-Z0-9 _.\-/]{4,48})"', text):
        out.add(m.group(1))
    for m in re.finditer(
        r"Private Creator[^A-Z]{0,20}([A-Z][A-Z0-9 _.\-/]{4,48})", text
    ):
        out.add(m.group(1).strip())
    return sorted(out)


def inspect(path: Path, pages: range | None = None) -> Report:
    tag_hit_pages: list[int] = []
    appendix_pages: list[int] = []
    col_counts: Counter = Counter()
    headers: Counter = Counter()
    creators: Counter = Counter()
    sample_lines: list[str] = []
    text_pages = 0
    total_pages = 0

    with pdfplumber.open(path) as pdf:
        total_pages = len(pdf.pages)
        wanted = pages or range(1, total_pages + 1)
        for i, page in enumerate(pdf.pages, start=1):
            if i not in wanted:
                continue
            text = page.extract_text() or ""
            if text.strip():
                text_pages += 1

            lower = text.lower()
            hits = TAG_TEXT_RE.findall(text)
            if hits:
                tag_hit_pages.append(i)
                if len(sample_lines) < 8:
                    for line in text.splitlines():
                        if TAG_TEXT_RE.search(line):
                            sample_lines.append(f"p{i}: {line.strip()[:140]}")
                            if len(sample_lines) >= 8:
                                break

            if any(h in lower for h in APPENDIX_HINTS):
                appendix_pages.append(i)

            for c in _guess_private_creators(text):
                creators[c] += 1

            try:
                tables = page.extract_tables() or []
            except Exception:  # noqa: BLE001 — pdfplumber can choke on odd pages
                tables = []
            for tbl in tables:
                if not tbl or not tbl[0]:
                    continue
                col_counts[len(tbl[0])] += 1
                first = tbl[0]
                if _looks_like_header(first):
                    key = " | ".join((c or "").strip().lower() for c in first)
                    headers[key] += 1

    return Report(
        path=path,
        n_pages=total_pages,
        text_pages=text_pages,
        tag_hit_pages=tag_hit_pages,
        appendix_ranges=_contiguous_ranges(appendix_pages),
        table_col_counts=col_counts,
        table_headers=headers,
        private_creators=creators,
        sample_tag_lines=sample_lines,
    )


# ---------------------------------------------------------------------------


def _classify(r: Report) -> str:
    """Return a rough harvester class (see Plan.md Step 5)."""
    if r.text_pages == 0:
        return "D — scanned / image-only (OCR needed)"
    if r.table_headers and any("vr" in h for h in r.table_headers):
        return "A — clean tables with VR column"
    if r.table_col_counts:
        return "B — tables present but headers unclear / merged cells"
    if r.tag_hit_pages:
        return "C — tags in prose / free text"
    return "? — no DICOM tags detected at all"


def render(r: Report) -> None:
    console.rule(f"[bold]{r.path.name}")
    console.print(f"pages: {r.n_pages}   text pages: {r.text_pages}")
    console.print(f"pages containing tags: {len(r.tag_hit_pages)}")
    if r.tag_hit_pages:
        span = f"{r.tag_hit_pages[0]}–{r.tag_hit_pages[-1]}"
        console.print(f"  first–last: {span}")
    if r.appendix_ranges:
        ranges = ", ".join(f"{a}–{b}" for a, b in r.appendix_ranges)
        console.print(f"private-appendix hint pages: {ranges}")

    if r.table_col_counts:
        tt = Table("cols", "count", title="table column counts")
        for cols, n in r.table_col_counts.most_common():
            tt.add_row(str(cols), str(n))
        console.print(tt)

    if r.table_headers:
        ht = Table("header signature", "n", title="detected table headers")
        for sig, n in r.table_headers.most_common(10):
            ht.add_row(sig[:120], str(n))
        console.print(ht)

    if r.private_creators:
        ct = Table("private creator (candidate)", "hits", title="private creators")
        for name, n in r.private_creators.most_common(15):
            ct.add_row(name, str(n))
        console.print(ct)

    if r.sample_tag_lines:
        console.print("[bold]sample tag lines[/bold]")
        for line in r.sample_tag_lines:
            console.print(f"  {line}")

    console.print(f"[bold green]class:[/bold green] {_classify(r)}")


def _parse_range(s: str | None) -> range | None:
    if not s:
        return None
    a, _, b = s.partition("-")
    return range(int(a), int(b) + 1)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser()
    p.add_argument("pdf", type=Path)
    p.add_argument("--pages", help="page range like 100-200", default=None)
    args = p.parse_args(argv)
    if not args.pdf.exists():
        console.print(f"[red]not found: {args.pdf}")
        return 2
    render(inspect(args.pdf, _parse_range(args.pages)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
