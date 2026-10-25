//! Reader: mmap a `.dmap` file and perform O(log n) lookups.

use std::fs::File;
use std::path::Path;

use memmap2::Mmap;
use rkyv::{access, access_unchecked, rancor::Error};

use crate::schema::{
    canonicalize_creator, creator_hash as compute_creator_hash, ArchivedDictionary,
    ArchivedTagRecord, ArchivedVrCode, FileHeader, MAGIC, VERSION,
};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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

/// Check the header and the archive, then the invariants the reader relies on
/// but rkyv's byte-level validation cannot know about.
fn validate(bytes: &[u8]) -> Result<(), DmapError> {
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
    let bad = |m: String| DmapError::BadArchive(m);
    let body = &bytes[FileHeader::SIZE..];
    if hdr.body_len != body.len() as u64 {
        return Err(bad(format!(
            "header body_len {} does not match the {} body bytes present",
            hdr.body_len,
            body.len()
        )));
    }
    if hdr._reserved != [0; 2]
        || hdr.body_sha256_lo != 0
        || bytes[24..FileHeader::SIZE].iter().any(|&b| b != 0)
    {
        return Err(bad(
            "reserved header bytes are not zero (version 4 defines no use for them)".into(),
        ));
    }
    let dict = access::<ArchivedDictionary, Error>(body).map_err(|e| bad(e.to_string()))?;

    let strings = dict.strings.as_slice();
    let text = |what: &str, i: usize, off: u32, len: u32| -> Result<&str, DmapError> {
        let (s, l) = (off as usize, len as usize);
        let end = s
            .checked_add(l)
            .filter(|&e| e <= strings.len())
            .ok_or_else(|| {
                bad(format!(
                    "record {i}: {what} range {s}+{l} lies outside the {} byte string pool",
                    strings.len()
                ))
            })?;
        std::str::from_utf8(&strings[s..end])
            .map_err(|e| bad(format!("record {i}: {what} is not valid UTF-8: {e}")))
    };

    let mut prev: Option<(u16, u16, u32)> = None;
    for (n, e) in dict.index.iter().enumerate() {
        let key = (
            e.group.to_native(),
            e.element.to_native(),
            e.creator_hash.to_native(),
        );
        if let Some(p) = prev {
            if p >= key {
                return Err(bad(format!(
                    "index entry {n} is not strictly after entry {}: keys must be sorted and unique",
                    n - 1
                )));
            }
        }
        prev = Some(key);
        let ri = e.record_idx.to_native() as usize;
        let rec = dict.records.get(ri).ok_or_else(|| {
            bad(format!(
                "index entry {n}: record_idx {ri} is out of range ({} records)",
                dict.records.len()
            ))
        })?;
        let creator = text(
            "creator",
            ri,
            rec.creator_off.to_native(),
            rec.creator_len.to_native() as u32,
        )?;
        let rec_key = (
            rec.group.to_native(),
            rec.element.to_native(),
            compute_creator_hash((!creator.is_empty()).then_some(creator)),
        );
        if rec_key != key {
            return Err(bad(format!(
                "index entry {n} key {key:04X?} does not match its record {ri} ({rec_key:04X?})"
            )));
        }
        if key.0 % 2 == 1 && key.2 == 0 {
            return Err(bad(format!(
                "record {ri}: odd group {:04X} entry without a private creator",
                key.0
            )));
        }
        text(
            "keyword",
            ri,
            rec.keyword_off.to_native(),
            rec.keyword_len.to_native() as u32,
        )?;
        text(
            "name",
            ri,
            rec.name_off.to_native(),
            rec.name_len.to_native() as u32,
        )?;
        text(
            "description",
            ri,
            rec.description_off.to_native(),
            rec.description_len.to_native(),
        )?;
        text(
            "sources",
            ri,
            rec.sources_off.to_native(),
            rec.sources_len.to_native(),
        )?;
    }
    Ok(())
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

        validate(&mmap[..])?;

        Ok(Self {
            backing: Backing::Mmap { _file: file, mmap },
            body_off: FileHeader::SIZE,
        })
    }

    /// Construct from a `'static` byte slice, typically `include_bytes!`.
    pub fn from_static(bytes: &'static [u8]) -> Result<Self, DmapError> {
        validate(bytes)?;
        Ok(Self {
            backing: Backing::Static(bytes),
            body_off: FileHeader::SIZE,
        })
    }

    fn archived(&self) -> &ArchivedDictionary {
        // SAFETY: validated in `open()` / `from_static()`.
        unsafe { access_unchecked::<ArchivedDictionary>(&self.backing.bytes()[self.body_off..]) }
    }

    pub fn len(&self) -> usize {
        self.archived().index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn lookup(&self, group: u16, element: u16, creator: Option<&str>) -> Option<TagView<'_>> {
        // A private element is addressed by its block offset (the low byte);
        // callers often hold the element as it appears in a file (0x1008 for
        // offset 0x08 in block 0x10), and any block belongs to the creator
        // (PS3.5 7.8.1), so reduce it rather than report a miss.
        let element = if creator.is_some() && group % 2 == 1 {
            element & 0x00FF
        } else {
            element
        };
        let ch = compute_creator_hash(creator);
        let dict = self.archived();
        let index = &dict.index;

        let key = (group, element, ch);
        let idx = index
            .binary_search_by(|e| {
                (
                    e.group.to_native(),
                    e.element.to_native(),
                    e.creator_hash.to_native(),
                )
                    .cmp(&key)
            })
            .ok()?;

        let rec_idx = index[idx].record_idx.to_native() as usize;
        let rec = &dict.records[rec_idx];
        let view = TagView {
            rec,
            strings: dict.strings.as_slice(),
        };
        // The index key holds only a 32-bit hash of the creator. Confirm the
        // match against the stored canonical creator string.
        if let Some(c) = creator {
            if view.creator() != canonicalize_creator(c) {
                return None;
            }
        }
        Some(view)
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
        // Every range and its UTF-8 validity is checked by `validate` when
        // the archive is opened, so this cannot fail on an accepted archive.
        std::str::from_utf8(&self.strings[s..e]).expect("string range validated at open")
    }

    pub fn group(&self) -> u16 {
        self.rec.group.to_native()
    }

    pub fn element(&self) -> u16 {
        self.rec.element.to_native()
    }

    pub fn keyword(&self) -> &'a str {
        self.slice(
            self.rec.keyword_off.to_native(),
            self.rec.keyword_len.to_native() as u32,
        )
    }

    pub fn name(&self) -> &'a str {
        self.slice(
            self.rec.name_off.to_native(),
            self.rec.name_len.to_native() as u32,
        )
    }

    pub fn creator(&self) -> &'a str {
        let len = self.rec.creator_len.to_native();
        if len == 0 {
            return "";
        }
        self.slice(self.rec.creator_off.to_native(), len as u32)
    }

    pub fn description(&self) -> &'a str {
        self.slice(
            self.rec.description_off.to_native(),
            self.rec.description_len.to_native(),
        )
    }

    /// Pipe-delimited list of source PDF filenames that document this tag.
    /// Each entry is a filename (optionally with a `#pN` page anchor) from
    /// `data/sources.json`. Empty for public PS3.6 tags.
    pub fn sources_raw(&self) -> &'a str {
        self.slice(
            self.rec.sources_off.to_native(),
            self.rec.sources_len.to_native(),
        )
    }

    /// Source PDF filenames split on `|`. Empty iterator for public PS3.6 tags.
    pub fn sources(&self) -> impl Iterator<Item = &'a str> {
        self.sources_raw().split('|').filter(|s| !s.is_empty())
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
        ArchivedVrCode::UsOrSs => "US or SS",
        ArchivedVrCode::ObOrOw => "OB or OW",
        ArchivedVrCode::UsOrOw => "US or OW",
        ArchivedVrCode::UsOrSsOrOw => "US or SS or OW",
        ArchivedVrCode::Unknown => "??",
    }
}
