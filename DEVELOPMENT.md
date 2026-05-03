# Development guide

This document covers the repository layout, how to rebuild `tags.dmap` from
the checked-in source data, how to re-run the full PDF scrape pipeline, and
how to run the test suite. If you just want to **use** the dictionary, see
[README.md](README.md).

---

## Repository layout

```
scraper/            Python — PDF → JSON-L pipeline
  harvest/            per-vendor extractors (ge, siemens, philips, canon, acuson)
  resolve.py          merges interim JSON-L → ResolvedTag (majority VR vote)
  ingest_pydicom.py   imports pydicom/GDCM entries not covered by PDFs

compiler/           Rust: JSON-L → .dmap frozen archive
  src/main.rs            `dmap-compile` binary
  src/bin/lookup.rs      `dicom-lookup` CLI binary

dicom-map/          Rust library: mmap-backed O(log n) lookup
  FORMAT.md              binary format & versioning policy
  src/lookup.rs          DmapDict::open / from_static
  tests/integration.rs   per-family smoke tests

dicom-map-py/       PyO3 Python bindings → `import dicom_map`
dicom-map-ffi/      C ABI → libdicom_map_ffi.{so,a} + dicom_map.h

data/
  standard/           PS3.6 public tags (checked in, 1.3 MB)
  resolved_pydicom_backfilled.jsonl  resolved + pydicom-ingested (checked in, 14k rows)
  pdfs/ raw/ interim/                gitignored

tags.csv            Human-editable canonical source of truth (checked in)
tags.dmap           Compiled binary artifact (checked in for convenience)
```

---

## Rebuild `tags.dmap` from source

The checked-in `tags.dmap` is compiled from `data/resolved_pydicom_backfilled.jsonl`
and `data/standard/attributes.json`. To recompile it locally:

```bash
python -m venv .venv && source .venv/bin/activate
pip install -e . maturin pydicom

cargo run --release --bin dmap-compile -- \
  --standard data/standard/attributes.json \
  --resolved data/resolved_pydicom_backfilled.jsonl \
  --out tags.dmap
```

CI verifies that the committed `tags.dmap` matches a fresh rebuild on every
push, so it will never silently drift from the source data.

---

## Re-scrape from PDFs (optional)

The PDF corpus is not checked in. Download it from archive.org using the
manifest in `data/sources.json`:

```bash
pip install -e ".[ops]"
python scripts/fetch_from_archive.py   # downloads all vendor PDFs from archive.org
```

Then re-harvest, resolve, and merge:

```bash
python -m scraper.harvest_batch --vendor ge --jobs 4
python -m scraper.harvest_batch --vendor siemens --jobs 6
python -m scraper.harvest_batch --vendor philips --jobs 4
python -m scraper.harvest_batch --vendor canon --jobs 4
python -m scraper.harvest_batch --vendor acuson --jobs 4
python -m scraper.resolve -o data/resolved.jsonl data/interim/*.jsonl
python -m scraper.ingest_pydicom \
  --input data/resolved.jsonl \
  --output data/resolved_pydicom_backfilled.jsonl
```

---

## Running the test suite

```bash
# Rust tests
cargo test --release --workspace

# Python unit tests + adapter tests
source .venv/bin/activate
cd dicom-map-py && maturin develop --release && cd ..
pytest tests/ -v

# Round-trip fuzz (random-sample lookup)
python tests/roundtrip_fuzz.py
```

---

## Binary format

See [dicom-map/FORMAT.md](dicom-map/FORMAT.md) for the `.dmap` binary format
specification and versioning policy.

---

## Roadmap

See [ROADMAP.md](ROADMAP.md) for planned work.
