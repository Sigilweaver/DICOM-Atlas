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
# {'vr': 'US', 'name': 'Auto Window Flag', 'block_offset': True,
#  'sources': ['siemens_xr_c2-064.pdf#p41', ...], ...}
```

#### pydicom adapter

If you already use pydicom, register dicom-map's private dictionary into
pydicom so private tags resolve automatically with no other code changes:

```python
import dicom_map
import pydicom

dicom_map.patch_pydicom("tags.dmap")  # one-time at startup

ds = pydicom.dcmread("scan.dcm")
elem = ds[0x0021, 0x1008]
print(elem.name, elem.VR)             # resolved via dicom-map
```

By default `patch_pydicom` runs in `mode="fill"` — it only adds entries
pydicom doesn't already have, so existing pydicom data is never clobbered.
Pass `mode="override"` to make dicom-map take precedence on conflicts, or
call `dicom_map.unpatch_pydicom()` to revert.

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

The PDF corpus is not checked in. Download from archive.org using `sources.json`:

```bash
pip install -e ".[ops]"                      # installs requests (already present) + internetarchive
python scripts/fetch_from_archive.py         # downloads all vendor PDFs from archive.org
```

Then re-harvest and re-resolve:

```bash
python -m scraper.harvest_batch --vendor ge --jobs 4
python -m scraper.harvest_batch --vendor siemens --jobs 6
python -m scraper.harvest_batch --vendor philips --jobs 4
python -m scraper.resolve -o data/resolved.jsonl data/interim/*.jsonl
python -m scraper.pydicom_compare   # writes resolved_pydicom_backfilled.jsonl
```

## Data provenance and limitations

Private DICOM tags are **inherently undocumented by design** — the standard
deliberately leaves the `(gggg, xxxx, creator)` space to vendors, who are under
no obligation to publish or stabilise their tag definitions. Even official
conformance statements vary across product versions, may contradict each other,
and sometimes document tags that were quietly dropped or repurposed in later
firmware. This registry is a **best-effort compilation for non-critical
use cases** — it can help you understand what you're looking at in a DICOM file,
but it should **not** be the basis for clinical decisions, automated
de-identification, or any application where a wrong VR or stale name would cause
harm.

> **Notice:** The private tag data in this repository is extracted by automated
> parsing of vendor-published conformance statement PDFs. It is **not** an
> authoritative standard and comes with no warranty of completeness or accuracy.

**Sources:**
- Public tags: DICOM Standard Part 6 (PS3.6) via the
  [Innolitics JSON export](https://github.com/innolitics/dicom-standard).
- Private tags: 1,633 conformance statement PDFs from GE HealthCare, Siemens
  Healthineers, and Philips Healthcare, archived at
  [archive.org/details/dicom-conformance-ge](https://archive.org/details/dicom-conformance-ge),
  [archive.org/details/dicom-conformance-siemens](https://archive.org/details/dicom-conformance-siemens), and
  [archive.org/details/dicom-conformance-philips](https://archive.org/details/dicom-conformance-philips).
  Original vendor source URLs are in `data/sources.json`.

Each private tag record carries a `sources` field listing the specific PDF
file(s) (with page number anchors) that the definition was scraped from. This
is exposed at runtime via the lookup API — `TagView::sources()` in Rust and the
`"sources"` key in the Python dict — so you can always trace a tag back to the
document it came from.

**Known limitations:**
- Only PDFs in which the vendor explicitly tabulates private tag dictionaries
  are harvested (~21% of the corpus). Many conformance statements describe
  service classes but do not enumerate private tags — these are not gaps in our
  extraction, they simply contain nothing to extract.
- Some widely-used private tags (e.g. `(0019,100a)` `NumberOfImagesInMosaic`
  for Siemens MRI mosaics) were established by community reverse-engineering
  and do not appear in official conformance PDFs. They are absent from this
  registry.
- Where the same (group, element, creator) appears in multiple PDFs with
  conflicting VR types, the majority vote wins; the `vr_inferred` flag marks
  the small number of cases where no majority existed.
- Cross-referenced against [pydicom](https://github.com/pydicom/pydicom)'s
  private dictionary for validation; some VR values were backfilled or corrected
  where pydicom had higher-confidence data.
- Multi-vendor products (e.g. the Siemens/GE joint AdvantageSim RT planning
  system) can cause the same tag to appear in conformance PDFs from more than
  one vendor. The `vendors` field reflects all vendors whose documents reference
  a tag, not necessarily the vendor that originally defined it.

## Status and roadmap

See [ROADMAP.md](ROADMAP.md) for current state and prioritised future work,
and [dicom-map/FORMAT.md](dicom-map/FORMAT.md) for the binary format and
versioning policy.

## License

Apache-2.0.
