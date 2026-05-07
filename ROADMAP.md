# DICOM-Map Roadmap

Current state (May 2026): 19,521 tags (14,392 private + 5,129 public), covering GE,
Siemens, Philips, Canon Medical, Acuson, and 200+ smaller vendors. Sources: 1,840
conformance statement PDFs (5,128 PDF-sourced private tags) plus pydicom/GDCM ingest
(9,264 additional entries). Binary artifact `tags.dmap` is 4.1 MB. CLI, Python
bindings (with pydicom adapter), C FFI, CI pipeline, and GitHub Release job are all
shipped.

See CHANGELOG.md for completed work.

---

## Data quality

### Description coverage

86% of private tags (roughly 12,000 / 14,000) have no description:

- pydicom-only entries (~9,600): 100% missing by construction - pydicom's private
  dictionary carries names and VRs but not descriptions.
- PDF-sourced entries (~4,300): ~56% missing. The remainder come from PDFs whose
  tables have a description column that the harvester did not extract, or from PDFs
  in unsupported table formats.

The GE variant-C harvester (`scraper/harvest/ge.py`) is implemented and handles the
six-column Senographe format. Interim files for affected GE PDFs must still be
regenerated:

    uv run python3 -m scraper.harvest_batch --force --vendor ge

Remaining gap:

- The pydicom-only gap requires identifying non-PDF sources (Siemens Confluence,
  online documentation).
- Roughly 213 zero-yield GE PDFs use PACS/RIS narrative layouts with low tag density.

---

## Vendor coverage

### Siemens

Siemens PDFs are the largest vendor subset (2,037 tags). The existing scraper handles
the standard `c2-*` document family. Remaining gaps:

- `syngo.*` product line PDFs (different table styles).
- Older `somatom`, `AXIOM`, and `FLUOROSPOT` PDFs that produce zero yield despite
  containing tag tables.

### Hitachi / Fujifilm / Hologic / Mindray

Smaller vendors with public conformance PDFs not covered by pydicom. Requires PDF
acquisition before harvester work can begin.

---

## Validation

All planned vendor integration tests are complete. See CHANGELOG.md.

---

## Schema and API

### Multi-version / history support

Some creator strings (e.g. Siemens `c2-025.*` vs `c2-028.*`) encode a software
version. The current schema collapses all versions of a creator into one entry. A
future schema could store per-version records and let the caller pass a
software-version hint. This is a breaking schema change.

### Online update mechanism

Allow `tags.dmap` to be refreshed without rebuilding from source, e.g. a signed
incremental patch format or a URL consumers can poll. Depends on a stable versioned
schema.
