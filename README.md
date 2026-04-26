# DICOM-Atlas

An open registry of **public and private DICOM tags** compiled from vendor
conformance statements (GE, Siemens, Philips, ...) plus the PS3.6 standard,
with additional entries contributed by [pydicom](https://github.com/pydicom/pydicom)
and [GDCM](https://gdcm.sourceforge.net). Queryable from Rust, C, or Python
in O(log n) with no runtime dependencies.

Current shipped size: **19,688 tags** (14,559 private + 5,129 public) in a
**3.9 MB** `tags.dmap` file.

### Private tag breakdown

| Source | Tags | PDFs scraped |
|--------|-----:|-------------:|
| Siemens Healthineers (PDF scrape) | 1,951 | 439 |
| GE HealthCare (PDF scrape) | 1,762 | 257 |
| Philips Healthcare (PDF scrape) | 791 | 807 |
| Canon Medical (PDF scrape) | 410 | 284 |
| Acuson (PDF scrape) | 214 | 57 |
| **PDF-scraped subtotal** | **5,128** | **1,844** |
| pydicom / GDCM (community-compiled) | 9,472 | — |
| **Total private** | **14,559** | |
| Public (PS3.6 standard) | 5,129 | — |
| **Grand total** | **19,688** | |

## Quick start — use the dictionary

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

If you already use pydicom, register DICOM-Atlas's private dictionary into
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
Pass `mode="override"` to make DICOM-Atlas take precedence on conflicts, or
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

See [DEVELOPMENT.md](DEVELOPMENT.md) for the full repository layout, rebuild
instructions, re-scrape pipeline, and test suite guide.

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
- Private tags: 1,844 conformance statement PDFs from GE HealthCare, Siemens
  Healthineers, Philips Healthcare, Canon Medical, and Acuson (Siemens ultrasound).
  GE, Siemens, and Philips PDFs are archived at
  [archive.org/details/dicom-conformance-ge](https://archive.org/details/dicom-conformance-ge),
  [archive.org/details/dicom-conformance-siemens](https://archive.org/details/dicom-conformance-siemens), and
  [archive.org/details/dicom-conformance-philips](https://archive.org/details/dicom-conformance-philips).
  Canon PDFs are fetched directly from
  [global.medical.canon](https://global.medical.canon/service-support/Interoperability)
  and archived at
  [archive.org/details/dicom-conformance-canon](https://archive.org/details/dicom-conformance-canon).
  Acuson PDFs are fetched directly from the Siemens Healthineers product page.
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

See [ROADMAP.md](ROADMAP.md) for current state and planned work, and
[DEVELOPMENT.md](DEVELOPMENT.md) for the binary format spec and contributor
guide.

## License

| What | License |
|------|---------|
| Source code (`compiler/`, `dicom-map/`, `dicom-map-py/`, `dicom-map-ffi/`, `scraper/`) | Apache-2.0 |
| Original compiled data (entries in `tags.csv` / `tags.dmap` whose `sources` field references a PDF) | CC0 1.0 (public domain) |
| pydicom / GDCM-derived entries (`sources = ["pydicom"]`) | MIT (pydicom) + BSD-3-Clause (GDCM) — see `THIRD_PARTY_LICENSES.md` |

Full texts: [LICENSE](LICENSE) (Apache-2.0), [LICENSE-DATA](LICENSE-DATA) (CC0 1.0), [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
