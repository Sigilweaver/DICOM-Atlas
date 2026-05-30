---
title: Data sources
sidebar_label: Data sources
---

# Data sources

DICOM-Atlas compiles tags from three categories of source.

## 1. Vendor conformance PDFs (PDF-verified)

Scraped under the [`scraper/`](https://github.com/Sigilweaver/DICOM-Atlas/tree/main/scraper)
pipeline. Each entry carries a citation back to the originating
PDF (title, vendor, version, source URL).

| Vendor                          | Archive                                                                          |
| ------------------------------- | -------------------------------------------------------------------------------- |
| Siemens (MR, CT, XR)            | `archive.org/details/dicom-conformance-siemens`                                  |
| GE HealthCare                   | `archive.org/details/dicom-conformance-ge`                                       |
| Philips Healthcare              | `archive.org/details/dicom-conformance-philips`                                  |
| Canon Medical (incl. Toshiba)   | `archive.org/details/dicom-conformance-canon`                                    |
| Acuson / Siemens ultrasound     | `archive.org/details/dicom-conformance-siemens` (Acuson product family)          |

Provenance is preserved per-entry and surfaced in `Tag::source()`.

## 2. PS3.6 public dictionary

The PS3.6 attribute list is parsed from the official DICOM
Standard JSON at `dicomstandard.org`, vendored under
`data/standard/attributes.json`. Refreshed manually when the
standard publishes a new edition; the version is recorded in the
[CHANGELOG](https://github.com/Sigilweaver/DICOM-Atlas/blob/main/CHANGELOG.md).

## 3. Community contributions (pydicom / GDCM)

Where a private tag has no vendor PDF, but is well-known from the
[pydicom](https://github.com/pydicom/pydicom) `_private_dict.py`
or the [GDCM](https://gdcm.sourceforge.net) private dictionary,
the entry is included with `source = "pydicom"` or `source =
"gdcm"`. PDF-sourced entries take precedence on overlapping keys.

## Licensing

- Code: Apache-2.0 (see [LICENSE](https://github.com/Sigilweaver/DICOM-Atlas/blob/main/LICENSE)).
- Data (`tags.dmap`, `tags.csv`, the JSONL snapshots, the compiled
  dictionary): CC-BY-SA-4.0 (see [LICENSE-DATA](https://github.com/Sigilweaver/DICOM-Atlas/blob/main/LICENSE-DATA)).
- Conformance PDFs themselves remain copyright their respective
  vendors and are not redistributed by this project. The archive
  links above are independently hosted on archive.org.
