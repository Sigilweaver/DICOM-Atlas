//! Creator canonicalization and the creator hash.
//!
//! Both are part of the file format: the hash is stored in the index.
//! Canonicalization is ASCII-only so it does not depend on the Unicode tables
//! of the Rust version that built the reader or writer.

/// Collapse runs of ASCII whitespace to one space, trim, and uppercase ASCII
/// letters. All other bytes (including non-ASCII characters) are unchanged.
pub fn canonicalize_creator(s: &str) -> String {
    s.split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// FNV-1a 32 of the canonical creator; `0` for no creator.
pub fn creator_hash(creator: Option<&str>) -> u32 {
    match creator {
        None => 0,
        Some(c) => fnv1a32(canonicalize_creator(c).as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_only() {
        assert_eq!(
            canonicalize_creator("  Siemens \t MR\n header "),
            "SIEMENS MR HEADER"
        );
        // Non-ASCII whitespace and letters are left alone.
        assert_eq!(canonicalize_creator("a\u{a0}b"), "A\u{a0}B");
        assert_eq!(canonicalize_creator("stra\u{df}e"), "STRA\u{df}E");
        assert_eq!(
            canonicalize_creator("\u{201c}x\u{201d}"),
            "\u{201c}X\u{201d}"
        );
    }

    #[test]
    fn hash_known_values() {
        assert_eq!(creator_hash(None), 0);
        // FNV-1a 32 of "A".
        assert_eq!(creator_hash(Some("a")), 0xC40B_F6CC);
    }
}
