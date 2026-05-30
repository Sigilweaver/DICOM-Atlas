---
title: Install
---

# Install

## Python

```sh
pip install dicom-map
```

Pre-built wheels are published for CPython 3.8 - 3.13 on Linux
(`manylinux` x86_64 + aarch64), macOS (x86_64 + arm64), and
Windows (x64). The bundled `tags.dmap` is available via
`dicom_map.bundled_dmap_path()`.

## Rust

```toml
[dependencies]
dicom-map = "0.2"
```

## CLI

```sh
cargo install dmap-compiler   # provides `dicom-lookup` and `dmap-compile`
dicom-lookup --help
```

For an offline single-binary that ships the dictionary baked in:

```sh
git clone https://github.com/Sigilweaver/DICOM-Atlas
cd DICOM-Atlas
cargo build --release --bin dicom-lookup --features embedded -p dmap-compiler
./target/release/dicom-lookup 0008 0005
```

## C / C++

```sh
cargo build --release -p dicom-map-ffi
# produces target/release/libdicom_map_ffi.{so,a}
# header: dicom-map-ffi/include/dicom_map.h
```

## tags.dmap directly

Download the latest `tags.dmap` from the
[GitHub Releases](https://github.com/Sigilweaver/DICOM-Atlas/releases)
page. Verify against the `SHA256SUMS` published alongside.

## MSRV

Rust 1.87.
