use std::fmt;

/// Which structural check an archive failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArchiveCheck {
    /// Header `body_len` differs from the bytes present.
    BodyLen,
    /// The section directory does not fit in the file.
    SectionDirectory,
    /// A required section is absent.
    MissingSection,
    /// A section id appears twice.
    DuplicateSection,
    /// A section's declared entry size is smaller than this reader needs, or
    /// its length is not a multiple of it.
    EntrySize,
    /// A section lies outside the file.
    SectionRange,
    /// Index entries are not strictly increasing by key.
    IndexOrder,
    /// An index entry points past the record array.
    RecordIndex,
    /// A string range lies outside the string pool.
    StringRange,
    /// A string is not valid UTF-8 (or a VR is not printable ASCII).
    Text,
    /// An index key disagrees with its record.
    KeyMismatch,
    /// An odd-group record has no private creator.
    CreatorMissing,
    /// A value multiplicity is inconsistent.
    Vm,
    /// The metadata section is malformed.
    Meta,
}

impl fmt::Display for ArchiveCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BodyLen => "body length",
            Self::SectionDirectory => "section directory",
            Self::MissingSection => "missing section",
            Self::DuplicateSection => "duplicate section",
            Self::EntrySize => "entry size",
            Self::SectionRange => "section range",
            Self::IndexOrder => "index order",
            Self::RecordIndex => "record index",
            Self::StringRange => "string range",
            Self::Text => "text",
            Self::KeyMismatch => "key mismatch",
            Self::CreatorMissing => "creator missing",
            Self::Vm => "value multiplicity",
            Self::Meta => "metadata",
        })
    }
}

/// Why a dictionary could not be opened.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DmapError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("file too small ({len} bytes) to contain a header")]
    TooSmall { len: usize },
    #[error("bad magic bytes {got:?}")]
    BadMagic { got: [u8; 4] },
    /// The file's format major is not the one this reader implements. An
    /// older major must be regenerated with `dmap-compile`; a newer one needs
    /// a newer reader. Unknown minors are accepted and never produce this.
    #[error(
        "unsupported .dmap format {got_major}.{got_minor} (this reader reads major {supported_major}); \
         regenerate the file with dmap-compile or upgrade the reader"
    )]
    UnsupportedVersion {
        got_major: u16,
        got_minor: u16,
        supported_major: u16,
    },
    #[error("archive integrity check failed ({check}): {detail}")]
    BadArchive { check: ArchiveCheck, detail: String },
}

impl DmapError {
    pub(crate) fn bad(check: ArchiveCheck, detail: impl Into<String>) -> Self {
        DmapError::BadArchive {
            check,
            detail: detail.into(),
        }
    }
}
