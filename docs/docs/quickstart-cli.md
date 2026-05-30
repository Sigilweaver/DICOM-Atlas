---
title: CLI quickstart
sidebar_label: CLI quickstart
---

# CLI quickstart

```sh
# Public tag
dicom-lookup 0008 0005
# -> (0008,0005) CS SpecificCharacterSet

# Private tag (creator string required)
dicom-lookup 0021 xx08 "Siemens: Thorax/Multix FD Lab Settings"

# JSON output for piping
dicom-lookup --json 0021 xx01 GEMS_XR3DCAL_01
```

For private tags, pass the **block offset** (low byte) as the
element argument, not the resolved element. The lookup logic
re-derives the actual element from the per-file private creator
mapping.

## Embedded mode (no external file)

```sh
cargo install dmap-compiler --features embedded
dicom-lookup 0008 0005   # works with no tags.dmap on disk
```

## Use a custom dictionary

```sh
dicom-lookup --file /path/to/my.dmap 0021 xx08 "MY VENDOR"
```

## Compile your own .dmap

```sh
dmap-compile \
    --standard data/standard/attributes.json \
    --resolved my_resolved_tags.jsonl \
    --out my.dmap
```

See the [format reference](./format.md) for the on-disk schema.
