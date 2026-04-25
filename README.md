# DICOM-Map

A single memory-mapped binary dictionary of **public and private DICOM
tags** compiled from vendor conformance statements (GE, Siemens, Philips, ...)
plus the PS3.6 standard. Query it from Rust, C, or Python in O(log n)
without pydicom or any other Python dependency.

Current shipped size: **9,610 tags** (4,316 private + 5,294 public) in a
**1 MB** `tags.dmap` file.

## Layout

```
scraper/            Python — PDF → JSON-L pipeline
  harvest/            per-vendor extractors (ge, siemens, philips)
  resolve.py          merges interim JSON-L → ResolvedTag (majority VR vote)
  pydicom_compare.py  cross-references + backfills from pydicom

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
  resolved_pydicom_backfilled.jsonl  resolved + cross-referenced (checked in)
  pdfs/ raw/ interim/                gitignored
```

## Quick start — consume the dictionary

### CLI

```bash
cargo build --release --bin dicom-lookup
./target/release/dicom-lookup 0008 0005
./target/release/dicom-lookup 0021 xx08 "Siemens: Thorax/Multix FD Lab Settings"
./target/release/dicom-lookup --json 0021 xx01 GEMS_XR3DCAL_01
```

### Python

```bash
pip install maturin
cd dicom-map-py && maturin develop --release
```

```python
import dicom_map
d = dicom_map.open("tags.dmap")
t = d.lookup(0x0021, 0x0008, "Siemens: Thorax/Multix FD Lab Settings")
# {'vr': 'US', 'name': 'Auto Window Flag', 'block_offset': True, ...}
```

### Rust

```toml
[dependencies]
dicom-map = { path = "dicom-map" }
# or with embedded mode (bakes tags.dmap into your binary):
# dicom-map = { path = "dicom-map", features = ["embedded"] }
```

```rust
let d = dicom_map::DmapDict::open("tags.dmap")?;
let t = d.lookup(0x0008, 0x0005, None).unwrap();
println!("{} {}", t.keyword(), t.vr());
```

### C / C++

```bash
cargo build --release -p dicom-map-ffi
gcc my_app.c -I dicom-map-ffi/include -L target/release -ldicom_map_ffi
```

See `dicom-map-ffi/include/dicom_map.h`.

## Quick start — rebuild from source

```bash
python -m venv .venv && source .venv/bin/activate
pip install -e . maturin pydicom

# Recompile tags.dmap from the checked-in resolved JSON-L:
cargo run --release --bin dmap-compile -- \
  --standard data/standard/attributes.json \
  --resolved data/resolved_pydicom_backfilled.jsonl \
  --out tags.dmap

# Run the full test suite:
cargo test --release --workspace
python tests/roundtrip_fuzz.py
```

## Re-scrape from PDFs (optional)

The PDF corpus is not checked in. If you have it in `data/pdfs/`:

```bash
python -m scraper.harvest_batch --vendor ge --jobs 4
python -m scraper.harvest_batch --vendor siemens --jobs 6
python -m scraper.harvest_batch --vendor philips --jobs 4
python -m scraper.resolve -o data/resolved.jsonl data/interim/*.jsonl
python -m scraper.pydicom_compare   # writes resolved_pydicom_backfilled.jsonl
```

## Status and roadmap

See [ROADMAP.md](ROADMAP.md) for current state and prioritised future work,
and [dicom-map/FORMAT.md](dicom-map/FORMAT.md) for the binary format and
versioning policy.

## License

MIT OR Apache-2.0.
# DICOM-Map

Build a single memory-mapped binary dictionary of **private DICOM tags** scraped
from vendor conformance statements (Siemens, GE, Philips, ...).

## Layout

```
scraper/            Python — PDF → JSON-L pipeline
  models.py           canonical data models (Pydantic)
  inspect_pdf.py      probe: classify a PDF before harvesting
  harvest/            per-vendor extractors
  standard.py         loads public PS3.6 tags (for validation)

data/
  standard/           public DICOM tag dictionary (PS3.6, one-time fetch)
  pdfs/               downloaded conformance PDFs  (gitignored)
  raw/                per-PDF raw extraction output (gitignored)
  interim/            normalized JSON-L              (gitignored)

compiler/           (future) Rust: JSON-L → .dmap frozen archive
dicom-map/          (future) Rust library: mmap-backed O(1) lookup
```

## Phases

| # | Phase                 | Status     |
|---|-----------------------|------------|
| 0 | Standard tag baseline | bootstrap  |
| 1 | PDF scraper (Python)  | in progress|
| 2 | Binary freeze (Rust)  | not started|

## Quick start

```bash
python -m venv .venv && source .venv/bin/activate
pip install -e .
python -m scraper.bootstrap          # fetches PS3.6 standard tags
python -m scraper.inspect_pdf <file> # probe an unknown PDF
```
