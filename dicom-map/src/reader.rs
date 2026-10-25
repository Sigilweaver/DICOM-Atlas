//! Reader: mmap a `.dmap` file and perform O(log n) lookups.

use std::fs::File;
use std::path::Path;

use memmap2::Mmap;

use crate::canon::{canonicalize_creator, creator_hash};
use crate::error::{ArchiveCheck, DmapError};
use crate::format::*;
use crate::types::{TagFlags, Vm, VM_UNBOUNDED};

/// Options for opening a dictionary. Strict validation is always on; this
/// type exists so options can be added without changing `open_with`.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct OpenOptions {}

impl OpenOptions {
    pub fn new() -> Self {
        Self::default()
    }
}

enum Backing {
    Mmap { _file: File, mmap: Mmap },
    Static(&'static [u8]),
    Owned(Vec<u8>),
}

impl Backing {
    fn bytes(&self) -> &[u8] {
        match self {
            Backing::Mmap { mmap, .. } => &mmap[..],
            Backing::Static(b) => b,
            Backing::Owned(v) => v,
        }
    }
}

#[derive(Clone, Copy)]
struct Section {
    off: usize,
    len: usize,
    stride: usize,
}

impl Section {
    fn count(&self) -> usize {
        self.len.checked_div(self.stride).unwrap_or(0)
    }
}

/// A validated dictionary.
pub struct DmapDict {
    backing: Backing,
    index: Section,
    records: Section,
    strings: Section,
    meta: Option<Section>,
}

impl std::fmt::Debug for DmapDict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DmapDict")
            .field("len", &self.len())
            .finish()
    }
}

impl DmapDict {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DmapError> {
        Self::open_with(path, &OpenOptions::default())
    }

    pub fn open_with(path: impl AsRef<Path>, _options: &OpenOptions) -> Result<Self, DmapError> {
        let file = File::open(path)?;
        // SAFETY: the file is held for the life of the mapping. As with any
        // mmap, another process truncating the file while it is mapped is
        // outside what this crate can guard against.
        let mmap = unsafe { Mmap::map(&file)? };
        let layout = validate(&mmap[..])?;
        Ok(Self::with(Backing::Mmap { _file: file, mmap }, layout))
    }

    /// Construct from a `'static` byte slice, typically `include_bytes!`.
    pub fn from_static(bytes: &'static [u8]) -> Result<Self, DmapError> {
        let layout = validate(bytes)?;
        Ok(Self::with(Backing::Static(bytes), layout))
    }

    /// Construct from owned bytes.
    pub fn from_vec(bytes: Vec<u8>) -> Result<Self, DmapError> {
        let layout = validate(&bytes)?;
        Ok(Self::with(Backing::Owned(bytes), layout))
    }

    fn with(backing: Backing, l: Layout) -> Self {
        Self {
            backing,
            index: l.index,
            records: l.records,
            strings: l.strings,
            meta: l.meta,
        }
    }

    pub fn len(&self) -> usize {
        self.index.count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Value of a metadata key written by the compiler (for example
    /// `standard_edition`), or `None`.
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta_iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// All metadata as `(key, value)` pairs in file order.
    pub fn meta_iter(&self) -> impl Iterator<Item = (&str, &str)> {
        let text = self
            .meta
            .map(|m| {
                let b = &self.backing.bytes()[m.off..m.off + m.len];
                std::str::from_utf8(b).expect("meta validated at open")
            })
            .unwrap_or("");
        text.lines().filter_map(|l| l.split_once('='))
    }

    /// Look up a tag. For a private creator on an odd group the element is
    /// reduced to its block offset (low byte), since any block belongs to the
    /// creator (PS3.5 2024b 7.8.1). A miss is `None`.
    pub fn lookup(&self, group: u16, element: u16, creator: Option<&str>) -> Option<TagView<'_>> {
        let element = if creator.is_some() && group % 2 == 1 {
            element & 0x00FF
        } else {
            element
        };
        let ch = creator_hash(creator);
        let b = self.backing.bytes();
        let key = (group, element, ch);
        let (mut lo, mut hi) = (0usize, self.index.count());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let e = self.index.off + mid * self.index.stride;
            let k = (u16_at(b, e), u16_at(b, e + 2), u32_at(b, e + 4));
            match k.cmp(&key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let view = self.view(u32_at(b, e + 8) as usize);
                    // The index key holds only a 32-bit hash of the creator;
                    // confirm against the stored canonical string.
                    if let Some(c) = creator {
                        if view.creator() != canonicalize_creator(c) {
                            return None;
                        }
                    }
                    return Some(view);
                }
            }
        }
        None
    }

    /// Every record, in index order.
    pub fn iter(&self) -> impl Iterator<Item = TagView<'_>> {
        let b = self.backing.bytes();
        (0..self.index.count()).map(move |i| {
            let e = self.index.off + i * self.index.stride;
            self.view(u32_at(b, e + 8) as usize)
        })
    }

    /// The `i`-th record in index order, `None` past the end.
    pub fn nth(&self, i: usize) -> Option<TagView<'_>> {
        if i >= self.index.count() {
            return None;
        }
        let e = self.index.off + i * self.index.stride;
        Some(self.view(u32_at(self.backing.bytes(), e + 8) as usize))
    }

    fn view(&self, rec_idx: usize) -> TagView<'_> {
        let b = self.backing.bytes();
        let r = self.records.off + rec_idx * self.records.stride;
        TagView {
            rec: &b[r..r + self.records.stride],
            strings: &b[self.strings.off..self.strings.off + self.strings.len],
        }
    }
}

/// One record. Borrows the dictionary.
#[derive(Clone, Copy)]
pub struct TagView<'a> {
    rec: &'a [u8],
    strings: &'a [u8],
}

impl std::fmt::Debug for TagView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TagView")
            .field("group", &self.group())
            .field("element", &self.element())
            .field("creator", &self.creator())
            .field("vr", &self.vr())
            .field("name", &self.name())
            .finish()
    }
}

impl<'a> TagView<'a> {
    fn string(&self, which: usize) -> &'a str {
        let o = REC_STRINGS + which * 8;
        let off = u32_at(self.rec, o) as usize;
        let len = u32_at(self.rec, o + 4) as usize;
        // Ranges and UTF-8 are validated when the archive is opened.
        std::str::from_utf8(&self.strings[off..off + len]).expect("string validated at open")
    }

    pub fn group(&self) -> u16 {
        u16_at(self.rec, 0)
    }

    pub fn element(&self) -> u16 {
        u16_at(self.rec, 2)
    }

    pub fn flags(&self) -> TagFlags {
        TagFlags::from_bits(u32_at(self.rec, REC_FLAGS))
    }

    pub fn retired(&self) -> bool {
        self.flags().contains(TagFlags::RETIRED)
    }

    pub fn is_block_offset(&self) -> bool {
        self.flags().contains(TagFlags::BLOCK_OFFSET)
    }

    pub fn vm(&self) -> Vm {
        Vm::new(
            u16_at(self.rec, REC_VM),
            u16_at(self.rec, REC_VM + 2),
            u16_at(self.rec, REC_VM + 4),
        )
    }

    /// VR text: a two-letter VR, `US or SS`, `OB or OW`, `US or OW`,
    /// `US or SS or OW`, or `??` when the source declares none. New values
    /// can appear in later minors.
    pub fn vr(&self) -> &'a str {
        let raw = &self.rec[REC_VR..REC_VR + VR_MAX];
        let n = raw.iter().position(|&b| b == 0).unwrap_or(VR_MAX);
        std::str::from_utf8(&raw[..n]).expect("VR validated at open")
    }

    pub fn keyword(&self) -> &'a str {
        self.string(STR_KEYWORD)
    }

    pub fn name(&self) -> &'a str {
        self.string(STR_NAME)
    }

    /// Canonical creator, empty for public tags.
    pub fn creator(&self) -> &'a str {
        self.string(STR_CREATOR)
    }

    pub fn description(&self) -> &'a str {
        self.string(STR_DESCRIPTION)
    }

    /// Pipe-delimited source PDF filenames (with `#pN` page anchors).
    pub fn sources_raw(&self) -> &'a str {
        self.string(STR_SOURCES)
    }

    pub fn sources(&self) -> impl Iterator<Item = &'a str> {
        self.sources_raw().split('|').filter(|s| !s.is_empty())
    }
}

struct Layout {
    index: Section,
    records: Section,
    strings: Section,
    meta: Option<Section>,
}

/// Check the header, the section directory and every invariant the reader
/// relies on. After this succeeds no accessor can index out of range.
fn validate(bytes: &[u8]) -> Result<Layout, DmapError> {
    use ArchiveCheck as C;
    if bytes.len() < HEADER_SIZE {
        return Err(DmapError::TooSmall { len: bytes.len() });
    }
    let magic = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if &magic != MAGIC {
        return Err(DmapError::BadMagic { got: magic });
    }
    let (major, minor) = (u16_at(bytes, 4), u16_at(bytes, 6));
    if major != FORMAT_MAJOR {
        return Err(DmapError::UnsupportedVersion {
            got_major: major,
            got_minor: minor,
            supported_major: FORMAT_MAJOR,
        });
    }
    let body_len = u64_at(bytes, 8);
    if body_len != (bytes.len() - HEADER_SIZE) as u64 {
        return Err(DmapError::bad(
            C::BodyLen,
            format!(
                "header body_len {body_len} does not match the {} bytes after the header",
                bytes.len() - HEADER_SIZE
            ),
        ));
    }
    // header_flags (offset 20) and the reserved word (24) are ignored: later
    // minors may define them.
    let nsec = u32_at(bytes, 16) as usize;
    let dir_end = nsec
        .checked_mul(DIR_ENTRY_SIZE)
        .and_then(|n| n.checked_add(HEADER_SIZE))
        .filter(|&e| e <= bytes.len())
        .ok_or_else(|| {
            DmapError::bad(
                C::SectionDirectory,
                format!("{nsec} directory entries do not fit in the file"),
            )
        })?;
    let _ = dir_end;

    let mut found: [Option<Section>; 5] = [None; 5];
    for i in 0..nsec {
        let d = HEADER_SIZE + i * DIR_ENTRY_SIZE;
        let id = u32_at(bytes, d);
        let stride = u32_at(bytes, d + 4) as usize;
        let off = u64_at(bytes, d + 8);
        let len = u64_at(bytes, d + 16);
        let end = off.checked_add(len);
        let (off, len) = match end {
            Some(e) if off >= (HEADER_SIZE + nsec * DIR_ENTRY_SIZE) as u64 && e <= bytes.len() as u64 => {
                (off as usize, len as usize)
            }
            _ => {
                return Err(DmapError::bad(
                    C::SectionRange,
                    format!("section {id}: offset {off} length {len} lies outside the file or inside the directory"),
                ))
            }
        };
        if !(1..=4).contains(&id) {
            continue; // unknown section from a later minor
        }
        if found[id as usize].is_some() {
            return Err(DmapError::bad(C::DuplicateSection, format!("section {id}")));
        }
        found[id as usize] = Some(Section { off, len, stride });
    }
    let need = |id: u32, min: usize, what: &str| -> Result<Section, DmapError> {
        let s = found[id as usize]
            .ok_or_else(|| DmapError::bad(C::MissingSection, format!("no {what} section")))?;
        if s.stride < min || s.len % s.stride.max(1) != 0 {
            return Err(DmapError::bad(
                C::EntrySize,
                format!(
                    "{what}: entry size {} (need at least {min}) and length {} must divide evenly",
                    s.stride, s.len
                ),
            ));
        }
        Ok(s)
    };
    let index = need(SEC_INDEX, INDEX_ENTRY_SIZE, "index")?;
    let records = need(SEC_RECORDS, RECORD_SIZE, "records")?;
    let strings = found[SEC_STRINGS as usize]
        .ok_or_else(|| DmapError::bad(C::MissingSection, "no strings section"))?;
    let meta = found[SEC_META as usize];

    if let Some(m) = meta {
        let text = std::str::from_utf8(&bytes[m.off..m.off + m.len])
            .map_err(|e| DmapError::bad(C::Meta, format!("not UTF-8: {e}")))?;
        if let Some(l) = text.lines().find(|l| !l.is_empty() && !l.contains('=')) {
            return Err(DmapError::bad(C::Meta, format!("line {l:?} has no `=`")));
        }
    }

    let pool = &bytes[strings.off..strings.off + strings.len];
    let text = |what: &str, i: usize, rec: &[u8], which: usize| -> Result<&str, DmapError> {
        let o = REC_STRINGS + which * 8;
        let (off, len) = (u32_at(rec, o) as usize, u32_at(rec, o + 4) as usize);
        let end = off
            .checked_add(len)
            .filter(|&e| e <= pool.len())
            .ok_or_else(|| {
                DmapError::bad(
                    C::StringRange,
                    format!(
                        "record {i}: {what} range {off}+{len} lies outside the {} byte string pool",
                        pool.len()
                    ),
                )
            })?;
        std::str::from_utf8(&pool[off..end]).map_err(|e| {
            DmapError::bad(
                C::Text,
                format!("record {i}: {what} is not valid UTF-8: {e}"),
            )
        })
    };

    let mut prev: Option<(u16, u16, u32)> = None;
    for n in 0..index.count() {
        let e = index.off + n * index.stride;
        let key = (u16_at(bytes, e), u16_at(bytes, e + 2), u32_at(bytes, e + 4));
        if let Some(p) = prev {
            if p >= key {
                return Err(DmapError::bad(
                    C::IndexOrder,
                    format!("index entry {n} is not strictly after entry {}: keys must be sorted and unique", n - 1),
                ));
            }
        }
        prev = Some(key);
        let ri = u32_at(bytes, e + 8) as usize;
        if ri >= records.count() {
            return Err(DmapError::bad(
                C::RecordIndex,
                format!(
                    "index entry {n}: record_idx {ri} is out of range ({} records)",
                    records.count()
                ),
            ));
        }
        let r = records.off + ri * records.stride;
        let rec = &bytes[r..r + records.stride];
        let creator = text("creator", ri, rec, STR_CREATOR)?;
        let rec_key = (
            u16_at(rec, 0),
            u16_at(rec, 2),
            creator_hash((!creator.is_empty()).then_some(creator)),
        );
        if rec_key != key {
            return Err(DmapError::bad(
                C::KeyMismatch,
                format!("index entry {n} key {key:04X?} does not match its record {ri} ({rec_key:04X?})"),
            ));
        }
        if key.0 % 2 == 1 && key.2 == 0 {
            return Err(DmapError::bad(
                C::CreatorMissing,
                format!(
                    "record {ri}: odd group {:04X} entry without a private creator",
                    key.0
                ),
            ));
        }
        for (what, w) in [
            ("keyword", STR_KEYWORD),
            ("name", STR_NAME),
            ("description", STR_DESCRIPTION),
            ("sources", STR_SOURCES),
        ] {
            text(what, ri, rec, w)?;
        }
        let vr = &rec[REC_VR..REC_VR + VR_MAX];
        let n_vr = vr.iter().position(|&b| b == 0).unwrap_or(VR_MAX);
        if n_vr == 0
            || !vr[..n_vr]
                .iter()
                .all(|b| b.is_ascii_graphic() || *b == b' ')
            || vr[n_vr..].iter().any(|&b| b != 0)
        {
            return Err(DmapError::bad(
                C::Text,
                format!(
                    "record {ri}: VR is not 1 to {VR_MAX} printable ASCII bytes followed by NULs"
                ),
            ));
        }
        let (vmin, vmax, vstep) = (
            u16_at(rec, REC_VM),
            u16_at(rec, REC_VM + 2),
            u16_at(rec, REC_VM + 4),
        );
        if vstep == 0 || (vmax != VM_UNBOUNDED && vmax < vmin) {
            return Err(DmapError::bad(
                C::Vm,
                format!("record {ri}: vm {vmin}..{vmax} step {vstep}"),
            ));
        }
    }
    Ok(Layout {
        index,
        records,
        strings,
        meta,
    })
}
