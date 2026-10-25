//! Embedded dictionary - available with the `embedded` feature.
//!
//! The bytes of `tags.dmap` at workspace root are baked into the compiled
//! binary via `include_bytes!`. The build fails if `tags.dmap` is missing.

use crate::DmapDict;

/// Raw bytes of the embedded dictionary.
pub const TAGS_DMAP: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../tags.dmap"));

/// Returns a validated [`DmapDict`] backed by the embedded bytes. Panics if
/// the bytes are malformed, which means the build embedded a bad file.
pub fn embedded() -> DmapDict {
    DmapDict::from_static(TAGS_DMAP).expect("embedded tags.dmap failed validation")
}
