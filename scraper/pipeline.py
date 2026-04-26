"""End-to-end pipeline driver.

    python -m scraper.pipeline harvest siemens data/pdfs/Siemens_*.pdf
        → writes data/interim/<pdf>.jsonl  (one NormalizedTag per line)
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from rich.console import Console
from rich.table import Table

from scraper.harvest.acuson import AcusonHarvester
from scraper.harvest.ge import GEHarvester
from scraper.harvest.philips import PhilipsHarvester
from scraper.harvest.siemens import SiemensHarvester
from scraper.models import NormalizedTag
from scraper.normalize import normalize

console = Console()

HARVESTERS = {
    "siemens": SiemensHarvester,
    "ge": GEHarvester,
    "philips": PhilipsHarvester,
    "acuson": AcusonHarvester,
}

INTERIM_DIR = Path(__file__).resolve().parent.parent / "data" / "interim"


def _out_path(pdf: Path) -> Path:
    INTERIM_DIR.mkdir(parents=True, exist_ok=True)
    return INTERIM_DIR / (pdf.stem + ".jsonl")


def harvest(vendor: str, pdf: Path, *, quiet: bool = False) -> Path:
    # Fallback: unknown vendors use the siemens harvester as a best-effort default.
    cls = HARVESTERS.get(vendor, HARVESTERS["siemens"])
    harv = cls(pdf)
    normalized: list[NormalizedTag] = []
    raw_seen = 0
    skipped = 0
    for raw in harv.harvest():
        raw_seen += 1
        n = normalize(raw, vendor)
        if n is None:
            skipped += 1
        else:
            normalized.append(n)

    out = _out_path(pdf)
    with out.open("w", encoding="utf-8") as fh:
        for n in normalized:
            fh.write(n.model_dump_json() + "\n")

    if not quiet:
        t = Table(title=f"{pdf.name}")
        t.add_column("metric")
        t.add_column("value", justify="right")
        t.add_row("raw rows", str(raw_seen))
        t.add_row("normalized", str(len(normalized)))
        t.add_row("skipped", str(skipped))
        t.add_row("output", str(out.relative_to(Path.cwd())))
        if normalized:
            creators = {n.private_creator for n in normalized if n.private_creator}
            t.add_row("private creators", str(len(creators)))
        console.print(t)
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    h = sub.add_parser("harvest")
    h.add_argument("vendor", choices=sorted(HARVESTERS))
    h.add_argument("pdf", type=Path)
    args = ap.parse_args(argv)

    if args.cmd == "harvest":
        if not args.pdf.exists():
            console.print(f"[red]not found: {args.pdf}")
            return 2
        harvest(args.vendor, args.pdf)
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
