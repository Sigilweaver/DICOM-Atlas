//! The reader must refuse archives whose contents it cannot trust, accept
//! compatible extensions, and not report a different tag's record.

use dicom_map::{
    ArchiveCheck, DmapBuilder, DmapDict, DmapError, NewRecord, TagFlags, Vm, FORMAT_MAJOR,
};

fn base_builder() -> DmapBuilder {
    let mut b = DmapBuilder::new();
    b.push(
        NewRecord::new(0x0008, 0x0005)
            .vr("CS")
            .name("Public")
            .vm(Vm::new(1, 1, 1)),
    );
    b.push(
        NewRecord::new(0x0029, 0x0008)
            .creator("ACME")
            .vr("US or SS")
            .name("Private")
            .block_offset(true),
    );
    b.push(
        NewRecord::new(0x0029, 0x0009)
            .creator("ACME")
            .vr("LO")
            .name("Other"),
    );
    b
}

fn base() -> Vec<u8> {
    base_builder().finish().unwrap()
}

fn expect_bad(r: Result<DmapDict, DmapError>, want: ArchiveCheck) {
    match r {
        Err(DmapError::BadArchive { check, detail }) => {
            assert_eq!(check, want, "{detail}")
        }
        Err(e) => panic!("wrong error: {e}"),
        Ok(_) => panic!("accepted a bad archive (wanted {want:?})"),
    }
}

// Directory entry i starts at 32 + 24 * i: id u32, stride u32, offset u64, len u64.
fn section(b: &[u8], id: u32) -> (usize, usize, usize) {
    let n = u32::from_le_bytes(b[16..20].try_into().unwrap()) as usize;
    for i in 0..n {
        let d = 32 + 24 * i;
        if u32::from_le_bytes(b[d..d + 4].try_into().unwrap()) == id {
            let off = u64::from_le_bytes(b[d + 8..d + 16].try_into().unwrap()) as usize;
            let len = u64::from_le_bytes(b[d + 16..d + 24].try_into().unwrap()) as usize;
            return (d, off, len);
        }
    }
    panic!("no section {id}");
}

const INDEX: u32 = 1;
const RECORDS: u32 = 2;
const STRINGS: u32 = 3;

#[test]
fn consistent_archive_opens_and_looks_up() {
    let d = DmapDict::from_vec(base()).unwrap();
    assert_eq!(d.len(), 3);
    let v = d.lookup(0x0029, 0x0008, Some("acme")).unwrap();
    assert_eq!(
        (v.name(), v.vr(), v.creator()),
        ("Private", "US or SS", "ACME")
    );
    assert!(v.flags().contains(TagFlags::BLOCK_OFFSET));
    assert_eq!(d.iter().count(), 3);
}

#[test]
fn unsorted_or_duplicate_index_is_rejected() {
    let mut b = base();
    let (_, off, _) = section(&b, INDEX);
    let (first, second) = (b[off..off + 12].to_vec(), b[off + 12..off + 24].to_vec());
    b[off..off + 12].copy_from_slice(&second);
    b[off + 12..off + 24].copy_from_slice(&first);
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::IndexOrder);

    let mut b = base();
    let (_, off, _) = section(&b, INDEX);
    let first = b[off..off + 12].to_vec();
    b[off + 12..off + 24].copy_from_slice(&first);
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::IndexOrder);
}

#[test]
fn record_idx_out_of_range_is_rejected() {
    let mut b = base();
    let (_, off, _) = section(&b, INDEX);
    b[off + 8..off + 12].copy_from_slice(&99u32.to_le_bytes());
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::RecordIndex);
}

#[test]
fn string_range_outside_pool_is_rejected() {
    let mut b = base();
    let (_, off, _) = section(&b, RECORDS);
    // name length of record 0
    b[off + 32 + 8 + 4..off + 32 + 8 + 8].copy_from_slice(&60_000u32.to_le_bytes());
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::StringRange);

    let mut b = base();
    let (_, off, _) = section(&b, RECORDS);
    b[off + 32 + 24..off + 32 + 28].copy_from_slice(&u32::MAX.to_le_bytes());
    b[off + 32 + 28..off + 32 + 32].copy_from_slice(&10u32.to_le_bytes());
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::StringRange);
}

#[test]
fn invalid_utf8_is_rejected_not_returned_as_empty() {
    let mut b = base();
    let (_, off, _) = section(&b, STRINGS);
    // First pooled string belongs to a record; corrupt its first byte.
    b[off] = 0xFF;
    b[off + 1] = 0xFE;
    match DmapDict::from_vec(b) {
        Err(DmapError::BadArchive { check, .. }) => {
            assert!(matches!(
                check,
                ArchiveCheck::Text | ArchiveCheck::KeyMismatch
            ))
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn index_key_must_match_its_record() {
    let mut b = base();
    let (_, off, _) = section(&b, RECORDS);
    b[off + 2] ^= 1; // record 0 element
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::KeyMismatch);
}

#[test]
fn bad_vr_and_vm_are_rejected() {
    let mut b = base();
    let (_, off, _) = section(&b, RECORDS);
    b[off + 16] = 0; // empty VR
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::Text);
    let mut b = base();
    let (_, off, _) = section(&b, RECORDS);
    b[off + 8..off + 10].copy_from_slice(&5u16.to_le_bytes()); // min 5 > max 1
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::Vm);
}

#[test]
fn builder_refuses_what_the_reader_would_refuse() {
    use dicom_map::BuildError;
    let mut b = DmapBuilder::new();
    b.push(NewRecord::new(0x0029, 0x0010).vr("LO"));
    assert!(matches!(b.finish(), Err(BuildError::CreatorMissing { .. })));
    let mut b = DmapBuilder::new();
    b.push(NewRecord::new(0x0029, 0x1008).creator("ACME").vr("LO"));
    assert!(matches!(
        b.finish(),
        Err(BuildError::PrivateElementOutOfRange {
            group: 0x0029,
            element: 0x1008
        })
    ));
    let mut b = DmapBuilder::new();
    b.push(NewRecord::new(0x0029, 0x00FF).creator("ACME").vr("LO"));
    assert!(b.finish().is_ok());
    let mut b = DmapBuilder::new();
    b.push(NewRecord::new(0x0008, 0x0005).vr("ob"));
    b.push(NewRecord::new(0x0008, 0x0006).vr("a\0"));
    assert!(matches!(b.finish(), Err(BuildError::InvalidVr { .. })));
    let mut b = DmapBuilder::new();
    b.push(NewRecord::new(0x0008, 0x0005).vr("CS").name("a\0b"));
    assert!(matches!(b.finish(), Err(BuildError::NulInText { .. })));
    let mut b = DmapBuilder::new();
    b.push(NewRecord::new(0x0029, 0x0008).creator("a  b").vr("CS"));
    b.push(NewRecord::new(0x0029, 0x0008).creator("A B").vr("CS"));
    assert!(matches!(b.finish(), Err(BuildError::DuplicateKey { .. })));
}

#[test]
fn header_checks_and_forward_compatibility() {
    let good = base();
    // body_len
    let mut b = good.clone();
    b.extend_from_slice(&[0; 8]);
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::BodyLen);
    let mut b = good.clone();
    b.truncate(good.len() - 8);
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::BodyLen);
    // Truncated below the header.
    assert!(matches!(
        DmapDict::from_vec(good[..10].to_vec()),
        Err(DmapError::TooSmall { len: 10 })
    ));
    // Bad magic.
    let mut b = good.clone();
    b[0] = b'X';
    assert!(matches!(
        DmapDict::from_vec(b),
        Err(DmapError::BadMagic { .. })
    ));
    // Header flags and the reserved word are ignored by this major.
    let mut b = good.clone();
    b[20] = 0xFF;
    b[24] = 0xFF;
    assert!(DmapDict::from_vec(b).is_ok());
    // Future major is rejected with the supported major named.
    let mut b = good.clone();
    b[4..6].copy_from_slice(&(FORMAT_MAJOR + 1).to_le_bytes());
    assert!(matches!(
        DmapDict::from_vec(b),
        Err(DmapError::UnsupportedVersion { supported_major, .. }) if supported_major == FORMAT_MAJOR
    ));
}

#[test]
fn unknown_section_and_larger_record_stride_are_accepted() {
    // Rebuild the archive by hand with an extra section and wider records,
    // as a later minor would write.
    let good = base();
    let (_, ioff, ilen) = section(&good, INDEX);
    let (_, roff, rlen) = section(&good, RECORDS);
    let (_, soff, slen) = section(&good, STRINGS);
    let index = &good[ioff..ioff + ilen];
    let strings = &good[soff..soff + slen];
    let mut records = Vec::new();
    for r in good[roff..roff + rlen].chunks(72) {
        records.extend_from_slice(r);
        records.extend_from_slice(&[0xAB; 8]); // appended fields
    }
    let extra = b"future".to_vec();
    let secs: [(u32, u32, &[u8]); 4] = [
        (1, 12, index),
        (2, 80, &records),
        (3, 1, strings),
        (99, 1, &extra),
    ];
    let dir_end = 32 + 24 * secs.len();
    let mut dir = Vec::new();
    let mut data = Vec::new();
    for (id, stride, bytes) in secs {
        let off = (dir_end + data.len()) as u64;
        dir.extend_from_slice(&id.to_le_bytes());
        dir.extend_from_slice(&stride.to_le_bytes());
        dir.extend_from_slice(&off.to_le_bytes());
        dir.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        data.extend_from_slice(bytes);
    }
    let mut out = b"DMAP".to_vec();
    out.extend_from_slice(&FORMAT_MAJOR.to_le_bytes());
    out.extend_from_slice(&5u16.to_le_bytes());
    out.extend_from_slice(&((dir.len() + data.len()) as u64).to_le_bytes());
    out.extend_from_slice(&(secs.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    out.extend_from_slice(&dir);
    out.extend_from_slice(&data);
    let d = DmapDict::from_vec(out).expect("later minor with extra section and fields");
    assert_eq!(
        d.lookup(0x0029, 0x0009, Some("acme")).unwrap().name(),
        "Other"
    );

    // A missing required section is an error.
    let mut b = good.clone();
    let (dpos, _, _) = section(&b, STRINGS);
    b[dpos..dpos + 4].copy_from_slice(&77u32.to_le_bytes());
    expect_bad(DmapDict::from_vec(b), ArchiveCheck::MissingSection);
}

#[test]
fn private_lookup_accepts_the_element_as_written_in_a_file() {
    let d = DmapDict::from_vec(base()).unwrap();
    for e in [0x0008, 0x1008, 0xFF08] {
        let v = d
            .lookup(0x0029, e, Some("ACME"))
            .unwrap_or_else(|| panic!("{e:04X}"));
        assert_eq!(v.name(), "Private");
    }
    assert!(d.lookup(0x0008, 0x1005, None).is_none());
    assert!(d.lookup(0x0029, 0x0008, None).is_none());
}

#[test]
fn creator_hash_collision_is_not_a_match() {
    use dicom_map::creator_hash;
    let mut seen = std::collections::HashMap::new();
    let (a, b) = (0u32..)
        .find_map(|i| {
            let c = format!("C{i}");
            seen.insert(creator_hash(Some(&c)), c.clone())
                .map(|p| (p, c))
        })
        .unwrap();
    let mut bl = DmapBuilder::new();
    bl.push(
        NewRecord::new(0x0029, 0x0008)
            .creator(a.as_str())
            .vr("LO")
            .name("A"),
    );
    let d = DmapDict::from_vec(bl.finish().unwrap()).unwrap();
    assert_eq!(d.lookup(0x0029, 0x0008, Some(&a)).unwrap().name(), "A");
    assert!(d.lookup(0x0029, 0x0008, Some(&b)).is_none());
}

#[test]
fn metadata_round_trips() {
    let mut b = base_builder();
    b.meta("standard_edition", "2024b").unwrap();
    let d = DmapDict::from_vec(b.finish().unwrap()).unwrap();
    assert_eq!(d.meta("standard_edition"), Some("2024b"));
    assert_eq!(d.meta("nope"), None);
}
