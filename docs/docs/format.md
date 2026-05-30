---
title: .dmap file format
sidebar_label: Format
---

# `.dmap` file format

The on-disk archive is built with [rkyv](https://rkyv.org/) and
is memory-mappable with no parsing step. See
[FORMAT.md](https://github.com/Sigilweaver/DICOM-Atlas/blob/main/dicom-map/FORMAT.md)
in the repo for the authoritative byte-level spec.

## Header

A small fixed-size header containing:

- 4-byte magic (`DMAP`).
- Format version (u16).
- Offset and length of the rkyv-archived `Dictionary` body.
- SHA-256 of the body for integrity verification.

## Body

The body is a single `Dictionary` value containing two
`Vec<TagEntry>` slices:

- A **public** slice keyed by `(group, element)` (sorted lex).
- A **private** slice keyed by `(group, element_offset,
  creator_hash)` (sorted lex).

Lookup is binary search; no hash table, no allocations.

## Tag entry

Each `TagEntry` carries: VR, VM, keyword, name, retired flag,
private-creator string (for private tags), and an optional
description.

## Alignment

rkyv requires 4-byte alignment on the body. The reader maps the
file with `Mmap::map` (which is page-aligned) and slices the body
out at the correct offset. For embedded mode the bytes are wrapped
in `#[repr(C, align(4))]` to satisfy the requirement after
`include_bytes!`.

## Versioning

Format version bumps are recorded in
[CHANGELOG.md](https://github.com/Sigilweaver/DICOM-Atlas/blob/main/CHANGELOG.md).
Old readers refuse to open a newer-format archive and vice versa.
