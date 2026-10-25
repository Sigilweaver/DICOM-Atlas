//! Memory-mapped DICOM tag dictionary (`.dmap`).
//!
//! Open a dictionary with [`DmapDict::open`] and look tags up with
//! [`DmapDict::lookup`]. The byte layout is documented in `FORMAT.md` and is
//! read and written by hand: no serialization framework appears in this
//! crate's API.
//!
//! Writing archives needs the `write` Cargo feature ([`DmapBuilder`]).

mod canon;
mod error;
mod format;
mod reader;
mod types;

#[cfg(feature = "write")]
mod writer;

#[cfg(feature = "embedded")]
pub mod embedded;

pub use canon::{canonicalize_creator, creator_hash};
pub use error::{ArchiveCheck, DmapError};
pub use format::{FORMAT_MAJOR, FORMAT_MINOR};
pub use reader::{DmapDict, OpenOptions, TagView};
pub use types::{TagFlags, Vm, VM_UNBOUNDED};

#[cfg(feature = "write")]
pub use writer::{BuildError, DmapBuilder, NewRecord};
