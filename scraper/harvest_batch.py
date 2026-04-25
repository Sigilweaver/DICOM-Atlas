"""Batch-harvest private DICOM tags from every PDF in data/pdfs/.

For each PDF lacking a data/interim/<stem>.jsonl, dispatch to the appropriate
vendor harvester (looked up in data/catalog.json) and write results.

Resource strategy
-----------------

pdfplumber holds a lot of per-page layout state in memory and does not always
release it. When parallelised naively, each worker's RSS grows PDF-by-PDF
until the host OOMs. We mitigate this with:

* ``max_tasks_per_child=1`` — each worker processes a single PDF then exits,
  returning all memory to the OS. This is the single biggest win.
* ``resource.RLIMIT_AS`` — hard per-worker virtual-memory ceiling. A runaway
  pdfplumber page cannot blow up the host.
* A file-size skip threshold — scanned manuals >100 MB almost never contain
  structured private-tag tables and waste ~minutes each.
* A conservative default worker count (4 on a 16-thread box). pdfplumber is
  not CPU-bound past ~4 workers; more workers just multiply memory pressure.

Usage
-----

    python -m scraper.harvest_batch                 # parallel, skip done
    python -m scraper.harvest_batch --force         # re-harvest everything
    python -m scraper.harvest_batch --jobs 1        # serial (for debugging)
    python -m scraper.harvest_batch --vendor philips
    python -m scraper.harvest_batch --mem-gb 6      # per-worker VM cap
    python -m scraper.harvest_batch --max-mb 150    # skip PDFs bigger than this
"""

from __future__ import annotations

import argparse
import json
import sys
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

from rich.console import Console
from rich.progress import (
    BarColumn,
    MofNCompleteColumn,
    Progress,
    SpinnerColumn,
    TaskProgressColumn,
    TextColumn,
    TimeElapsedColumn,
)
from rich.table import Table

console = Console()

ROOT = Path(__file__).resolve().parent.parent
PDFS_DIR = ROOT / "data" / "pdfs"
INTERIM_DIR = ROOT / "data" / "interim"
CATALOG_PATH = ROOT / "data" / "catalog.json"

DEFAULT_JOBS = 4            # conservative for pdfplumber
DEFAULT_MEM_GB = 6          # per-worker VM cap
DEFAULT_MAX_MB = 100        # skip PDFs bigger than this


def _install_rlimit(mem_gb: int) -> None:
    """Set a hard RLIMIT_AS ceiling inside a worker process (Linux)."""
    if mem_gb <= 0:
        return
    try:
        import resource
        cap = mem_gb * 1024 * 1024 * 1024
        resource.setrlimit(resource.RLIMIT_AS, (cap, cap))
    except Exception:
        pass


def _catalog_vendor_map() -> dict[str, str]:
    if not CATALOG_PATH.exists():
        return {}
    cat = json.loads(CATALOG_PATH.read_text())
    return {e["filename"]: e["vendor"] for e in cat.get("entries", []) if e.get("filename")}


def _worker_init(mem_gb: int) -> None:
    _install_rlimit(mem_gb)


def _harvest_one(args: tuple[str, str]) -> tuple[str, int, bool, str]:
    """Worker: harvest one PDF. Returns (pdf_name, tag_count, ok, err)."""
    vendor, pdf_str = args
    from scraper.pipeline import harvest

    pdf = Path(pdf_str)
    try:
        out = harvest(vendor, pdf, quiet=True)
        n = len(out.read_text().splitlines()) if out.exists() else 0
        return (pdf.name, n, True, "")
    except MemoryError:
        return (pdf.name, 0, False, "MemoryError (hit RLIMIT_AS cap)")
    except Exception as exc:
        return (pdf.name, 0, False, f"{type(exc).__name__}: {exc}"[:200])


def run(
    vendor_filter: str | None,
    force: bool,
    jobs: int,
    verbose: bool,
    mem_gb: int,
    max_mb: int,
) -> None:
    INTERIM_DIR.mkdir(parents=True, exist_ok=True)
    pdfs = sorted(PDFS_DIR.glob("*.pdf"))
    if not pdfs:
        console.print(f"[yellow]No PDFs found in {PDFS_DIR}")
        return

    vmap = _catalog_vendor_map()

    todo: list[tuple[str, str]] = []
    skipped_done = 0
    skipped_size = 0
    skipped_filter = 0
    for pdf in pdfs:
        vendor = vmap.get(pdf.name, "siemens")
        if vendor_filter and vendor != vendor_filter:
            skipped_filter += 1
            continue
        out = INTERIM_DIR / (pdf.stem + ".jsonl")
        if not force and out.exists():
            skipped_done += 1
            continue
        if max_mb > 0 and pdf.stat().st_size > max_mb * 1024 * 1024:
            skipped_size += 1
            if verbose:
                console.print(
                    f"  [yellow]skip[/yellow] {pdf.name} "
                    f"({pdf.stat().st_size / 1024 / 1024:.0f} MB > {max_mb} MB)"
                )
            continue
        todo.append((vendor, str(pdf)))

    console.print(
        f"[cyan]Harvest:[/cyan] {len(todo)} to process  "
        f"([green]{skipped_done}[/green] done, "
        f"[yellow]{skipped_size}[/yellow] too large, "
        f"{skipped_filter} filtered out, "
        f"{len(pdfs)} total)"
    )
    console.print(
        f"  workers={jobs}  mem_cap={mem_gb} GB/worker  "
        f"max_pdf={max_mb} MB  vendor={vendor_filter or 'ALL'}"
    )
    if not todo:
        console.print("[green]Nothing to do.[/green]")
        return

    total_tags = 0
    results: list[tuple[str, int, bool, str]] = []

    columns = [
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        MofNCompleteColumn(),
        TaskProgressColumn(),
        TimeElapsedColumn(),
    ]

    with Progress(*columns, console=console, transient=False) as progress:
        task = progress.add_task("harvesting", total=len(todo))

        if jobs == 1:
            _install_rlimit(mem_gb)
            for item in todo:
                name, n, ok, err = _harvest_one(item)
                results.append((name, n, ok, err))
                total_tags += max(n, 0)
                if verbose and n > 0:
                    progress.log(f"  [green]{n:4d}[/green]  {name}")
                elif not ok:
                    progress.log(f"  [red]ERR[/red]  {name}: {err}")
                progress.advance(task)
        else:
            # max_tasks_per_child=1 is the critical knob: each worker handles
            # exactly one PDF then exits, returning all memory to the OS.
            with ProcessPoolExecutor(
                max_workers=jobs,
                initializer=_worker_init,
                initargs=(mem_gb,),
                max_tasks_per_child=1,
            ) as pool:
                futs = [pool.submit(_harvest_one, item) for item in todo]
                for fut in as_completed(futs):
                    name, n, ok, err = fut.result()
                    results.append((name, n, ok, err))
                    total_tags += max(n, 0)
                    if verbose and n > 0:
                        progress.log(f"  [green]{n:4d}[/green]  {name}")
                    elif not ok:
                        progress.log(f"  [red]ERR[/red]  {name}: {err}")
                    progress.advance(task)

    nonzero = [(n, t) for n, t, ok, _ in results if ok and t > 0]
    zero = [n for n, t, ok, _ in results if ok and t == 0]
    errors = [(n, e) for n, _, ok, e in results if not ok]

    console.print(
        f"\n[green]Done.[/green] {len(results)} PDFs processed — "
        f"[bold]{total_tags}[/bold] tags across {len(nonzero)} PDFs  "
        f"([yellow]{len(zero)}[/yellow] zero-yield, "
        f"[red]{len(errors)}[/red] errors)"
    )

    if verbose and nonzero:
        t = Table(title="Top PDFs by tag count")
        t.add_column("tags", justify="right")
        t.add_column("file")
        for name, n in sorted(nonzero, key=lambda x: -x[1])[:40]:
            t.add_row(str(n), name)
        console.print(t)

    if errors:
        console.print("\n[red]Errors (first 20):[/red]")
        for name, err in errors[:20]:
            console.print(f"  {name}: {err}")


def main() -> None:
    p = argparse.ArgumentParser(description="Batch-harvest private DICOM tags from PDFs.")
    p.add_argument("--vendor", help="Only harvest PDFs from this vendor (per catalog.json).")
    p.add_argument("--force", action="store_true", help="Re-harvest already-processed PDFs.")
    p.add_argument("--jobs", type=int, default=DEFAULT_JOBS,
                   help=f"Parallel workers (default: {DEFAULT_JOBS}).")
    p.add_argument("--mem-gb", type=int, default=DEFAULT_MEM_GB,
                   help=f"Per-worker virtual-memory cap in GB (default: {DEFAULT_MEM_GB}). "
                        "0 disables.")
    p.add_argument("--max-mb", type=int, default=DEFAULT_MAX_MB,
                   help=f"Skip PDFs larger than this (MB, default: {DEFAULT_MAX_MB}). "
                        "0 disables.")
    p.add_argument("--verbose", "-v", action="store_true", help="Print per-PDF tag counts.")
    args = p.parse_args()
    try:
        run(args.vendor, args.force, args.jobs, args.verbose, args.mem_gb, args.max_mb)
    except KeyboardInterrupt:
        console.print("\n[yellow]Interrupted.[/yellow]")
        sys.exit(130)


if __name__ == "__main__":
    main()
