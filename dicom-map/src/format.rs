//! On-disk layout. All integers little-endian; nothing here depends on
//! alignment. See `FORMAT.md`.

pub const MAGIC: &[u8; 4] = b"DMAP";

/// Incompatible layout changes bump this. Readers reject any other major.
pub const FORMAT_MAJOR: u16 = 4;
/// Compatible additions (new sections, appended record fields, new flag
/// bits) bump this. Readers accept any minor of their major.
pub const FORMAT_MINOR: u16 = 0;

pub const HEADER_SIZE: usize = 32;
// Header: magic[4] major u16 minor u16 body_len u64 section_count u32
//         header_flags u32 reserved u64
pub const DIR_ENTRY_SIZE: usize = 24;
// Directory entry: id u32, entry_size u32, offset u64 (from file start), len u64

pub const SEC_INDEX: u32 = 1;
pub const SEC_RECORDS: u32 = 2;
pub const SEC_STRINGS: u32 = 3;
pub const SEC_META: u32 = 4;

// Index entry: group u16, element u16, creator_hash u32, record_idx u32
pub const INDEX_ENTRY_SIZE: usize = 12;

// Record: group u16, element u16, flags u32, vm_min u16, vm_max u16,
//         vm_step u16, pad u16, vr[16] (ASCII, NUL padded),
//         then (off u32, len u32) for keyword, name, creator, description,
//         sources.
pub const RECORD_SIZE: usize = 72;
pub const REC_FLAGS: usize = 4;
pub const REC_VM: usize = 8;
pub const REC_VR: usize = 16;
pub const VR_MAX: usize = 16;
pub const REC_STRINGS: usize = 32;
pub const STR_KEYWORD: usize = 0;
pub const STR_NAME: usize = 1;
pub const STR_CREATOR: usize = 2;
pub const STR_DESCRIPTION: usize = 3;
pub const STR_SOURCES: usize = 4;

pub fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}
pub fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
pub fn u64_at(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}
