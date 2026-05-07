//! Integration tests against the real tags.dmap produced by the compiler.
//!
//! These run only if `tags.dmap` exists at the workspace root (i.e. the
//! scraper + compiler have been executed). They are a no-op otherwise.

use std::path::PathBuf;

use dicom_map::DmapDict;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn dmap_path() -> PathBuf {
    workspace_root().join("tags.dmap")
}

#[test]
fn opens_and_reports_len() {
    let path = dmap_path();
    if !path.exists() {
        eprintln!("skip: {} not present", path.display());
        return;
    }
    let d = DmapDict::open(&path).expect("open dmap");
    assert!(d.len() > 5000, "expected >5000 entries, got {}", d.len());
}

#[test]
fn lookup_known_standard_tag() {
    let path = dmap_path();
    if !path.exists() {
        return;
    }
    let d = DmapDict::open(&path).expect("open dmap");
    // (0008,0005) SpecificCharacterSet — core DICOM tag, always present.
    let v = d
        .lookup(0x0008, 0x0005, None)
        .expect("SpecificCharacterSet missing");
    assert_eq!(v.keyword(), "SpecificCharacterSet");
    assert_eq!(v.vr(), "CS");
    assert_eq!(v.creator(), "");
}

#[test]
fn lookup_known_private_tag() {
    let path = dmap_path();
    if !path.exists() {
        return;
    }
    let d = DmapDict::open(&path).expect("open dmap");
    // From the Siemens FLUOROSPOT PDF: (0021,xx08) under
    // "Siemens: Thorax/Multix FD Lab Settings" → "Auto Window Flag", VR=US
    let creator = "Siemens: Thorax/Multix FD Lab Settings";
    let v = d
        .lookup(0x0021, 0x0008, Some(creator))
        .expect("Auto Window Flag missing");
    assert_eq!(v.vr(), "US");
    assert_eq!(v.name(), "Auto Window Flag");
    assert!(v.is_block_offset());
}

#[test]
fn miss_returns_none() {
    let path = dmap_path();
    if !path.exists() {
        return;
    }
    let d = DmapDict::open(&path).expect("open dmap");
    assert!(d.lookup(0xFFFF, 0xFFFE, None).is_none());
}

/// Per-creator smoke tests: at least one hand-verified tag per major family.
///
/// Each tuple is `(group, element_low, creator, expected_vr, expected_name_fragment)`.
/// Elements use the low byte (private block offset form, PS3.5 §7.8.1).
fn known_tags() -> &'static [(u16, u16, &'static str, &'static str, &'static str)] {
    &[
        // GE imaging families
        (0x0019, 0x0001, "GEMS_DL_IMG_01", "DS", "angle value"),
        (0x0019, 0x001E, "GEMS_ACQU_01", "DS", "FOV"),
        // Philips MR
        (
            0x2005,
            0x000D,
            "PHILIPS MR IMAGING DD 001",
            "FL",
            "Scale Intercept",
        ),
        (
            0x2001,
            0x0001,
            "PHILIPS IMAGING DD 001",
            "FL",
            "Chemical Shift",
        ),
        // Siemens (mixed-case creator should canonicalize)
        (
            0x0021,
            0x0008,
            "Siemens: Thorax/Multix FD Lab Settings",
            "US",
            "Auto Window",
        ),
        // Canon Medical (PDF-sourced, verified against 2G985-* conformance docs)
        (0x7FE1, 0x0010, "CANON MDW NON-IMAGE", "OB", "US Private Data"),
        (0x7099, 0x0002, "CANON_MEC_MG3", "CS", "Miss Shot Status"),
        // Acuson / Siemens ultrasound (PDF-sourced, verified against SC2000 conformance docs)
        (
            0x0119,
            0x0001,
            "SIEMENS ULTRASOUND SC2000",
            "OB",
            "Common Acoustic Meta Information",
        ),
    ]
}

#[test]
fn lookup_known_tag_per_family() {
    let path = dmap_path();
    if !path.exists() {
        return;
    }
    let d = DmapDict::open(&path).expect("open dmap");
    let mut failures: Vec<String> = Vec::new();
    for &(g, e, creator, want_vr, want_name_frag) in known_tags() {
        match d.lookup(g, e, Some(creator)) {
            Some(v) => {
                if v.vr() != want_vr {
                    failures.push(format!(
                        "({g:04X},xx{e:02X}) {creator:?}: expected VR {want_vr}, got {}",
                        v.vr()
                    ));
                }
                if !v
                    .name()
                    .to_lowercase()
                    .contains(&want_name_frag.to_lowercase())
                {
                    failures.push(format!(
                        "({g:04X},xx{e:02X}) {creator:?}: expected name to contain {want_name_frag:?}, got {:?}",
                        v.name()
                    ));
                }
            }
            None => failures.push(format!("({g:04X},xx{e:02X}) {creator:?}: not found")),
        }
    }
    assert!(
        failures.is_empty(),
        "tag lookup failures:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn creator_lookup_is_case_insensitive() {
    let path = dmap_path();
    if !path.exists() {
        return;
    }
    let d = DmapDict::open(&path).expect("open dmap");
    let a = d.lookup(0x2001, 0x0001, Some("PHILIPS IMAGING DD 001"));
    let b = d.lookup(0x2001, 0x0001, Some("Philips Imaging DD 001"));
    let c = d.lookup(0x2001, 0x0001, Some("philips imaging dd 001"));
    assert!(a.is_some());
    assert!(b.is_some());
    assert!(c.is_some());
    assert_eq!(a.as_ref().unwrap().vr(), b.as_ref().unwrap().vr());
    assert_eq!(b.as_ref().unwrap().vr(), c.as_ref().unwrap().vr());
}

#[test]
fn dict_size_is_sensible() {
    let path = dmap_path();
    if !path.exists() {
        return;
    }
    let d = DmapDict::open(&path).expect("open dmap");
    // We currently ship > 9000 combined (public + private) entries.
    assert!(d.len() >= 9000, "expected >= 9000 entries, got {}", d.len());
}

#[test]
fn public_tag_has_no_creator() {
    let path = dmap_path();
    if !path.exists() {
        return;
    }
    let d = DmapDict::open(&path).expect("open dmap");
    let v = d.lookup(0x0008, 0x0005, None).expect("public tag missing");
    assert_eq!(v.creator(), "");
    assert_eq!(v.keyword(), "SpecificCharacterSet");
}

#[test]
fn private_tag_has_creator_string() {
    let path = dmap_path();
    if !path.exists() {
        return;
    }
    let d = DmapDict::open(&path).expect("open dmap");
    let v = d
        .lookup(0x0019, 0x0001, Some("GEMS_DL_IMG_01"))
        .expect("private tag missing");
    assert!(!v.creator().is_empty());
    assert!(v.is_block_offset());
}
