//! The reader must refuse archives whose contents it cannot trust, and must
//! not report a different tag's record for a query.

use dicom_map::schema::{
    canonicalize_creator, creator_hash, Dictionary, FileHeader, IndexEntry, TagRecord, VrCode,
};
use dicom_map::{DmapDict, DmapError};

struct Rec {
    group: u16,
    element: u16,
    creator: &'static str,
    name: &'static [u8],
}

/// Build a structurally consistent archive; tests then break one thing.
fn dictionary(recs: &[Rec]) -> Dictionary {
    let mut strings = Vec::new();
    let mut records = Vec::new();
    let mut index = Vec::new();
    for (i, r) in recs.iter().enumerate() {
        let c = canonicalize_creator(r.creator);
        let c_off = strings.len() as u32;
        strings.extend_from_slice(c.as_bytes());
        let n_off = strings.len() as u32;
        strings.extend_from_slice(r.name);
        records.push(TagRecord {
            group: r.group,
            element: r.element,
            element_is_block_offset: !r.creator.is_empty(),
            retired: false,
            vr: VrCode::LO,
            vm_min: 1,
            vm_max: 1,
            vm_step: 1,
            keyword_off: 0,
            keyword_len: 0,
            name_off: n_off,
            name_len: r.name.len() as u16,
            creator_off: c_off,
            creator_len: c.len() as u16,
            description_off: 0,
            description_len: 0,
            sources_off: 0,
            sources_len: 0,
        });
        index.push(IndexEntry {
            group: r.group,
            element: r.element,
            creator_hash: creator_hash((!r.creator.is_empty()).then_some(r.creator)),
            record_idx: i as u32,
        });
    }
    index.sort_by_key(|e| (e.group, e.element, e.creator_hash));
    Dictionary {
        index,
        records,
        strings,
    }
}

fn bytes(d: &Dictionary) -> Vec<u8> {
    let body = rkyv::to_bytes::<rkyv::rancor::Error>(d).unwrap();
    let mut out = FileHeader::new(body.len() as u64).to_bytes().to_vec();
    out.extend_from_slice(&body);
    out
}

fn open(b: &[u8]) -> Result<DmapDict, DmapError> {
    let mut aligned = rkyv::util::AlignedVec::<16>::new();
    aligned.extend_from_slice(b);
    DmapDict::from_static(Box::leak(Box::new(aligned)).as_slice())
}

fn base() -> Vec<Rec> {
    vec![
        Rec {
            group: 0x0008,
            element: 0x0005,
            creator: "",
            name: b"Public",
        },
        Rec {
            group: 0x0029,
            element: 0x0008,
            creator: "ACME",
            name: b"Private",
        },
        Rec {
            group: 0x0029,
            element: 0x0009,
            creator: "ACME",
            name: b"Other",
        },
    ]
}

fn expect_bad(r: Result<DmapDict, DmapError>, needle: &str) {
    match r {
        Err(DmapError::BadArchive(m)) => assert!(m.contains(needle), "{m}"),
        Err(e) => panic!("wrong error: {e}"),
        Ok(_) => panic!("accepted a bad archive (wanted {needle})"),
    }
}

#[test]
fn consistent_archive_opens() {
    let d = open(&bytes(&dictionary(&base()))).unwrap();
    assert_eq!(d.len(), 3);
    assert_eq!(
        d.lookup(0x0029, 0x0008, Some("acme")).unwrap().name(),
        "Private"
    );
}

#[test]
fn unsorted_or_duplicate_index_is_rejected() {
    let mut d = dictionary(&base());
    d.index.swap(0, 2);
    expect_bad(open(&bytes(&d)), "sorted and unique");
    let mut d = dictionary(&base());
    d.index[2] = d.index[1];
    expect_bad(open(&bytes(&d)), "sorted and unique");
}

#[test]
fn record_idx_out_of_range_is_rejected() {
    let mut d = dictionary(&base());
    d.index[1].record_idx = 99;
    expect_bad(open(&bytes(&d)), "record_idx 99");
}

#[test]
fn string_range_outside_pool_is_rejected() {
    let mut d = dictionary(&base());
    d.records[0].name_len = 60_000;
    expect_bad(open(&bytes(&d)), "outside");
    let mut d = dictionary(&base());
    d.records[0].description_off = u32::MAX;
    d.records[0].description_len = 10;
    expect_bad(open(&bytes(&d)), "description");
}

#[test]
fn invalid_utf8_is_rejected_not_returned_as_empty() {
    let recs = vec![Rec {
        group: 0x0008,
        element: 0x0005,
        creator: "",
        name: &[0xFF, 0xFE],
    }];
    expect_bad(open(&bytes(&dictionary(&recs))), "UTF-8");
}

#[test]
fn index_key_must_match_its_record() {
    let mut d = dictionary(&base());
    d.records[0].element = 0x0006;
    expect_bad(open(&bytes(&d)), "does not match");
    // Creator changed under the same hash key.
    let mut d = dictionary(&base());
    d.index[2].creator_hash ^= 1;
    expect_bad(open(&bytes(&d)), "does not match");
}

#[test]
fn odd_group_without_creator_is_rejected() {
    let recs = vec![Rec {
        group: 0x0029,
        element: 0x0010,
        creator: "",
        name: b"Bogus",
    }];
    expect_bad(
        open(&bytes(&dictionary(&recs))),
        "without a private creator",
    );
}

#[test]
fn header_body_len_and_reserved_bytes_are_checked() {
    let good = bytes(&dictionary(&base()));
    let mut b = good.clone();
    b.extend_from_slice(&[0; 8]);
    expect_bad(open(&b), "body_len");
    let mut b = good.clone();
    b.truncate(good.len() - 8);
    expect_bad(open(&b), "body_len");
    let mut b = good.clone();
    b[6] = 1;
    expect_bad(open(&b), "reserved");
    let mut b = good.clone();
    b[16] = 1;
    expect_bad(open(&b), "reserved");
    let mut b = good;
    b[30] = 1;
    expect_bad(open(&b), "reserved");
}

#[test]
fn private_lookup_accepts_the_element_as_written_in_a_file() {
    let d = open(&bytes(&dictionary(&base()))).unwrap();
    for e in [0x0008, 0x1008, 0xFF08] {
        let v = d
            .lookup(0x0029, e, Some("ACME"))
            .unwrap_or_else(|| panic!("{e:04X}"));
        assert_eq!(v.name(), "Private");
    }
    // Public lookups are not reduced.
    assert!(d.lookup(0x0008, 0x1005, None).is_none());
    // And a private lookup with no creator is a miss.
    assert!(d.lookup(0x0029, 0x0008, None).is_none());
}

#[test]
fn creator_hash_collision_is_not_a_match() {
    // Find two distinct canonical creators with the same 32-bit hash.
    let mut seen = std::collections::HashMap::new();
    let (a, b) = (0u32..)
        .find_map(|i| {
            let c = format!("C{i}");
            seen.insert(creator_hash(Some(&c)), c.clone())
                .map(|p| (p, c))
        })
        .unwrap();
    let a: &'static str = Box::leak(a.into_boxed_str());
    let recs = vec![Rec {
        group: 0x0029,
        element: 0x0008,
        creator: a,
        name: b"A",
    }];
    let d = open(&bytes(&dictionary(&recs))).unwrap();
    assert_eq!(d.lookup(0x0029, 0x0008, Some(a)).unwrap().name(), "A");
    assert!(d.lookup(0x0029, 0x0008, Some(&b)).is_none());
}
