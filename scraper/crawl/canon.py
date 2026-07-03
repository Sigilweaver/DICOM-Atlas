"""Canon Medical DICOM conformance statement crawler.

Scrapes PDF links from:
  https://global.medical.canon/service-support/Interoperability/dicom_current_products
  https://global.medical.canon/service-support/Interoperability/DICOM_PastProducts

and downloads each PDF to data/pdfs/, skipping files already present.

Behaviour
---------
- 2-second delay between HTTP requests (polite crawling).
- Retries up to 3 times on transient errors (5xx, connection error) with
  exponential back-off.
- Skips files already downloaded (idempotent; safe to re-run).
- Hard size cap: PDFs > MAX_PDF_MB are skipped with a warning (Ultrasound
  PDFs can exceed 16 MB).
- Writes a manifest CSV to data/canon_manifest.csv recording every PDF URL
  with modality, product, version and doc-id metadata.

Usage
-----
    python -m scraper.crawl.canon                   # dry-run (default)
    python -m scraper.crawl.canon --download        # actually download
    python -m scraper.crawl.canon --download --max-mb 50
    python -m scraper.crawl.canon --download --delay 3
"""

from __future__ import annotations

import argparse
import csv
import re
import sys
import time
from dataclasses import dataclass, fields
from pathlib import Path
from urllib.parse import unquote, urlparse

import requests
from bs4 import BeautifulSoup
from rich.console import Console
from rich.progress import (
    BarColumn,
    MofNCompleteColumn,
    Progress,
    SpinnerColumn,
    TextColumn,
    TimeElapsedColumn,
)

console = Console()

ROOT = Path(__file__).resolve().parent.parent.parent
PDFS_DIR = ROOT / "data" / "pdfs"
MANIFEST_PATH = ROOT / "data" / "canon_manifest.csv"

INDEX_URLS = [
    "https://global.medical.canon/service-support/Interoperability/dicom_current_products",
    "https://global.medical.canon/service-support/Interoperability/DICOM_PastProducts",
]

BASE_URL = "https://global.medical.canon"
_USER_AGENT = "private-dicom-tags-research/1.0 (+https://github.com; academic research)"

DEFAULT_DELAY = 2.0   # seconds between requests
DEFAULT_MAX_MB = 30   # skip PDFs larger than this
MAX_RETRIES = 3


@dataclass
class PdfEntry:
    source_page: str        # "current" or "past"
    modality: str
    product: str
    version: str
    doc_id: str
    url: str
    size_hint: str = ""     # e.g. "827KB" from link text


def _session() -> requests.Session:
    s = requests.Session()
    s.headers.update({"User-Agent": _USER_AGENT})
    return s


def _get_with_retry(
    session: requests.Session,
    url: str,
    delay: float,
    stream: bool = False,
) -> requests.Response | None:
    """GET with exponential back-off on transient errors."""
    for attempt in range(1, MAX_RETRIES + 1):
        try:
            resp = session.get(url, stream=stream, timeout=60)
            if resp.status_code < 500:
                return resp
            console.print(
                f"  [yellow]HTTP {resp.status_code} on attempt {attempt}: {url}"
            )
        except requests.RequestException as exc:
            console.print(f"  [yellow]Network error attempt {attempt}: {exc}")
        if attempt < MAX_RETRIES:
            time.sleep(delay * 2**attempt)
    return None


def _parse_index_page(html: str, source_label: str) -> list[PdfEntry]:
    """Parse one Canon DICOM index page into a flat list of PdfEntry records."""
    soup = BeautifulSoup(html, "html.parser")
    entries: list[PdfEntry] = []

    # Walk the content area. Section headings (h2) name the modality.
    # Each modality section contains one or more tables. Within each table,
    # rows have 3 cells (product | version | doc) or 2 cells (version | doc)
    # when the product name is carried forward from the previous row.
    current_modality = "Unknown"
    current_product = ""

    # Find all h2 and table nodes in document order.
    content = soup.find("body") or soup
    for node in content.descendants:
        tag = getattr(node, "name", None)
        if tag == "h2":
            heading = node.get_text(" ", strip=True)
            # Skip navigation / footer headings that are not modality names.
            if len(heading) < 60:
                current_modality = heading
                current_product = ""
            continue

        if tag != "table":
            continue

        rows = node.find_all("tr")
        # Ignore tables without any PDF links.
        if not any(
            ".pdf" in (a.get("href", "")).lower()
            for row in rows
            for a in row.find_all("a", href=True)
        ):
            continue

        # Reset product carry-forward at each new table.
        row_product = current_product
        for row in rows:
            cells = row.find_all(["td", "th"])
            if not cells:
                continue

            # Skip pure-header rows (all cells are <th>).
            if all(c.name == "th" for c in cells):
                continue

            cell_texts = [c.get_text(" ", strip=True) for c in cells]
            pdf_anchors = [
                a
                for c in cells
                for a in c.find_all("a", href=True)
                if ".pdf" in a["href"].lower()
            ]
            if not pdf_anchors:
                continue

            # Determine column mapping: 3 cols = product | version | doc,
            # 2 cols = version | doc, 1 col = doc only.
            n = len(cells)
            if n >= 3 and cell_texts[0].strip():
                row_product = cell_texts[0].strip()
                version_text = cell_texts[1].strip()
            elif n == 2:
                version_text = cell_texts[0].strip()
            else:
                version_text = ""

            for a in pdf_anchors:
                href = a["href"]
                if href.startswith("/"):
                    href = BASE_URL + href

                # Extract doc-id: base filename without extension.
                path = unquote(urlparse(href).path)
                doc_id = Path(path).stem

                # Size hint from link text, e.g. "2G985-047EN (PDF:827KB)".
                link_text = a.get_text(" ", strip=True)
                size_hint = ""
                m = re.search(r"\(PDF:([^)]+)\)", link_text)
                if m:
                    size_hint = m.group(1).strip()

                entries.append(
                    PdfEntry(
                        source_page=source_label,
                        modality=current_modality,
                        product=row_product,
                        version=version_text,
                        doc_id=doc_id,
                        url=href,
                        size_hint=size_hint,
                    )
                )

    return entries


def _parse_size_mb(hint: str) -> float | None:
    """Parse '827KB', '1.09MB', '8.27MB' into MB. Returns None if unparseable."""
    hint = hint.strip().upper()
    m = re.match(r"([\d.,]+)\s*(KB|MB)", hint)
    if not m:
        return None
    val = float(m.group(1).replace(",", ""))
    if m.group(2) == "KB":
        val /= 1024
    return val


def crawl(
    download: bool,
    delay: float,
    max_mb: float,
) -> None:
    session = _session()
    PDFS_DIR.mkdir(parents=True, exist_ok=True)

    # -- 1. Fetch and parse index pages --
    all_entries: list[PdfEntry] = []
    labels = ["current", "past"]
    for url, label in zip(INDEX_URLS, labels):
        console.print(f"[cyan]Fetching index[/cyan] {url}")
        resp = _get_with_retry(session, url, delay)
        if resp is None or resp.status_code != 200:
            console.print(f"[red]Failed to fetch {url}")
            sys.exit(1)
        time.sleep(delay)
        entries = _parse_index_page(resp.text, label)
        console.print(f"  found [green]{len(entries)}[/green] PDF links ({label})")
        all_entries.extend(entries)

    # Deduplicate by URL (same PDF may appear for multiple model variants).
    seen_urls: set[str] = set()
    unique_entries: list[PdfEntry] = []
    for e in all_entries:
        if e.url not in seen_urls:
            seen_urls.add(e.url)
            unique_entries.append(e)

    console.print(
        f"\n[cyan]Total:[/cyan] {len(all_entries)} links, "
        f"[green]{len(unique_entries)}[/green] unique PDFs"
    )

    # -- 2. Write manifest (always, even in dry-run) --
    field_names = [f.name for f in fields(PdfEntry)] + ["local_filename"]
    with MANIFEST_PATH.open("w", newline="") as mf:
        writer = csv.DictWriter(mf, fieldnames=field_names)
        writer.writeheader()
        for e in unique_entries:
            path = unquote(urlparse(e.url).path)
            local = Path(path).name
            row = {f.name: getattr(e, f.name) for f in fields(PdfEntry)}
            row["local_filename"] = local
            writer.writerow(row)
    console.print(f"Manifest written to [cyan]{MANIFEST_PATH.relative_to(ROOT)}[/cyan]")

    if not download:
        console.print(
            "[yellow]Dry-run mode - pass --download to actually fetch PDFs."
        )
        return

    # -- 3. Download PDFs --
    to_download: list[tuple[PdfEntry, str, Path]] = []
    skipped_size = 0
    skipped_done = 0
    for e in unique_entries:
        path = unquote(urlparse(e.url).path)
        filename = Path(path).name
        dest = PDFS_DIR / filename
        if dest.exists():
            skipped_done += 1
            continue
        size_mb = _parse_size_mb(e.size_hint)
        if size_mb is not None and size_mb > max_mb:
            skipped_size += 1
            console.print(
                f"  [yellow]skip (large)[/yellow] {filename} "
                f"({size_mb:.1f} MB > {max_mb} MB limit)"
            )
            continue
        to_download.append((e, filename, dest))

    console.print(
        f"\n[cyan]Download:[/cyan] {len(to_download)} PDFs  "
        f"([green]{skipped_done}[/green] already present, "
        f"[yellow]{skipped_size}[/yellow] too large)"
    )

    with Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        MofNCompleteColumn(),
        TimeElapsedColumn(),
        console=console,
    ) as progress:
        task = progress.add_task("downloading", total=len(to_download))
        ok = 0
        errors: list[tuple[str, str]] = []

        for e, filename, dest in to_download:
            progress.update(task, description=filename[:55])
            resp = _get_with_retry(session, e.url, delay, stream=True)
            if resp is None or resp.status_code != 200:
                errors.append((filename, f"HTTP {getattr(resp, 'status_code', '?')}"))
                progress.advance(task)
                time.sleep(delay)
                continue

            # Stream to a .tmp file first, then rename (avoids partial writes).
            tmp = dest.with_suffix(".tmp")
            try:
                with tmp.open("wb") as f:
                    for chunk in resp.iter_content(chunk_size=65536):
                        f.write(chunk)
                tmp.rename(dest)
                ok += 1
            except Exception as exc:
                tmp.unlink(missing_ok=True)
                errors.append((filename, str(exc)))
            finally:
                resp.close()

            progress.advance(task)
            time.sleep(delay)

    console.print(f"\n[green]Downloaded {ok}[/green] PDFs  ({len(errors)} errors)")
    if errors:
        console.print("[red]Errors:[/red]")
        for fn, err in errors:
            console.print(f"  {fn}: {err}")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Crawl Canon Medical DICOM conformance statement index pages "
        "and optionally download PDFs."
    )
    parser.add_argument(
        "--download",
        action="store_true",
        help="Actually download PDFs (default: dry-run only, writes manifest).",
    )
    parser.add_argument(
        "--delay",
        type=float,
        default=DEFAULT_DELAY,
        metavar="SEC",
        help=f"Seconds between HTTP requests (default: {DEFAULT_DELAY}).",
    )
    parser.add_argument(
        "--max-mb",
        type=float,
        default=DEFAULT_MAX_MB,
        metavar="MB",
        help=f"Skip PDFs larger than this many MB (default: {DEFAULT_MAX_MB}).",
    )
    args = parser.parse_args()
    crawl(download=args.download, delay=args.delay, max_mb=args.max_mb)


if __name__ == "__main__":
    main()
