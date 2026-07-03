# `.dmap` binary format - versioning policy

The `.dmap` file format is a compact, memory-mapped DICOM tag dictionary.
This document describes how the format evolves and what consumers must do
to stay compatible.

## Layout

```text
+----------------------------------------------+
| FileHeader (32 bytes, little-endian)         |
|   magic:        b"DMAP"                      |
|   version:      u16  (currently 1)           |
|   _reserved:    [u8; 2]                      |
|   body_len:     u64                          |
|   body_sha256_lo: u64  (integrity hint)      |
|   _pad:         [u8; 8]                      |
+----------------------------------------------+
| rkyv-archived Dictionary                     |
|   .index   : Vec<IndexEntry>  (sorted)       |
|   .records : Vec<TagRecord>                  |
|   .strings : Vec<u8>                         |
+----------------------------------------------+
```

All integer fields in the header are little-endian. The body is produced by
[rkyv](https://rkyv.org) and validated via `check_archived_root` on load.

## Version field

`FileHeader::version` is the single source of truth. The current value is
**`VERSION = 1`** (`dicom_map::schema::VERSION`).

## Compatibility contract

### Patch (no version bump)
Changes that do NOT change the bit-level layout of `FileHeader`,
`IndexEntry`, `TagRecord`, `VrCode`, or the string-pool encoding.

Safe to do at any time:
- Adding new tag entries (public or private).
- Correcting existing entries (name, VR, description, keyword).
- Changing the sort order of `.index` (readers don't rely on it - binary
  search is performed on the key, not the physical order).
- Changing the `.strings` encoding in an additive way that remains valid
  UTF-8 at the existing offsets (e.g. appending new strings).

### Breaking (MUST bump `VERSION`)
Any change that would cause a `v1` reader to misinterpret a field:

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
- Validate `version == VERSION` (currently `2`).
- Fail loudly (don't silently continue) if either check fails.

The provided `DmapDict::open` and `DmapDict::from_static` functions already
enforce this and return `DmapError::UnsupportedVersion { got }` on mismatch.

## History

| Version | Date       | Notes                                                       |
|---------|------------|-------------------------------------------------------------|
| 1       | 2026-04    | Initial format. 32-byte header, rkyv-archived Dictionary.   |
| 2       | 2026-04    | Added `sources_off: u32` and `sources_len: u32` to `TagRecord`. Stores pipe-delimited source PDF filenames (with `#pN` page anchors) in the shared string pool. Empty for public PS3.6 tags. |
