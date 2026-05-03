# Changelog

---

## May 2026

### Compiler: private-tag normalization and deduplication

`normalize_and_dedup()` added to `compiler/src/main.rs`, called before compilation.

Two bugs were causing silent lookup failures and incorrect results:

1. Some private-tag entries in `resolved_pydicom_backfilled.jsonl` had
   `element_is_block_offset=false` and stored concrete element addresses (e.g. 0x1011
   for block 0x10, offset 0x11). The lookup API always passes the block offset (low
   byte), so these tags were silently unfindable. Fix: normalize all private-tag
   elements to `element & 0xFF` before building the index.

2. Creator strings that differ only in case (e.g. "SPI RELEASE 1" vs "SPI Release 1")
   canonicalize to the same string and produce the same `creator_hash`. Both entries
   appeared at the same index key; `binary_search` returned either one unpredictably.
   Fix: deduplicate by `(group, element, creator_hash)` after normalization, keeping
   the entry with the most informative data (PDF source preferred over pydicom-only;
   non-"Unknown" name preferred over "Unknown").

167 duplicate rows removed; 5 round-trip fuzz test failures eliminated. Dictionary
reduced from 19,688 to 19,521 entries.

### Round-trip fuzz test

`tests/roundtrip_fuzz.py` added. Samples 500 deterministic private tags from
`resolved_pydicom_backfilled.jsonl` (seed `0xDEADBEEF`) and asserts name/VR match
via `dicom-lookup --json`. Run in CI on every push to `main`. Skips the name equality
check for rows with `name="Unknown"` since the compiler's dedup may have promoted a
more informative name for the same logical tag.

### CI pipeline

`.github/workflows/ci.yml` added. On every push to `main`:

1. Builds all Rust targets (`dmap-compile`, `dicom-lookup`, `dicom-map`).
2. Compiles a fresh `tags.dmap` from checked-in JSONL snapshots.
3. Checks that the compiled artifact is at least 3 MB (guards against empty output).
4. Runs `cargo test --release -p dicom-map` (9 unit/integration tests).
5. Runs `tests/roundtrip_fuzz.py` (500 samples).
6. Publishes a GitHub Release with `tags.dmap` and `SHA256SUMS` on pushes to `main`.

---

## April 2026

### Canon Medical / Toshiba scraper

`scraper/harvest/canon.py` and `scraper/crawl/canon.py` implemented. Handles Canon's
multi-column conformance table format. 284 PDFs harvested from `global.medical.canon`,
yielding 410 vendor-verified private tags. pydicom-only entries for Canon creators
upgraded to PDF-sourced provenance where confirmed. All 284 PDFs archived at
`archive.org/details/dicom-conformance-canon`. Toshiba Medical is covered via the
Canon Medical product family (Canon acquired Toshiba Medical in 2016); no separate
Toshiba scraper is needed.

### Acuson / Siemens ultrasound scraper

`scraper/harvest/acuson.py` implemented. Handles two Acuson table variants: the
Sequoia six-column format and the Juniper/NX four-column format. 53 conformance PDFs
processed, yielding 214 vendor-verified private tags. PDFs are hosted on the Siemens
Healthineers marketing portal and archived under
`archive.org/details/dicom-conformance-siemens`.

### GE variant-C harvester

`_parse_table_variant_c()` added to `scraper/harvest/ge.py`. Handles the six-column
Senographe mammography format (`Attribute name | Tag | Type | Attribute description |
VR | VM`) used in Senographe conformance PDFs (rev 40+). Creator state persists across
page boundaries. Validated on `ge_xr-mammo_gech-dicom-conformance-rev42-br-en.pdf`:
144 tags across 4 creators with VR, VM, and description all populated. Affected GE
interim files must be regenerated:

    uv run python3 -m scraper.harvest_batch --force --vendor ge

### Siemens and GE agreement rate investigation

Post-pydicom-backfill audit of `resolved_pydicom_backfilled.jsonl`. Both Siemens
(3,863 tags) and GE show ~99.4% agreement between PDF-sourced and pydicom entries on
overlapping tags. Previously reported figures (20% and 17%) were match-as-fraction-of-
total before the backfill expanded the entry counts. Remaining discrepancies are
isolated VR differences (CS vs LO, US vs SS/SL) within normal vendor variation.

---

## Prior

### Initial feature set shipped

- `dicom-lookup` CLI binary wrapping `DmapDict::lookup`.
- Python bindings (PyO3) exposing `DmapDict` as an extension module with a pydicom
  adapter.
- C FFI header (`dicom_map.h`) for C, C++, Java (JNA), and .NET (P/Invoke) consumers.
- GitHub Release asset: `tags.dmap` and `SHA256SUMS` attached to versioned releases.
- `.dmap` format versioned at VERSION=2; `DmapError::UnsupportedVersion` on mismatch.
- Keyword field for private tags: derived by CamelCasing the `name` field in the
  compiler; returned by Python bindings and CLI.
- VR conflict resolution: majority-vote across source PDFs, pydicom as tiebreaker on
  equal counts. Implemented in `scraper/resolve.py`.
