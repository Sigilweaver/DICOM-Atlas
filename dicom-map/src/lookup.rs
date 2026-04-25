//! Reader: mmap a `.dmap` file and perform O(log n) lookups.

use std::fs::File;
use std::path::Path;

use memmap2::Mmap;
use rkyv::{archived_root, check_archived_root};

use crate::schema::{
    creator_hash as compute_creator_hash, ArchivedDictionary, ArchivedTagRecord, ArchivedVrCode,
    Dictionary, FileHeader, MAGIC, VERSION,
};

#[derive(Debug, thiserror::Error)]
pub enum DmapError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("file too small ({0} bytes) to contain a header")]
    TooSmall(usize),
    #[error("bad magic bytes {got:?}")]
    BadMagic { got: [u8; 4] },
    #[error("unsupported version {got}")]
    UnsupportedVersion { got: u16 },
    #[error("archive integrity check failed: {0}")]
    BadArchive(String),
}

pub struct DmapDict {
    // When loaded from disk we hold an mmap. When constructed from a byte
    // slice (e.g. `include_bytes!`), the slice itself is the backing store.
    backing: Backing,
    body_off: usize,
}

enum Backing {
    Mmap { _file: File, mmap: Mmap },
    Static(&'static [u8]),
}

impl Backing {
    fn bytes(&self) -> &[u8] {
        match self {
            Backing::Mmap { mmap, .. } => &mmap[..],
            Backing::Static(b) => b,
        }
    }
}

impl DmapDict {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DmapError> {
        let file = File::open(path)?;
        // SAFETY: file is held for the life of the mapping.
        let mmap = unsafe { Mmap::map(&file)? };

        if mmap.len() < FileHeader::SIZE {
            return Err(DmapError::TooSmall(mmap.len()));
        }
        let hdr = FileHeader::from_bytes(&mmap[..FileHeader::SIZE])
            .ok_or(DmapError::TooSmall(mmap.len()))?;
        if &hdr.magic != MAGIC {
            return Err(DmapError::BadMagic { got: hdr.magic });
        }
        if hdr.version != VERSION {
            return Err(DmapError::UnsupportedVersion { got: hdr.version });
        }

        let body = &mmap[FileHeader::SIZE..];
        check_archived_root::<Dictionary>(body)
            .map_err(|e| DmapError::BadArchive(e.to_string()))?;

        Ok(Self {
            backing: Backing::Mmap { _file: file, mmap },
            body_off: FileHeader::SIZE,
        })
    }

    /// Construct from a `'static` byte slice, typically `include_bytes!`.
    pub fn from_static(bytes: &'static [u8]) -> Result<Self, DmapError> {
        if bytes.len() < FileHeader::SIZE {
            return Err(DmapError::TooSmall(bytes.len()));
        }
        let hdr = FileHeader::from_bytes(&bytes[..FileHeader::SIZE])
            .ok_or(DmapError::TooSmall(bytes.len()))?;
        if &hdr.magic != MAGIC {
            return Err(DmapError::BadMagic { got: hdr.magic });
        }
        if hdr.version != VERSION {
            return Err(DmapError::UnsupportedVersion { got: hdr.version });
        }
        let body = &bytes[FileHeader::SIZE..];
        check_archived_root::<Dictionary>(body)
            .map_err(|e| DmapError::BadArchive(e.to_string()))?;
        Ok(Self {
            backing: Backing::Static(bytes),
            body_off: FileHeader::SIZE,
        })
    }

    fn archived(&self) -> &ArchivedDictionary {
        // SAFETY: validated in `open()` / `from_static()`.
        unsafe { archived_root::<Dictionary>(&self.backing.bytes()[self.body_off..]) }
    }

    pub fn len(&self) -> usize {
        self.archived().index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn lookup(&self, group: u16, element: u16, creator: Option<&str>) -> Option<TagView<'_>> {
        let ch = compute_creator_hash(creator);
        let dict = self.archived();
        let index = &dict.index;

        let key = (group, element, ch);
        let idx = index
            .binary_search_by(|e| (e.group, e.element, e.creator_hash).cmp(&key))
            .ok()?;

        let rec_idx = index[idx].record_idx as usize;
        let rec = &dict.records[rec_idx];
        Some(TagView {
            rec,
            strings: dict.strings.as_slice(),
        })
    }
}

pub struct TagView<'a> {
    rec: &'a ArchivedTagRecord,
    strings: &'a [u8],
}

impl<'a> TagView<'a> {
    fn slice(&self, off: u32, len: u32) -> &'a str {
        let s = off as usize;
        let e = s + len as usize;
        std::str::from_utf8(&self.strings[s..e]).unwrap_or("")
    }

    pub fn group(&self) -> u16 {
        self.rec.group
    }

    pub fn element(&self) -> u16 {
        self.rec.element
    }

    pub fn keyword(&self) -> &'a str {
        self.slice(self.rec.keyword_off, self.rec.keyword_len as u32)
    }

    pub fn name(&self) -> &'a str {
        self.slice(self.rec.name_off, self.rec.name_len as u32)
    }

    pub fn creator(&self) -> &'a str {
        let len = self.rec.creator_len;
        if len == 0 {
            return "";
        }
        self.slice(self.rec.creator_off, len as u32)
    }

    pub fn description(&self) -> &'a str {
        self.slice(self.rec.description_off, self.rec.description_len)
    }

    pub fn vr(&self) -> &'static str {
        archived_vr_as_str(&self.rec.vr)
    }

    pub fn retired(&self) -> bool {
        self.rec.retired
    }

    pub fn is_block_offset(&self) -> bool {
        self.rec.element_is_block_offset
    }
}

fn archived_vr_as_str(v: &ArchivedVrCode) -> &'static str {
    match v {
        ArchivedVrCode::AE => "AE",
        ArchivedVrCode::AS => "AS",
        ArchivedVrCode::AT => "AT",
        ArchivedVrCode::CS => "CS",
        ArchivedVrCode::DA => "DA",
        ArchivedVrCode::DS => "DS",
        ArchivedVrCode::DT => "DT",
        ArchivedVrCode::FL => "FL",
        ArchivedVrCode::FD => "FD",
        ArchivedVrCode::IS => "IS",
        ArchivedVrCode::LO => "LO",
        ArchivedVrCode::LT => "LT",
        ArchivedVrCode::OB => "OB",
        ArchivedVrCode::OD => "OD",
        ArchivedVrCode::OF => "OF",
        ArchivedVrCode::OL => "OL",
        ArchivedVrCode::OV => "OV",
        ArchivedVrCode::OW => "OW",
        ArchivedVrCode::PN => "PN",
        ArchivedVrCode::SH => "SH",
        ArchivedVrCode::SL => "SL",
        ArchivedVrCode::SQ => "SQ",
        ArchivedVrCode::SS => "SS",
        ArchivedVrCode::ST => "ST",
        ArchivedVrCode::SV => "SV",
        ArchivedVrCode::TM => "TM",
        ArchivedVrCode::UC => "UC",
        ArchivedVrCode::UI => "UI",
        ArchivedVrCode::UL => "UL",
        ArchivedVrCode::UN => "UN",
        ArchivedVrCode::UR => "UR",
        ArchivedVrCode::US => "US",
        ArchivedVrCode::UT => "UT",
        ArchivedVrCode::UV => "UV",
        ArchivedVrCode::Unknown => "??",
    }
}
