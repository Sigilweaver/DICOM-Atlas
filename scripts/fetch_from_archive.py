#!/usr/bin/env python3
"""
fetch_from_archive.py - Download DICOM conformance PDFs from archive.org.

Reads data/sources.json and downloads every PDF that has an archive_url into
data/pdfs/.  Already-present files are skipped (idempotent).

Usage:
    python scripts/fetch_from_archive.py [--vendor ge|siemens|philips] [--jobs 4]

This is the recommended way to rebuild the local PDF corpus so you can
re-run the extraction pipeline without hitting vendor websites directly.
"""

from __future__ import annotations

import argparse
import os
import json
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

import requests

REPO_ROOT = Path(__file__).resolve().parent.parent
SOURCES_JSON = REPO_ROOT / "data" / "sources.json"
PDF_DIR = REPO_ROOT / "data" / "pdfs"


def download_one(entry: dict, session: requests.Session) -> tuple[str, bool, str]:
    filename = entry["filename"]
    url = entry["archive_url"]
    dest = PDF_DIR / filename

    if dest.exists():
        return filename, True, "already present"

    try:
        r = session.get(url, timeout=60, stream=True)
        r.raise_for_status()
        dest.parent.mkdir(parents=True, exist_ok=True)
        tmp = dest.with_suffix(".tmp")
        with tmp.open("wb") as fh:
            for chunk in r.iter_content(65536):
                fh.write(chunk)
        tmp.rename(dest)
        return filename, True, "downloaded"
    except Exception as exc:
        return filename, False, str(exc)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--vendor",
        choices=["ge", "siemens", "philips"],
        help="Restrict to one vendor (default: all)",
    )
    ap.add_argument(
        "--jobs",
        type=int,
        default=4,
        help="Parallel download threads (default: 4)",
    )
    args = ap.parse_args()

    sources = json.loads(SOURCES_JSON.read_text())
    entries = [
        e
        for e in sources["sources"]
        if e.get("archive_url")
        and (args.vendor is None or e.get("vendor") == args.vendor)
    ]

    print(f"Entries to download: {len(entries)}")
    PDF_DIR.mkdir(parents=True, exist_ok=True)

    session = requests.Session()
    session.headers["User-Agent"] = "DicomAtlas-fetch/1.0 (https://github.com/Sigilweaver/DicomAtlas)"

    ok = skipped = failed = 0

    def _dl(e: dict):
        result = download_one(e, session)
        time.sleep(0.1)  # be polite to archive.org
        return result

    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = {pool.submit(_dl, e): e for e in entries}
        for i, fut in enumerate(as_completed(futures), 1):
            fname, success, msg = fut.result()
            if not success:
                print(f"  [error] {fname}: {msg}")
                failed += 1
            elif msg == "already present":
                skipped += 1
            else:
                ok += 1
            if i % 100 == 0:
                print(f"  {i}/{len(entries)}  (ok={ok} skipped={skipped} failed={failed})")

    print(f"\nDone. downloaded={ok}  skipped={skipped}  failed={failed}")


if __name__ == "__main__":
    main()
