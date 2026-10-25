# `.dmap` binary format

A `.dmap` file is a memory-mappable DICOM tag dictionary. This document is the
byte-level specification and the compatibility rule between writers and
readers. The layout is read and written by hand; no serialization framework is
involved.

All integers are little-endian. Nothing requires alignment, so a reader may
work on any byte slice.

## Layout

```text
+----------------------------------------------+
| Header (32 bytes)                            |
|   0   magic            b"DMAP"               |
|   4   format_major     u16  (currently 4)    |
|   6   format_minor     u16  (currently 0)    |
|   8   body_len         u64  file length - 32 |
|   16  section_count    u32                   |
|   20  header_flags     u32  (0; ignored)     |
|   24  reserved         u64  (0; ignored)     |
+----------------------------------------------+
| Section directory: section_count x 24 bytes  |
|   0   id          u32                        |
|   4   entry_size  u32  bytes per entry       |
|   8   offset      u64  from start of file    |
|   16  len         u64  bytes                 |
+----------------------------------------------+
| Section data (writers pad to 8 bytes)        |
+----------------------------------------------+
```

### Sections

| id | name    | entry_size | contents |
|----|---------|-----------:|----------|
| 1  | INDEX   | 12 (min)   | sorted lookup keys |
| 2  | RECORDS | 72 (min)   | one record per tag |
| 3  | STRINGS | 1          | UTF-8 string pool |
| 4  | META    | 1          | optional `key=value\n` lines (UTF-8) |

Sections 1 to 3 are required. Each id appears at most once. Unknown ids are
ignored.

**INDEX entry:** `group u16, element u16, creator_hash u32, record_idx u32`.
Entries are strictly increasing by `(group, element, creator_hash)`.
`creator_hash` is 0 for no creator.

**RECORD:**

| offset | size | field |
|-------:|-----:|-------|
| 0  | 2 | group |
| 2  | 2 | element (the block offset, low byte, for private tags) |
| 4  | 4 | flags: bit 0 retired, bit 1 element is a block offset; other bits ignored |
| 8  | 2 | vm_min |
| 10 | 2 | vm_max (`0xFFFF` is `n`; with vm_min 0 and vm_max 0 the standard declares no VM) |
| 12 | 2 | vm_step (`k` for `k-kn`, else 1) |
| 14 | 2 | padding |
| 16 | 16 | VR: 1 to 16 printable ASCII bytes, NUL padded. A two-letter VR, `US or SS`, `OB or OW`, `US or OW`, `US or SS or OW`, or `??` when the source declares none |
| 32 | 8 | keyword (offset u32, length u32 into STRINGS) |
| 40 | 8 | name |
| 48 | 8 | canonical creator (empty for public tags) |
| 56 | 8 | description |
| 64 | 8 | sources (pipe-delimited PDF names with `#pN` anchors) |

**META:** lines of `key=value`. The compiler writes `standard_edition` (the
PS3.6 edition of the public tags) and `standard_source`.

### Creator canonicalisation and hash

The canonical creator is the string with runs of ASCII whitespace collapsed to
one space, leading and trailing whitespace removed, and ASCII letters
uppercased. Other bytes, including non-ASCII characters, are unchanged. The
hash is FNV-1a 32 over the canonical UTF-8 bytes. Both are part of the format.
The hash is only a filter: a lookup also compares the canonical creator string
stored in the record.

## Compatibility rule

The header holds `format_major` and `format_minor`.

- A reader accepts files with its own major and **any** minor. It ignores
  unknown sections, unknown flag bits, header flags, the reserved word, and
  record or index bytes beyond the sizes it knows (`entry_size` gives the
  stride).
- A reader rejects any other major with
  `UnsupportedVersion { got_major, got_minor, supported_major }`. For an older
  major the remedy is to regenerate the file with `dmap-compile`; for a newer
  one, a newer reader.
- Check order: magic, then major, then the rest of the file.
- A writer bumps the **minor** for additions an older reader of the same major
  can safely ignore: a new section, fields appended to INDEX or RECORD entries
  (with a larger `entry_size`), new flag bits, new VR spellings, new META keys.
- A writer bumps the **major** for anything such a reader would misread:
  changing or removing existing fields, their meaning, the canonicalisation or
  the hash.

## Reader validation

Opening a file checks, and fails with `DmapError::BadArchive { check, .. }`:

- `body_len` equals the number of bytes after the header (`BodyLen`).
- The directory fits in the file and every section lies inside the file
  (`SectionDirectory`, `SectionRange`); required sections exist and ids are not
  repeated (`MissingSection`, `DuplicateSection`).
- Each section's `entry_size` is at least what the reader needs and divides its
  length (`EntrySize`).
- INDEX keys are strictly increasing (`IndexOrder`).
- Every `record_idx` is below the record count (`RecordIndex`).
- Every string range lies inside STRINGS and is UTF-8 (`StringRange`, `Text`);
  VR bytes are printable ASCII (`Text`).
- Each record's group, element and creator hash equal its index key
  (`KeyMismatch`), and no odd-group record lacks a creator (`CreatorMissing`).
- VM bounds are consistent (`Vm`) and META lines contain `=` (`Meta`).

Failures are errors at open time; lookups afterwards cannot panic on a file
that passed. A lookup miss is `None`.

## `tags.csv`

`tags.csv` is the human-editable form read by `dmap-compile --csv`: columns
`group,element,private_creator,vr,vm,keyword,name,description,retired,vendors,sources`,
group as 4 hex digits, element as 4 hex digits or `xx` and 2 hex digits,
retired as `Y` or `N`, VR and VM as the compiler's strict grammar accepts.

## History

| Format | Date    | Notes |
|--------|---------|-------|
| 1      | 2026-04 | Initial format. Derived struct layout of the Rust types, 32-byte header. |
| 2      | 2026-04 | Added `sources`. |
| 3      | 2026-10 | Different serialization framework layout. Never released. |
| 4.0    | 2026-10 | Hand-laid layout described above: section directory, major/minor header, flags word, 16-bit VM bounds and step, VR as ASCII text, META section, ASCII-only creator canonicalisation. Files of formats 1 to 3 must be regenerated. |

## Regenerating

```sh
cargo run --release --bin dmap-compile -- \
  --standard data/standard/attributes.json \
  --resolved data/resolved_pydicom_backfilled.jsonl \
  --meta standard_edition=2024b \
  --meta standard_source=innolitics/dicom-standard@8670abdd \
  --out tags.dmap
```
