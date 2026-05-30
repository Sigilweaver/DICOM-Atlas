---
title: Rust quickstart
sidebar_label: Rust quickstart
---

# Rust quickstart

```toml
[dependencies]
dicom-map = "0.2"
```

```rust
use dicom_map::Dictionary;

fn main() -> anyhow::Result<()> {
    let dict = Dictionary::open("tags.dmap")?;

    // Public tag
    let tag = dict.lookup(0x0008, 0x0005, None).unwrap();
    println!("{} {} {}", tag.vr(), tag.keyword(), tag.name());

    // Private tag (creator string required)
    let tag = dict.lookup(
        0x0021,
        0x0008,
        Some("Siemens: Thorax/Multix FD Lab Settings"),
    ).unwrap();
    println!("{} {}", tag.vr(), tag.name());

    Ok(())
}
```

## Embedded dictionary (no file path)

```toml
[dependencies]
dicom-map = { version = "0.2", features = ["embedded"] }
```

```rust
use dicom_map::Dictionary;

let dict = Dictionary::embedded();   // baked-in tags.dmap
let tag = dict.lookup(0x0008, 0x0005, None).unwrap();
```

The `embedded` feature requires `DMAP_EMBED_PATH=/abs/path/to/tags.dmap`
at build time, or defaults to the `tags.dmap` next to the workspace
root.

## API

See the rustdoc on [docs.rs/dicom-map](https://docs.rs/dicom-map)
for the full surface, including iteration, the `Tag` accessor
methods, and error types.
