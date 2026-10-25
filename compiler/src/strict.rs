//! Strict parsers for compiler input fields.
//!
//! Every parser either returns a value that exactly matches the documented
//! grammar or says what it expected. Nothing here pads, truncates, filters
//! characters or substitutes a default.
//!
//! Sources cited by the grammars below:
//! - DICOM PS3.6 2024b (Data Dictionary), Section 6 "Registry of DICOM Data
//!   Elements", Table 6-1: tags are written `(GGGG,EEEE)`, and a repeating
//!   group or element is written with `x` (or `X`) in the varying hex
//!   nibbles, e.g. `(60xx,0010)`. The same table lists the retired
//!   `(50xx,....)`, `(7Fxx,....)` and `(1000,xxx.)` patterns and the VM
//!   column forms `k`, `k-m`, `k-n` and `k-kn` (for example `2-2n`).
//! - DICOM PS3.5 2024b (Data Structures and Encoding), Section 7.6 "Repeating
//!   Groups": the `60xx` overlay groups (6000-601E) repeat; the `50xx` curve
//!   groups are retired.
//! - DICOM PS3.5 2024b, Section 7.8.1 "Private Data Element Tags": private data
//!   elements live in odd groups and are addressed through a private creator.
//! - DICOM PS3.5 2024b, Section 6.4 "Value Multiplicity (VM) and Delimitation":
//!   a VM written as two numbers separated by a dash is a range (`1-10`).

use dicom_map::{Vm, VM_UNBOUNDED};

/// Where in which input a problem was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loc {
    pub origin: String,
    /// 1-based data row (JSON array index, JSON-L line, or CSV record).
    pub row: usize,
}

impl std::fmt::Display for Loc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: row {}", self.origin, self.row)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InputError {
    #[error("{loc}: tag {value:?} is neither (GGGG,EEEE) nor a repeating pattern with X nibbles")]
    BadTag { loc: Loc, value: String },
    #[error("{loc}: column `{column}`: expected {expected}, found {value:?}")]
    BadField {
        loc: Loc,
        column: &'static str,
        expected: &'static str,
        value: String,
    },
    #[error(
        "{loc}: private tag ({group:04X},{element:04X}) has no private creator \
         (odd group requires one, PS3.5 2024b 7.8.1)"
    )]
    MissingCreator { loc: Loc, group: u16, element: u16 },
    #[error("{loc}: column `{column}` is {len} bytes, the format stores at most {max}")]
    FieldTooLong {
        loc: Loc,
        column: &'static str,
        len: usize,
        max: usize,
    },
    #[error("{loc}: column `{column}` contains a NUL byte")]
    NulInField { loc: Loc, column: &'static str },
    #[error(
        "creator hash collision at ({group:04X},{element:04X}): {a:?} and {b:?} \
         canonicalise differently but hash to {hash:#010x}"
    )]
    HashCollision {
        group: u16,
        element: u16,
        hash: u32,
        a: String,
        b: String,
    },
    #[error("{} conflicting duplicate entries:\n{}", .0.len(), .0.join("\n"))]
    Conflicts(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdTag {
    Concrete(u16, u16),
    /// Written with at least one `X` nibble (repeating group or element).
    Repeating,
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// Parse `(GGGG,EEEE)` where each position is a hex digit or `X`/`x`.
pub fn parse_standard_tag(s: &str) -> Option<StdTag> {
    let b = s.as_bytes();
    if b.len() != 11 || b[0] != b'(' || b[5] != b',' || b[10] != b')' {
        return None;
    }
    let digits = [&b[1..5], &b[6..10]];
    let mut repeating = false;
    for part in digits {
        for &c in part {
            if c == b'X' || c == b'x' {
                repeating = true;
            } else if !is_hex(c) {
                return None;
            }
        }
    }
    if repeating {
        return Some(StdTag::Repeating);
    }
    let g = u16::from_str_radix(&s[1..5], 16).ok()?;
    let e = u16::from_str_radix(&s[6..10], 16).ok()?;
    Some(StdTag::Concrete(g, e))
}

/// Exactly four hex digits.
pub fn parse_hex4(s: &str) -> Option<u16> {
    if s.len() == 4 && s.bytes().all(is_hex) {
        u16::from_str_radix(s, 16).ok()
    } else {
        None
    }
}

/// CSV element column: four hex digits (concrete) or `xx` plus two hex digits
/// (private block offset). Returns `(element, is_block_offset)`.
pub fn parse_csv_element(s: &str) -> Option<(u16, bool)> {
    if let Some(rest) = s.strip_prefix("xx").or_else(|| s.strip_prefix("XX")) {
        if rest.len() == 2 && rest.bytes().all(is_hex) {
            return Some((u16::from_str_radix(rest, 16).ok()?, true));
        }
        return None;
    }
    parse_hex4(s).map(|e| (e, false))
}

/// Value of `Vm` for a tag whose source declares no VM.
pub const VM_UNDECLARED: Vm = Vm::UNDECLARED;

fn parse_vm_number(s: &str) -> Option<u16> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = s.parse().ok()?;
    // 0xFFFF is reserved for "unbounded".
    u16::try_from(n).ok().filter(|&n| n != VM_UNBOUNDED)
}

/// Parse a value multiplicity.
///
/// Accepted forms (ASCII whitespace anywhere is ignored, `n`/`N` are the same):
/// `k`, `k-m` (m >= k), `k-n`, `k-kn` (same k on both sides; stored as
/// unbounded with step k), and the two spellings that appear in the sources:
/// bare `n` (read as `1-n`) and PS3.6 2024b's `1-n or 1` (read as `1-n`).
/// An empty VM is accepted only when `allow_undeclared` is set, and yields
/// [`VM_UNDECLARED`].
pub fn parse_vm(s: &str, allow_undeclared: bool) -> Result<Vm, &'static str> {
    let t: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if t.is_empty() {
        return if allow_undeclared {
            Ok(VM_UNDECLARED)
        } else {
            Err("a value multiplicity (k, k-m, k-n or k-kn)")
        };
    }
    const EXPECTED: &str = "a value multiplicity (k, k-m, k-n or k-kn)";
    if t.eq_ignore_ascii_case("n") || t.eq_ignore_ascii_case("1-nor1") {
        return Ok(Vm::new(1, VM_UNBOUNDED, 1));
    }
    match t.split_once('-') {
        None => {
            let n = parse_vm_number(&t).ok_or(EXPECTED)?;
            Ok(Vm::new(n, n, 1))
        }
        Some((lo, hi)) => {
            let min = parse_vm_number(lo).ok_or(EXPECTED)?;
            if hi.eq_ignore_ascii_case("n") {
                return Ok(Vm::new(min, VM_UNBOUNDED, 1));
            }
            if let Some(k) = hi.strip_suffix('n').or_else(|| hi.strip_suffix('N')) {
                let k = parse_vm_number(k).ok_or(EXPECTED)?;
                if k == min && k > 0 {
                    return Ok(Vm::new(min, VM_UNBOUNDED, k));
                }
                return Err(EXPECTED);
            }
            let max = parse_vm_number(hi).ok_or(EXPECTED)?;
            if max < min {
                return Err(EXPECTED);
            }
            Ok(Vm::new(min, max, 1))
        }
    }
}

/// The 34 two-letter VRs of PS3.5 2024b Table 6.2-1 and the four multi-VR spellings
/// used by PS3.6 2024b.
const VRS: &[&str] = &[
    "AE",
    "AS",
    "AT",
    "CS",
    "DA",
    "DS",
    "DT",
    "FL",
    "FD",
    "IS",
    "LO",
    "LT",
    "OB",
    "OD",
    "OF",
    "OL",
    "OV",
    "OW",
    "PN",
    "SH",
    "SL",
    "SQ",
    "SS",
    "ST",
    "SV",
    "TM",
    "UC",
    "UI",
    "UL",
    "UN",
    "UR",
    "US",
    "UT",
    "UV",
    "US or SS",
    "OB or OW",
    "US or OW",
    "US or SS or OW",
];

/// VR text stored when the source declares no VR.
pub const NO_VR: &str = "??";

/// Parse a VR column and return its canonical text.
///
/// Exact two-letter VRs and the four PS3.6 2024b multi-VR forms are accepted, plus
/// the one spelling `OB_OW` seen in scraped data. The two ways PS3.6 2024b says "no
/// VR" (an empty cell, or `See Note 2`) are accepted only when
/// `allow_undeclared` is set and give [`NO_VR`].
pub fn parse_vr(s: &str, allow_undeclared: bool) -> Result<&'static str, &'static str> {
    if let Some(v) = VRS.iter().find(|v| **v == s) {
        return Ok(v);
    }
    if s == "OB_OW" {
        return Ok("OB or OW");
    }
    if allow_undeclared && (s.is_empty() || s == "See Note 2") {
        return Ok(NO_VR);
    }
    Err("a DICOM VR (two capital letters, or `US or SS`, `OB or OW`, `US or OW`, `US or SS or OW`)")
}

/// Narrow a string length to the width the format stores.
pub fn fit_len(len: usize, max: usize) -> Option<usize> {
    (len <= max).then_some(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_tag_concrete() {
        assert_eq!(
            parse_standard_tag("(0008,0005)"),
            Some(StdTag::Concrete(0x0008, 0x0005))
        );
        assert_eq!(
            parse_standard_tag("(7fe0,0010)"),
            Some(StdTag::Concrete(0x7FE0, 0x0010))
        );
    }

    #[test]
    fn standard_tag_repeating_is_not_given_an_address() {
        for t in [
            "(60XX,0010)",
            "(0028,04X0)",
            "(1000,XXX0)",
            "(0020,31XX)",
            "(50xx,0010)",
        ] {
            assert_eq!(parse_standard_tag(t), Some(StdTag::Repeating), "{t}");
        }
    }

    #[test]
    fn standard_tag_rejects_other_shapes() {
        for t in [
            "",
            "00080005",
            "(0008,005)",
            "(0008,00055)",
            "0008,0005",
            "(0008;0005)",
            "(00G8,0005)",
            "(0008, 0005)",
        ] {
            assert_eq!(parse_standard_tag(t), None, "{t:?}");
        }
    }

    #[test]
    fn hex4_is_exact() {
        assert_eq!(parse_hex4("0028"), Some(0x28));
        assert_eq!(parse_hex4("ffff"), Some(0xFFFF));
        for s in ["", "28", "002G", "10xx", "0x28", "00280", " 028"] {
            assert_eq!(parse_hex4(s), None, "{s:?}");
        }
    }

    #[test]
    fn csv_element_forms() {
        assert_eq!(parse_csv_element("0010"), Some((0x10, false)));
        assert_eq!(parse_csv_element("xx10"), Some((0x10, true)));
        assert_eq!(parse_csv_element("XXFF"), Some((0xFF, true)));
        for s in ["", "x10", "xx1", "xx100", "10xx", "x010", "00G0", "xxZZ"] {
            assert_eq!(parse_csv_element(s), None, "{s:?}");
        }
    }

    fn vm(min: u16, max: u16, step: u16) -> Vm {
        Vm::new(min, max, step)
    }

    #[test]
    fn vm_forms() {
        assert_eq!(parse_vm("1", false), Ok(vm(1, 1, 1)));
        assert_eq!(parse_vm("3", false), Ok(vm(3, 3, 1)));
        assert_eq!(parse_vm("1-2", false), Ok(vm(1, 2, 1)));
        assert_eq!(parse_vm("1-n", false), Ok(vm(1, VM_UNBOUNDED, 1)));
        assert_eq!(parse_vm("1-N", false), Ok(vm(1, VM_UNBOUNDED, 1)));
        assert_eq!(parse_vm("1- n", false), Ok(vm(1, VM_UNBOUNDED, 1)));
        assert_eq!(parse_vm("0- 1", false), Ok(vm(0, 1, 1)));
        assert_eq!(parse_vm("n", false), Ok(vm(1, VM_UNBOUNDED, 1)));
        assert_eq!(parse_vm("2-2n", false), Ok(vm(2, VM_UNBOUNDED, 2)));
        assert_eq!(parse_vm("47-47n", false), Ok(vm(47, VM_UNBOUNDED, 47)));
        assert_eq!(parse_vm("1-n or 1", false), Ok(vm(1, VM_UNBOUNDED, 1)));
        assert_eq!(parse_vm("256", false), Ok(vm(256, 256, 1)));
    }

    #[test]
    fn vm_rejects_truncated_and_odd_forms() {
        for s in [
            "", "0-", "1-", "2-", "-", "-n", "a", "1-x", "3-2", "2-3n", "0-0n", "1.5", "65535",
            "70000", "1-2-3", "n-n", "1-nn",
        ] {
            assert!(parse_vm(s, false).is_err(), "{s:?}");
        }
    }

    #[test]
    fn vm_empty_only_when_declared_allowed() {
        assert!(parse_vm("", false).is_err());
        assert_eq!(parse_vm("", true), Ok(VM_UNDECLARED));
        assert!(parse_vm("0-", true).is_err());
    }

    #[test]
    fn vr_forms() {
        assert_eq!(parse_vr("US", false), Ok("US"));
        assert_eq!(parse_vr("US or SS", false), Ok("US or SS"));
        assert_eq!(parse_vr("OB or OW", false), Ok("OB or OW"));
        assert_eq!(parse_vr("OB_OW", false), Ok("OB or OW"));
        assert_eq!(parse_vr("US or SS or OW", false), Ok("US or SS or OW"));
        assert_eq!(parse_vr("", true), Ok(NO_VR));
        assert_eq!(parse_vr("See Note 2", true), Ok(NO_VR));
    }

    #[test]
    fn vr_rejects_typos_and_undeclared_when_not_allowed() {
        for s in [
            "ob",
            " US",
            "US ",
            "USED",
            "??",
            "XX",
            "",
            "See Note 2",
            "US/SS",
        ] {
            assert!(parse_vr(s, false).is_err(), "{s:?}");
        }
        assert!(parse_vr("ob", true).is_err());
        assert!(parse_vr("??", true).is_err());
    }
}
