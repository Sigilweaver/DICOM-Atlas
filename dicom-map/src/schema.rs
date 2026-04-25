//! Shared schema for the `.dmap` archive file format.
//!
//! Layout on disk (all little-endian):
//!
//! ```text
//! +---------------------------------------+
//! | FileHeader (fixed, 32 bytes)          |
//! +---------------------------------------+
//! | rkyv-archived Dictionary              |
//! |   .index  : Vec<IndexEntry>  sorted   |
//! |   .records: Vec<TagRecord>            |
//! |   .strings: Vec<u8> pool              |
//! +---------------------------------------+
//! ```
//!
//! The index is sorted by `(group, element, creator_hash)` so lookup is a
//! plain binary search. String fields inside `TagRecord` are stored as
//! `(offset, len)` into `strings`; the reader slices without copying.

use rkyv::{Archive, Deserialize, Serialize};

pub const MAGIC: &[u8; 4] = b"DMAP";
pub const VERSION: u16 = 2;

/// 32-byte file prefix. Kept outside rkyv so we can sanity-check before
/// attempting to deserialize the body.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FileHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub _reserved: [u8; 2],
    pub body_len: u64,
    pub body_sha256_lo: u64, // first 8 bytes of body sha256 (integrity hint)
    pub _pad: [u8; 8],
}

impl FileHeader {
    pub const SIZE: usize = 32;

    pub fn new(body_len: u64) -> Self {
        Self {
            magic: *MAGIC,
            version: VERSION,
            _reserved: [0; 2],
            body_len,
            body_sha256_lo: 0,
            _pad: [0; 8],
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[0..4].copy_from_slice(&self.magic);
        out[4..6].copy_from_slice(&self.version.to_le_bytes());
        out[6..8].copy_from_slice(&self._reserved);
        out[8..16].copy_from_slice(&self.body_len.to_le_bytes());
        out[16..24].copy_from_slice(&self.body_sha256_lo.to_le_bytes());
        out[24..32].copy_from_slice(&self._pad);
        out
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&buf[0..4]);
        Some(Self {
            magic,
            version: u16::from_le_bytes([buf[4], buf[5]]),
            _reserved: [buf[6], buf[7]],
            body_len: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            body_sha256_lo: u64::from_le_bytes(buf[16..24].try_into().unwrap()),
            _pad: [0; 8],
        })
    }
}

/// One entry in the sorted lookup index.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, Copy)]
#[archive(check_bytes)]
pub struct IndexEntry {
    pub group: u16,
    pub element: u16,
    /// FNV-1a 32-bit hash of the canonical (uppercase, single-space) private
    /// creator string. `0` means "no private creator" (public tag).
    pub creator_hash: u32,
    /// Index into `records`.
    pub record_idx: u32,
}

/// One fully resolved tag record.
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct TagRecord {
    pub group: u16,
    pub element: u16,
    /// Whether `element` is the low byte of a private block offset (PS3.5 §7.8.1).
    pub element_is_block_offset: bool,
    pub retired: bool,
    pub vr: VrCode,
    pub vm_min: u8,
    pub vm_max: u8, // 0xFF == 'n' (unbounded)
    pub keyword_off: u32,
    pub keyword_len: u16,
    pub name_off: u32,
    pub name_len: u16,
    pub creator_off: u32, // 0 for public tags
    pub creator_len: u16,
    pub description_off: u32,
    pub description_len: u32,
    /// Pipe-delimited list of source PDF filenames (with `#pN` page anchors).
    /// Points into the shared string pool. Empty for public (PS3.6) tags.
    pub sources_off: u32,
    pub sources_len: u32,
}

/// DICOM value representation, 2-byte ASCII packed into one byte via a
/// fixed enum so records stay small.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[archive(check_bytes)]
#[repr(u8)]
pub enum VrCode {
    AE,
    AS,
    AT,
    CS,
    DA,
    DS,
    DT,
    FL,
    FD,
    IS,
    LO,
    LT,
    OB,
    OD,
    OF,
    OL,
    OV,
    OW,
    PN,
    SH,
    SL,
    SQ,
    SS,
    ST,
    SV,
    TM,
    UC,
    UI,
    UL,
    UN,
    UR,
    US,
    UT,
    UV,
    /// For rows where the VR couldn't be determined.
    Unknown = 0xFF,
}

impl VrCode {
    pub fn from_str2(s: &str) -> Self {
        match s {
            "AE" => Self::AE,
            "AS" => Self::AS,
            "AT" => Self::AT,
            "CS" => Self::CS,
            "DA" => Self::DA,
            "DS" => Self::DS,
            "DT" => Self::DT,
            "FL" => Self::FL,
            "FD" => Self::FD,
            "IS" => Self::IS,
            "LO" => Self::LO,
            "LT" => Self::LT,
            "OB" => Self::OB,
            "OD" => Self::OD,
            "OF" => Self::OF,
            "OL" => Self::OL,
            "OV" => Self::OV,
            "OW" => Self::OW,
            "PN" => Self::PN,
            "SH" => Self::SH,
            "SL" => Self::SL,
            "SQ" => Self::SQ,
            "SS" => Self::SS,
            "ST" => Self::ST,
            "SV" => Self::SV,
            "TM" => Self::TM,
            "UC" => Self::UC,
            "UI" => Self::UI,
            "UL" => Self::UL,
            "UN" => Self::UN,
            "UR" => Self::UR,
            "US" => Self::US,
            "UT" => Self::UT,
            "UV" => Self::UV,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AE => "AE",
            Self::AS => "AS",
            Self::AT => "AT",
            Self::CS => "CS",
            Self::DA => "DA",
            Self::DS => "DS",
            Self::DT => "DT",
            Self::FL => "FL",
            Self::FD => "FD",
            Self::IS => "IS",
            Self::LO => "LO",
            Self::LT => "LT",
            Self::OB => "OB",
            Self::OD => "OD",
            Self::OF => "OF",
            Self::OL => "OL",
            Self::OV => "OV",
            Self::OW => "OW",
            Self::PN => "PN",
            Self::SH => "SH",
            Self::SL => "SL",
            Self::SQ => "SQ",
            Self::SS => "SS",
            Self::ST => "ST",
            Self::SV => "SV",
            Self::TM => "TM",
            Self::UC => "UC",
            Self::UI => "UI",
            Self::UL => "UL",
            Self::UN => "UN",
            Self::UR => "UR",
            Self::US => "US",
            Self::UT => "UT",
            Self::UV => "UV",
            Self::Unknown => "??",
        }
    }
}

/// Root archived object in the `.dmap` file body.
#[derive(Archive, Serialize, Deserialize, Debug)]
#[archive(check_bytes)]
pub struct Dictionary {
    pub index: Vec<IndexEntry>,
    pub records: Vec<TagRecord>,
    pub strings: Vec<u8>,
}

/// FNV-1a 32-bit. Used for creator-hash in the index key.
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Canonicalize a private creator string the same way the Python normalize
/// stage does: uppercase, collapse internal whitespace.
pub fn canonicalize_creator(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// Compute the index key hash for a creator (0 for public tags).
pub fn creator_hash(s: Option<&str>) -> u32 {
    match s {
        None => 0,
        Some(c) => fnv1a32(canonicalize_creator(c).as_bytes()),
    }
}
