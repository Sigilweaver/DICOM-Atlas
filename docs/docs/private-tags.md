---
title: Private tags
sidebar_label: Private tags
---

# Private tags

## Block convention

DICOM private tags live in odd-numbered groups. Within a group, an
application reserves a "block" by writing a creator string into
element `(gggg, 00XX)` where `XX` is the block number
(`0x10` - `0xFF`). Tags inside the reserved block then use elements
`(gggg, XXYY)` where `YY` is the offset within the block.

To look a private tag up, you need three pieces of information:

1. The group `gggg`.
2. The **block offset** `0xYY` (low byte of the element).
3. The exact private creator string from the file.

```python
d.lookup(0x0021, 0x08, "Siemens: Thorax/Multix FD Lab Settings")
```

Not:

```python
d.lookup(0x0021, 0x1008, "Siemens: ...")   # WRONG, this is the resolved element
```

## Creator string canonicalization

Private creator strings are canonicalized via
[Unicode NFC normalization](https://www.unicode.org/reports/tr15/)
plus a lowercase fold before hashing into the index. Lookup
accepts the original (cased, possibly non-NFC) string.

## Block range note

This dictionary's compiler normalizes every private-tag element to
`element & 0xFF` so that all entries are keyed by block offset.
Some upstream sources stored the resolved element (`0x1008`) rather
than the offset (`0x08`); these are corrected at compile time and
deduplicated.

## Vendor coverage

| Vendor                                              | Tags  | PDFs |
| --------------------------------------------------- | ----: | ---: |
| Siemens Healthineers                                | 1,951 |  439 |
| GE HealthCare                                       | 1,762 |  257 |
| Philips Healthcare                                  |   791 |  807 |
| Canon Medical (incl. Toshiba)                       |   410 |  284 |
| Acuson / Siemens ultrasound                         |   214 |   53 |
| **PDF-scraped subtotal**                            | 5,128 | 1,840 |
| pydicom / GDCM community contributions              | 9,432 |    - |
| **Total private**                                   | 14,560 |    - |
| Public (PS3.6 standard)                             | 5,129 |    - |
| **Grand total**                                     | 19,521 |   - |

Counts reflect the post-dedup 0.2.0 dictionary.
