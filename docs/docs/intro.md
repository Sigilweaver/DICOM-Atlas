---
title: Introduction
sidebar_label: Intro
slug: /
---

# DICOM-Atlas

**An open registry of public and private DICOM tags.**

DICOM-Atlas compiles tag entries from vendor conformance statements
plus the PS3.6 public dictionary, with additional contributions from
[pydicom](https://github.com/pydicom/pydicom) and
[GDCM](https://gdcm.sourceforge.net), into a single frozen
memory-mapped archive (`tags.dmap`). The archive is queryable in
O(log n) with no runtime allocations from Rust, C, or Python.

## What you get

- **19,521 tags** in a single 3.9 MB file.
- **5,128 PDF-verified private tags** scraped from 1,840 vendor
  conformance PDFs (Siemens, GE, Philips, Canon, Acuson).
- **9,432 community-contributed private tags** from pydicom / GDCM.
- **5,129 public tags** from the PS3.6 standard.
- A single-binary `dicom-lookup` CLI.
- A `dicom-map` Rust crate, C FFI (`dicom-map-ffi`), and Python
  package (`dicom-map` on PyPI).

## Why

Most DICOM toolkits ship only the public dictionary. Vendor
private blocks - which carry the truly interesting per-scanner
metadata - are spread across thousands of pages of PDF. DICOM-Atlas
turns that haystack into a single hash-lookup.

## Status

The registry is **research / reference** data. It is not validated
for diagnostic use. Tag entries may contain errors, especially for
older or hand-transcribed vendor blocks; please open issues with
citations if you find one.

## Get started

- [Install](./install.md)
- [CLI quickstart](./quickstart-cli.md)
- [Rust quickstart](./quickstart-rust.md)
- [Python quickstart](./quickstart-python.md)
- [C FFI quickstart](./quickstart-c.md)
