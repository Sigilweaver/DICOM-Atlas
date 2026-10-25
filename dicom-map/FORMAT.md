# `.dmap` binary format - versioning policy

The `.dmap` file format is a compact, memory-mapped DICOM tag dictionary.
This document describes how the format evolves and what consumers must do
to stay compatible.

## Layout

```text
+----------------------------------------------+
| FileHeader (32 bytes, little-endian)         |
|   magic:        b"DMAP"                      |
|   version:      u16  (currently 4)           |
|   _reserved:    [u8; 2]  (must be 0)         |
|   body_len:     u64                          |
|   body_sha256_lo: u64  (reserved, must be 0) |
|   _pad:         [u8; 8]  (must be 0)         |
+----------------------------------------------+
| rkyv-archived Dictionary                     |
|   .index   : Vec<IndexEntry>  (sorted)       |
|   .records : Vec<TagRecord>                  |
|   .strings : Vec<u8>                         |
+----------------------------------------------+
```

All integer fields in the header are little-endian. The body is produced by
[rkyv](https://rkyv.org) and validated via `rkyv::access` on load.

## Version field

`FileHeader::version` is the single source of truth. The current value is
**`VERSION = 4`** (`dicom_map::schema::VERSION`).

## Compatibility contract

### Patch (no version bump)
Changes that do NOT change the bit-level layout of `FileHeader`,
`IndexEntry`, `TagRecord`, `VrCode`, or the string-pool encoding.

Safe to do at any time:
- Adding new tag entries (public or private).
- Correcting existing entries (name, VR, description, keyword).
- Changing the `.strings` encoding in an additive way that remains valid
  UTF-8 at the existing offsets (e.g. appending new strings).

### Breaking (MUST bump `VERSION`)
Any change that would cause an older reader to misinterpret a field:

- Adding, removing, reordering, or resizing fields in `FileHeader`,
  `IndexEntry`, or `TagRecord`.
- Adding variants to `VrCode` at positions that clash with previously
  reserved discriminants, or changing its `repr(u8)`.
- Changing the string-offset type from `u32` to something else.
- Changing the hash function for `creator_hash` (currently FNV-1a 32).
- Changing creator canonicalisation (currently uppercase + single-spaced).

When `VERSION` bumps:

1. Increment `VERSION` in `dicom-map/src/schema.rs`.
2. Bump the minor version of the `dicom-map` crate.
3. Regenerate `tags.dmap`.
4. Bump `DMAP_ABI_VERSION` in `dicom-map-ffi/include/dicom_map.h` if the C
   struct layout changed.
5. Update this document with a short change note below.

## Reader behaviour

Readers MUST:

- Validate `magic == b"DMAP"`.
- Validate `version == VERSION` (currently `4`).
- Validate `body_len` equals the number of bytes after the header, and that
  `_reserved`, `body_sha256_lo` and `_pad` are zero.
- Validate the archive and its invariants before the first lookup:
  - `.index` is strictly sorted by `(group, element, creator_hash)` with no
    duplicate keys (lookup is a binary search on that order).
  - Every `record_idx` is below `records.len()`.
  - Every `(off, len)` string range lies inside `.strings` and is UTF-8.
  - Each record's `(group, element)` and the hash of its stored creator equal
    its index key (a creator hash of 0 means no creator).
  - No odd-group record lacks a creator.
- On a lookup hit with a creator, compare the canonicalised query with the
  record's stored creator; the 32-bit hash alone does not identify a creator.
- Fail loudly (don't silently continue) if either check fails.

The provided `DmapDict::open` and `DmapDict::from_static` functions enforce
this. They return `DmapError::UnsupportedVersion { got }` on a version
mismatch and `DmapError::BadArchive` with the failed check otherwise.

## History

| Version | Date       | Notes                                                       |
|---------|------------|-------------------------------------------------------------|
| 1       | 2026-04    | Initial format. 32-byte header, rkyv-archived Dictionary.   |
| 2       | 2026-04    | Added `sources_off: u32` and `sources_len: u32` to `TagRecord`. Stores pipe-delimited source PDF filenames (with `#pN` page anchors) in the shared string pool. Empty for public PS3.6 tags. |
| 3       | 2026-10    | rkyv 0.8 archive layout, little-endian, aligned primitives, 32-bit pointers. Requires regeneration of version 1 and 2 files. Never released. |
| 4       | 2026-10    | `TagRecord.vm_min` and `vm_max` widen to `u16` (`0xFFFF` is `n`) and `vm_step: u16` is added; `VrCode` gains `UsOrSs`, `ObOrOw`, `UsOrOw`, `UsOrSsOrOw`. Version 3 was never released, so 0.3.0 ships version 4. |

## Migration to version 3 and later

The checked-in `tags.dmap` and external `.dmap` files are persisted data.
Current readers reject version 1, 2 and 3 files before archive validation;
older readers reject newer files. There is no backward-compatible reader.
Do not change only the header of an old archive: the body layout differs.

Regenerate from the original inputs with the new compiler, for example:

```sh
cargo run --bin dmap-compile -- \
  --standard data/standard/attributes.json \
  --resolved data/resolved_pydicom_backfilled.jsonl --out tags.dmap
```
