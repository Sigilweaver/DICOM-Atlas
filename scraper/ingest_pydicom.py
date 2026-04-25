"""Ingest pydicom's bundled private dictionary (originally from GDCM) for
creators that our PDF scrape does not cover.

The resulting rows are appended into the resolved JSONL with:
  - sources = ["pydicom"]
  - vendors = [<inferred from creator string>] or ["unknown"]
  - description = ""  (pydicom doesn't carry descriptions)
  - retired flag derived from the pydicom entry's 4th tuple element (string;
    "1" or "True" or non-empty == retired in some forks; in current pydicom
    this slot is typically empty/keyword)

Provenance: pydicom's private dictionary is itself derived from the GDCM
project (see pydicom/_private_dict.py header). Both projects use permissive
licenses (MIT and BSD-3-Clause respectively) and explicitly permit
redistribution with attribution. See THIRD_PARTY_LICENSES.md.

We only add (creator, group, element_lo) triples that are NOT already
present in our resolved data. We never overwrite existing entries.

Usage:
    python -m scraper.ingest_pydicom \
      --input data/resolved_pydicom_backfilled.jsonl \
      --output data/resolved_pydicom_backfilled.jsonl

(In-place is supported; the script reads the entire input first.)
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from pydicom._private_dict import private_dictionaries as _PD


# ---------------------------------------------------------------------------
# Vendor inference from creator strings.

GE_RX      = re.compile(r"^(GEMS_|GEHC_|GEIIS_|GE )", re.IGNORECASE)
SIEMENS_RX = re.compile(r"^(SIEMENS|SYNGO)", re.IGNORECASE)
PHILIPS_RX = re.compile(r"^PHILIPS", re.IGNORECASE)
TOSHIBA_RX = re.compile(r"(TOSHIBA|CANON)", re.IGNORECASE)


def infer_vendor(creator: str) -> str:
    if GE_RX.search(creator):
        return "ge"
    if SIEMENS_RX.search(creator):
        return "siemens"
    if PHILIPS_RX.search(creator):
        return "philips"
    if TOSHIBA_RX.search(creator):
        return "toshiba_canon"
    return "unknown"


# ---------------------------------------------------------------------------
# Keyword derivation: pydicom doesn't carry keywords for private tags. We
# generate a CamelCase one from the name so consumers can still keyword-match.

_KW_SPLIT = re.compile(r"[\s\-_/.,()\[\]:'\"]+")


def derive_keyword(name: str) -> str:
    if not name:
        return ""
    parts = [p for p in _KW_SPLIT.split(name) if p]
    cleaned = []
    for p in parts:
        # Strip non-alphanumerics but keep digits attached to letters
        s = "".join(ch for ch in p if ch.isalnum())
        if not s:
            continue
        # If already mixed-case (e.g. "ImageType"), keep as-is; else title-case
        if any(c.isupper() for c in s) and any(c.islower() for c in s):
            cleaned.append(s)
        else:
            cleaned.append(s[:1].upper() + s[1:].lower())
    return "".join(cleaned)


# ---------------------------------------------------------------------------
# Parse pydicom keys: "GGGGxxEE" with EE = block offset (block-relative).

_KEY_BLOCK_RX = re.compile(r"^([0-9A-Fa-f]{4})xx([0-9A-Fa-f]{2})$")
_KEY_CONCRETE_RX = re.compile(r"^([0-9A-Fa-f]{4})([0-9A-Fa-f]{4})$")


def parse_pydicom_key(k: str) -> tuple[int, int, bool] | None:
    """Returns (group, element, is_block_offset) or None for unsupported formats.

    pydicom uses two main key shapes:
      - ``GGGGxxEE`` — block-relative private tag (low byte = block offset)
      - ``GGGGEEEE`` — concrete (non-block-relative) private tag

    A small minority of entries use ``GGBBxxEE`` or ``GGGGEExx`` to encode
    additional block constraints; these are skipped for now.
    """
    m = _KEY_BLOCK_RX.match(k)
    if m:
        return int(m.group(1), 16), int(m.group(2), 16), True
    m = _KEY_CONCRETE_RX.match(k)
    if m:
        return int(m.group(1), 16), int(m.group(2), 16), False
    return None


# ---------------------------------------------------------------------------


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--input", type=Path, default=Path("data/resolved_pydicom_backfilled.jsonl"))
    p.add_argument("--output", type=Path, default=Path("data/resolved_pydicom_backfilled.jsonl"))
    p.add_argument(
        "--skip-creators",
        type=Path,
        default=None,
        help="Optional path to a file with one creator per line to skip.",
    )
    args = p.parse_args()

    # Load existing rows
    existing: list[dict] = []
    seen_keys: set[tuple[str, int, int]] = set()
    with args.input.open() as fh:
        for ln in fh:
            ln = ln.strip()
            if not ln:
                continue
            r = json.loads(ln)
            existing.append(r)
            creator = (r.get("private_creator") or "").strip().upper()
            if creator:
                seen_keys.add((creator, int(r["group"]), int(r["element"])))

    skip_creators: set[str] = set()
    if args.skip_creators and args.skip_creators.exists():
        skip_creators = {
            ln.strip().upper()
            for ln in args.skip_creators.read_text().splitlines()
            if ln.strip()
        }

    # Walk pydicom's dict and collect rows we don't already have
    new_rows: list[dict] = []
    skipped_have = 0
    skipped_filter = 0
    by_vendor: dict[str, int] = {}

    for creator, entries in _PD.items():
        creator_norm = creator.strip().upper()
        if creator_norm in skip_creators:
            continue
        vendor = infer_vendor(creator)
        for key, meta in entries.items():
            parsed = parse_pydicom_key(key)
            if not parsed:
                skipped_filter += 1
                continue
            group, elem, is_block = parsed

            if (creator_norm, group, elem) in seen_keys:
                skipped_have += 1
                continue

            vr = (meta[0] or "").strip() if len(meta) > 0 else ""
            vm = (meta[1] or "").strip() if len(meta) > 1 else "1"
            name = (meta[2] or "").strip() if len(meta) > 2 else ""
            retired_field = (meta[3] or "").strip() if len(meta) > 3 else ""

            if not vr or vr in {"NONE", "??"}:
                vr = "UN"

            # In pydicom 3.x the 4th tuple slot is sometimes a keyword and
            # sometimes a retired marker; treat any explicit "1"/"True"/"Y"
            # as retired and otherwise default to False.
            retired = retired_field.lower() in {"1", "true", "y", "yes", "retired"}

            row = {
                "group": group,
                "element": elem,
                "element_is_block_offset": is_block,
                "private_creator": creator,  # preserve original casing
                "keyword": derive_keyword(name),
                "name": name,
                "vr": vr,
                "vm": vm or "1",
                "description": "",
                "retired": retired,
                "vendors": [vendor],
                "sources": ["pydicom"],
            }
            new_rows.append(row)
            by_vendor[vendor] = by_vendor.get(vendor, 0) + 1

    # Sort new rows by (creator, group, element) so the JSONL is stable
    new_rows.sort(key=lambda r: (r["private_creator"], r["group"], r["element"]))

    # Write merged output
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w") as fh:
        for r in existing:
            fh.write(json.dumps(r) + "\n")
        for r in new_rows:
            fh.write(json.dumps(r) + "\n")

    # Report
    total_pd = sum(len(t) for t in _PD.values())
    print(f"pydicom total entries:    {total_pd}")
    print(f"already in our scrape:    {skipped_have}")
    print(f"skipped (bad key):        {skipped_filter}")
    print(f"new rows ingested:        {len(new_rows)}")
    print()
    print("Ingested by inferred vendor:")
    for v, n in sorted(by_vendor.items(), key=lambda x: -x[1]):
        print(f"  {v:14s}  {n:5d}")
    print()
    print(f"Existing rows:            {len(existing)}")
    print(f"Total after merge:        {len(existing) + len(new_rows)}")
    print(f"Wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
