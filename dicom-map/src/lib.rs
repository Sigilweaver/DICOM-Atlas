pub mod lookup;
pub mod schema;

#[cfg(feature = "embedded")]
pub mod embedded;

pub use lookup::{DmapDict, DmapError, TagView};
pub use schema::{
    canonicalize_creator, creator_hash, fnv1a32, Dictionary, FileHeader, IndexEntry, TagRecord,
    VrCode, MAGIC, VERSION,
};
