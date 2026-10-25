//! Archive writer, behind the `write` feature.

use std::collections::BTreeMap;

use crate::canon::{canonicalize_creator, creator_hash};
use crate::format::*;
use crate::types::{TagFlags, Vm, VM_UNBOUNDED};

/// Why an archive could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    #[error("({group:04X},{element:04X}) creator {creator:?} appears twice")]
    DuplicateKey {
        group: u16,
        element: u16,
        creator: String,
    },
    #[error(
        "creators {a:?} and {b:?} differ but share hash {hash:#010x} at ({group:04X},{element:04X})"
    )]
    HashCollision {
        group: u16,
        element: u16,
        hash: u32,
        a: String,
        b: String,
    },
    #[error("({group:04X},{element:04X}): odd group needs a private creator")]
    CreatorMissing { group: u16, element: u16 },
    #[error(
        "({group:04X},{element:04X}): a private element is stored as its block offset (0x00 to 0xFF); \
         lookup reduces the element to its low byte, so this record could never be found"
    )]
    PrivateElementOutOfRange { group: u16, element: u16 },
    #[error("({group:04X},{element:04X}): VR {vr:?} is not 1 to 16 printable ASCII bytes")]
    InvalidVr {
        group: u16,
        element: u16,
        vr: String,
    },
    #[error("({group:04X},{element:04X}): vm {min}..{max} step {step} is inconsistent")]
    InvalidVm {
        group: u16,
        element: u16,
        min: u16,
        max: u16,
        step: u16,
    },
    #[error("({group:04X},{element:04X}): {field} contains a NUL byte")]
    NulInText {
        group: u16,
        element: u16,
        field: &'static str,
    },
    #[error("metadata key {0:?} or its value is invalid (no `=`, newline or NUL allowed in keys)")]
    InvalidMeta(String),
    #[error("{0} exceeds the 32-bit limits of the format")]
    TooLarge(&'static str),
}

/// One record to write. Built with chained setters.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NewRecord {
    group: u16,
    element: u16,
    creator: String,
    vr: String,
    vm: Vm,
    keyword: String,
    name: String,
    description: String,
    sources: String,
    flags: TagFlags,
}

impl NewRecord {
    /// `vr` and `vm` start as `"??"` and [`Vm::UNDECLARED`].
    pub fn new(group: u16, element: u16) -> Self {
        Self {
            group,
            element,
            creator: String::new(),
            vr: "??".into(),
            vm: Vm::UNDECLARED,
            keyword: String::new(),
            name: String::new(),
            description: String::new(),
            sources: String::new(),
            flags: TagFlags::default(),
        }
    }
    /// Private creator (any spelling; stored canonicalized).
    pub fn creator(mut self, v: impl Into<String>) -> Self {
        self.creator = v.into();
        self
    }
    pub fn vr(mut self, v: impl Into<String>) -> Self {
        self.vr = v.into();
        self
    }
    pub fn vm(mut self, v: Vm) -> Self {
        self.vm = v;
        self
    }
    pub fn keyword(mut self, v: impl Into<String>) -> Self {
        self.keyword = v.into();
        self
    }
    pub fn name(mut self, v: impl Into<String>) -> Self {
        self.name = v.into();
        self
    }
    pub fn description(mut self, v: impl Into<String>) -> Self {
        self.description = v.into();
        self
    }
    /// Pipe-delimited source list.
    pub fn sources(mut self, v: impl Into<String>) -> Self {
        self.sources = v.into();
        self
    }
    pub fn retired(mut self, v: bool) -> Self {
        if v {
            self.flags = self.flags.union(TagFlags::RETIRED);
        }
        self
    }
    /// `element` is a private block offset.
    pub fn block_offset(mut self, v: bool) -> Self {
        if v {
            self.flags = self.flags.union(TagFlags::BLOCK_OFFSET);
        }
        self
    }
}

/// Collects records and metadata and produces archive bytes.
#[derive(Debug, Default)]
pub struct DmapBuilder {
    records: Vec<NewRecord>,
    meta: Vec<(String, String)>,
}

#[derive(Default)]
struct Pool(Vec<u8>);

impl Pool {
    fn put(&mut self, s: &str) -> Result<(u32, u32), BuildError> {
        if s.is_empty() {
            return Ok((0, 0));
        }
        let off = u32::try_from(self.0.len()).map_err(|_| BuildError::TooLarge("string pool"))?;
        let len = u32::try_from(s.len()).map_err(|_| BuildError::TooLarge("string"))?;
        self.0.extend_from_slice(s.as_bytes());
        Ok((off, len))
    }
}

impl DmapBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, r: NewRecord) -> &mut Self {
        self.records.push(r);
        self
    }

    /// Add a metadata pair (for example `standard_edition`, `2024b`).
    pub fn meta(&mut self, key: &str, value: &str) -> Result<&mut Self, BuildError> {
        let bad = |s: &str| s.contains(['\n', '\0']);
        if key.is_empty() || key.contains('=') || bad(key) || bad(value) {
            return Err(BuildError::InvalidMeta(key.to_owned()));
        }
        self.meta.push((key.to_owned(), value.to_owned()));
        Ok(self)
    }

    /// Validate, sort and serialize.
    pub fn finish(self) -> Result<Vec<u8>, BuildError> {
        let mut pool = Pool::default();
        let mut keyed: BTreeMap<(u16, u16, u32), (Vec<u8>, String)> = BTreeMap::new();
        for r in &self.records {
            let (g, e) = (r.group, r.element);
            let canon = canonicalize_creator(&r.creator);
            if g % 2 == 1 && canon.is_empty() {
                return Err(BuildError::CreatorMissing {
                    group: g,
                    element: e,
                });
            }
            if g % 2 == 1 && e > 0x00FF {
                return Err(BuildError::PrivateElementOutOfRange {
                    group: g,
                    element: e,
                });
            }
            let vrb = r.vr.as_bytes();
            if vrb.is_empty()
                || vrb.len() > VR_MAX
                || !vrb.iter().all(|b| b.is_ascii_graphic() || *b == b' ')
            {
                return Err(BuildError::InvalidVr {
                    group: g,
                    element: e,
                    vr: r.vr.clone(),
                });
            }
            if r.vm.step == 0 || (r.vm.max != VM_UNBOUNDED && r.vm.max < r.vm.min) {
                return Err(BuildError::InvalidVm {
                    group: g,
                    element: e,
                    min: r.vm.min,
                    max: r.vm.max,
                    step: r.vm.step,
                });
            }
            for (field, s) in [
                ("keyword", &r.keyword),
                ("name", &r.name),
                ("creator", &canon),
                ("description", &r.description),
                ("sources", &r.sources),
            ] {
                if s.contains('\0') {
                    return Err(BuildError::NulInText {
                        group: g,
                        element: e,
                        field,
                    });
                }
            }
            let hash = creator_hash((!canon.is_empty()).then_some(canon.as_str()));
            let mut rec = vec![0u8; RECORD_SIZE];
            rec[0..2].copy_from_slice(&g.to_le_bytes());
            rec[2..4].copy_from_slice(&e.to_le_bytes());
            rec[REC_FLAGS..REC_FLAGS + 4].copy_from_slice(&r.flags.bits().to_le_bytes());
            rec[REC_VM..REC_VM + 2].copy_from_slice(&r.vm.min.to_le_bytes());
            rec[REC_VM + 2..REC_VM + 4].copy_from_slice(&r.vm.max.to_le_bytes());
            rec[REC_VM + 4..REC_VM + 6].copy_from_slice(&r.vm.step.to_le_bytes());
            rec[REC_VR..REC_VR + vrb.len()].copy_from_slice(vrb);
            for (i, s) in [&r.keyword, &r.name, &canon, &r.description, &r.sources]
                .into_iter()
                .enumerate()
            {
                let (off, len) = pool.put(s)?;
                let o = REC_STRINGS + i * 8;
                rec[o..o + 4].copy_from_slice(&off.to_le_bytes());
                rec[o + 4..o + 8].copy_from_slice(&len.to_le_bytes());
            }
            match keyed.get(&(g, e, hash)) {
                Some((_, prev)) if *prev == canon => {
                    return Err(BuildError::DuplicateKey {
                        group: g,
                        element: e,
                        creator: canon,
                    })
                }
                Some((_, prev)) => {
                    return Err(BuildError::HashCollision {
                        group: g,
                        element: e,
                        hash,
                        a: prev.clone(),
                        b: canon,
                    })
                }
                None => {
                    keyed.insert((g, e, hash), (rec, canon));
                }
            }
        }

        let n = keyed.len();
        u32::try_from(n).map_err(|_| BuildError::TooLarge("record count"))?;
        let mut index = Vec::with_capacity(n * INDEX_ENTRY_SIZE);
        let mut records = Vec::with_capacity(n * RECORD_SIZE);
        for (i, ((g, e, h), (rec, _))) in keyed.into_iter().enumerate() {
            index.extend_from_slice(&g.to_le_bytes());
            index.extend_from_slice(&e.to_le_bytes());
            index.extend_from_slice(&h.to_le_bytes());
            index.extend_from_slice(&(i as u32).to_le_bytes());
            records.extend_from_slice(&rec);
        }
        let meta: String = self
            .meta
            .iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect();

        let sections: [(u32, usize, &[u8]); 4] = [
            (SEC_INDEX, INDEX_ENTRY_SIZE, &index),
            (SEC_RECORDS, RECORD_SIZE, &records),
            (SEC_STRINGS, 1, &pool.0),
            (SEC_META, 1, meta.as_bytes()),
        ];
        let dir_end = HEADER_SIZE + sections.len() * DIR_ENTRY_SIZE;
        let mut dir = Vec::new();
        let mut data = Vec::new();
        for (id, stride, bytes) in sections {
            let pad = (dir_end + data.len()).next_multiple_of(8) - (dir_end + data.len());
            data.resize(data.len() + pad, 0);
            let off = (dir_end + data.len()) as u64;
            dir.extend_from_slice(&id.to_le_bytes());
            dir.extend_from_slice(&(stride as u32).to_le_bytes());
            dir.extend_from_slice(&off.to_le_bytes());
            dir.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            data.extend_from_slice(bytes);
        }
        let body_len = (dir.len() + data.len()) as u64;
        let mut out = Vec::with_capacity(HEADER_SIZE + body_len as usize);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_MAJOR.to_le_bytes());
        out.extend_from_slice(&FORMAT_MINOR.to_le_bytes());
        out.extend_from_slice(&body_len.to_le_bytes());
        out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&dir);
        out.extend_from_slice(&data);
        Ok(out)
    }
}
