"""Dump raw table content around pages where private-tag markers appear.

Usage:
    python -m scraper.peek path/to.pdf [--around 50-80]
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import pdfplumber
from rich import print as rprint


def peek(pdf_path: Path, rng: range) -> None:
    with pdfplumber.open(pdf_path) as pdf:
        for i, page in enumerate(pdf.pages, 1):
            if i not in rng:
                continue
            tables = page.extract_tables() or []
            if not tables:
                continue
            rprint(f"\n[bold magenta]── page {i} — {len(tables)} table(s) ──")
            for ti, tbl in enumerate(tables):
                rprint(f"[cyan]table {ti} ({len(tbl)} rows)[/cyan]")
                for row in tbl[:25]:
                    cleaned = [(c or "").strip().replace("\n", " ")[:50] for c in row]
                    rprint(f"  {cleaned}")
                if len(tbl) > 25:
                    rprint(f"  ... +{len(tbl)-25} more rows")


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("pdf", type=Path)
    p.add_argument("--around", default=None, help="page range e.g. 50-80")
    args = p.parse_args()
    if args.around:
        a, _, b = args.around.partition("-")
        rng = range(int(a), int(b) + 1)
    else:
        rng = range(1, 10_000)
    peek(args.pdf, rng)
    return 0


if __name__ == "__main__":
    sys.exit(main())
