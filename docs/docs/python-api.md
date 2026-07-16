---
title: Python API reference
sidebar_label: Python API reference
---

# Python API reference

The `dicom-map` wheel (package `dicom_map`) wraps the same mmap-backed
Rust dictionary used by the CLI, Rust crate, and C FFI. This page covers
the full exposed surface. See the [Python quickstart](./quickstart-python.md)
for a task-oriented walkthrough.

```python
import dicom_map
```

## Module functions

| Function | Signature | Description |
| --- | --- | --- |
| `open` | `open(path: str \| os.PathLike) -> Dict` | Open a compiled `.dmap` file. Raises `IOError` if the file is missing or corrupt, `ValueError` if the `.dmap` version is unsupported. |
| `patch_pydicom` | `patch_pydicom(dmap_path=None, *, csv_path=None, mode="fill") -> int` | Register private tag entries into pydicom's runtime `private_dictionaries`, resolving `dmap_path`'s or `csv_path`'s sibling `tags.csv`. `mode="fill"` (default) keeps pydicom's existing entries; `mode="override"` replaces them. Returns the number of entries added or replaced. |
| `unpatch_pydicom` | `unpatch_pydicom() -> int` | Reverse a previous `patch_pydicom` call, restoring pydicom's dictionary to its pre-patch state. Returns the number of entries removed. No-op if `patch_pydicom` was never called. |

```python
d = dicom_map.open("tags.dmap")

import pydicom
dicom_map.patch_pydicom("tags.dmap")
ds = pydicom.dcmread("scan.dcm")   # private tags now resolved via dicom-map
dicom_map.unpatch_pydicom()
```

## `Dict`

Opaque handle returned by `open()`. Holds an mmap over the `.dmap` file
for the lifetime of the object; there is no explicit `close()` since the
mapping is released on drop (`__enter__`/`__exit__` are supported as a
no-op convenience for `with` blocks).

| Member | Type | Description |
| --- | --- | --- |
| `lookup(group, element, creator=None)` | `(int, int, str \| None) -> dict \| None` | Look up a tag. `element` is the low byte (block offset) for private tags. Returns `None` if not found. |
| `__len__()` | `() -> int` | Number of entries in the dictionary. |
| `__repr__()` | `() -> str` | `<dicom_map.Dict len=N>` |

`lookup` returns a `dict` with stable keys: `group`, `element`, `creator`,
`keyword`, `name`, `vr`, `description`, `retired`, `block_offset`, `sources`
(a list of source identifiers backing the entry).

```python
d = dicom_map.open("tags.dmap")
len(d)                                                       # entry count

d.lookup(0x0008, 0x0005)                                     # public tag
d.lookup(0x0021, 0x0008, "Siemens: Thorax/Multix FD Lab Settings")
```

## Next

- [Python quickstart](./quickstart-python.md)
- [`.dmap` file format](./format.md)
