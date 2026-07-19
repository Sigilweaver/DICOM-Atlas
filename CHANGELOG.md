# Changelog

All notable changes to this project will be documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Docs: Python API reference page covering the `dicom_map` module
  functions (`open`, `patch_pydicom`, `unpatch_pydicom`) and the `Dict`
  class, registered in the sidebar under "Use the dictionary". (thanks
  @Nabejo)

### Changed

- `dicom-map-py`: bump `pyo3` from 0.22 to 0.29, clearing RUSTSEC-2025-0020
  and RUSTSEC-2026-0177. Updated the `PyDict::new_bound` and `__exit__`
  parameter types for the 0.29 API (`Bound` is now the default; the
  `PyObject` type alias was removed in favour of `Py<PyAny>`). Removed the
  now-stale `--ignore` flags from the `cargo audit` workflow. (#3)
- `.github/workflows/ci.yml`: added `windows-latest` and `macos-14` to the
  test matrix, matching the wheel targets the release workflow already
  ships for `dicom-map-py`. `fmt`/`clippy` still run once (Linux only) to
  avoid redundant CI time; the C FFI smoke test (which hard-codes
  gcc/`.so`/`LD_LIBRARY_PATH`) stays Linux-only since porting it to
  Windows/macOS is a separate effort from the parsing/reader coverage
  this change is meant to add. (#1)

## [0.2.8] - 2026-07-06

### Fixed

- Corrected the published package description: `dicom-map`/`dicom-map-py`
  claimed an O(1) lookup, but `Dictionary::lookup` uses `binary_search_by`
  over a sorted index, which is O(log n). Both crates.io and PyPI
  descriptions now say O(log n).

### Added

- `dicom-map-py`: `keywords` (`dicom`, `medical-imaging`, `tags`,
  `registry`, `rkyv`) added to `pyproject.toml` for PyPI search
  discoverability; the Rust crate already had them.

## [0.2.7] - 2026-07-04

### Added

- Release workflow now builds and publishes a source distribution
  (sdist) to PyPI alongside the wheels, with `LICENSE` bundled. Enables
  source-based installs and conda-forge packaging. (The `embedded`
  feature stays default-off, so source builds do not require
  `tags.dmap`.)

### Changed

- Realign the `dicom-map` PyPI package version with the crate version
  (both now 0.2.7); the wheel had lagged at 0.2.4.

## [0.2.6] - 2026-05-31

### Fixed

- `CITATION.cff`: change `type` from `dataset` to `software` and
  `license` from a YAML list to a plain `Apache-2.0` string. Zenodo's
  `cffconvert` integration does not handle `type: dataset` or a license
  list, causing "Citation metadata load failed" on both v0.2.4 and v0.2.5.
- `CITATION.cff`: remove `license-url` (not recognised by cffconvert).

## [0.2.5] - 2026-05-31

### Fixed

- `CITATION.cff`: split compound `Apache-2.0 AND CC0-1.0` into a CFF
  list; CFF v1.2.0 does not allow compound SPDX expressions and Zenodo
  rejected the v0.2.4 deposit with "Citation metadata load failed".
- `CITATION.cff`: correct stale `repository-code`, `url`, and
  `license-url` values left over from the `DicomAtlas` -> `DICOM-Atlas`
  organisation rename.
- `CITATION.cff`: bump version / date-released to reflect 0.2.4 release.

## [0.2.4] - 2026-05-31

### Added

- `CITATION.cff`: author identity (Nathan Riley + ORCID) and a
  scaffolded `identifiers:` block ready for the Zenodo concept DOI.

### Changed

- README badge block unified across the Sigilweaver portfolio.

## [0.2.3] - 2026-05-30

### Changed

- Drop the `macos-13` (Intel) wheel matrix entry. GitHub-hosted Intel
  macOS runners now have multi-hour queue times and block the release;
  Apple Silicon (`macos-14`) wheels remain.

## [0.2.2] - 2026-05-30

### Fixed

- Drop the maturin `sdist` job which cannot bundle the parent-directory
  `dicom-map` path dependency; PyPI publish now uses wheels only.
- Make `cargo publish` steps idempotent so retagging does not fail when
  a crate version is already on crates.io.

## [0.2.1] - 2026-05-30

### Fixed

- Add explicit `version = "0.2.1"` to internal `dicom-map` path deps so
  publishing the `dmap-compiler`, `dicom-map-ffi`, and `dicom-map-py`
  crates resolves correctly from crates.io.
- Add `generate-import-lib` to the `pyo3` feature set so Windows abi3
  wheels build under maturin without a Python interpreter present.

### Added

- `SECURITY.md` with private GHSA reporting policy.
- `CONTRIBUTING.md` with PR checklist and development setup.
- Docusaurus documentation site at `docs/`, published to
  <https://sigilweaver.app/dicom-atlas/docs/>.
- README badges (CI, license, MSRV, docs).

### Changed

- Workspace metadata fully consolidated under `[workspace.package]`:
  `authors`, `rust-version`, `repository`, `homepage`,
  `documentation`, `readme`, `keywords`, `categories`.
- `[lints.rust] unsafe_code` not forbidden: `dicom-map` requires
  `unsafe` for `Mmap::map` and `archived_root`, and `dicom-map-ffi`
  is a C FFI surface. Documented in the workspace manifest.

## [0.2.0] - 2026-05-31

Detailed historical changes for 0.2.0 and earlier are preserved in
the dated sections below.

---

## May 2026

### Per-vendor integration tests

`dicom-map/tests/integration.rs` extended with ground-truth spot checks for all
shipped vendor families: GE HealthCare (`GEMS_DL_IMG_01`, `GEMS_ACQU_01`), Philips
(`PHILIPS MR IMAGING DD 001`, `PHILIPS IMAGING DD 001`), Siemens
(`Siemens: Thorax/Multix FD Lab Settings`), Canon Medical (`CANON MDW NON-IMAGE`,
`CANON_MEC_MG3`), and Acuson / Siemens ultrasound (`SIEMENS ULTRASOUND SC2000`).
All values are PDF-verified. Existing 9 library tests unchanged.

### Embedded binary mode for dicom-lookup

`dicom-map` crate gains an `embedded` feature (already scaffolded, now working).
When the `compiler` crate is built with `--features embedded`, `dicom-lookup` bakes
`tags.dmap` into the binary at compile time. The binary works with no external file:

    cargo build --release --bin dicom-lookup --features embedded
    ./target/release/dicom-lookup 0008 0005   # no tags.dmap needed

The `--file` flag still overrides the embedded dictionary when given. The alignment
bug (rkyv requires 4-byte alignment; `include_bytes!` only guarantees 1-byte) is
fixed via an `#[repr(C, align(4))]` wrapper in `dicom-map/src/embedded.rs`.

### GE variant-C re-harvest

Interim files for GE Senographe mammography PDFs regenerated via:

    uv run python3 -m scraper.harvest_batch --force --vendor ge

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
