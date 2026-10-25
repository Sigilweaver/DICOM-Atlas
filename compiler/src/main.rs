//! `dmap-compile` - freeze DICOM tag records into a .dmap file.
//!
//! Sources (pick any combination):
//!   --standard <path>    PS3.6 attributes.json (public tags)
//!   --resolved <path>    ResolvedTag JSON-L from scraper.resolve (private tags)
//!   --csv      <path>    tags.csv produced by --export-csv (replaces both above)
//!
//! Inputs are validated strictly: a tag, VR, VM, hex or Y/N field that does
//! not match its grammar stops the build with the file, row and column.
//! Repeating-group patterns in PS3.6 (`(60XX,0010)`) cannot be addressed by
//! the index; they are skipped, counted and listed on stderr.
//!
//! Outputs:
//!   --out        <path>  write .dmap archive
//!   --export-csv <path>  also write a human-editable tags.csv

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use dicom_map::schema::{
    canonicalize_creator, creator_hash, Dictionary, FileHeader, IndexEntry, TagRecord, VrCode,
    VM_UNBOUNDED,
};
use rkyv::rancor::Error;
use serde::Deserialize;

mod strict;
use strict::{
    fit_len, parse_csv_element, parse_hex4, parse_standard_tag, parse_vm, parse_vr, InputError,
    Loc, StdTag, Vm,
};

// ---------------------------------------------------------------------------
// CLI args

#[derive(Parser)]
#[command(
    name = "dmap-compile",
    about = "Compile DICOM tag records into a .dmap archive"
)]
struct Args {
    /// PS3.6 standard attributes.json (public tags).
    #[arg(long)]
    standard: Option<PathBuf>,
    /// ResolvedTag JSON-L from scraper.resolve (private tags).
    #[arg(long)]
    resolved: Option<PathBuf>,
    /// Read from a previously exported tags.csv (replaces --standard + --resolved).
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Output .dmap path.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Also export all tags to a human-editable CSV file.
    #[arg(long)]
    export_csv: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Unified in-memory row

struct Row {
    loc: Loc,
    group: u16,
    element: u16,
    element_is_block_offset: bool,
    private_creator: String,
    /// Validated VR text (an exact VR, a multi-VR form, or one of the two
    /// "no VR" spellings of PS3.6).
    vr: String,
    vm: Vm,
    keyword: String,
    name: String,
    description: String,
    retired: bool,
    vendors: Vec<String>,
    sources: Vec<String>,
}

// ---------------------------------------------------------------------------
// Input deserialization helpers

#[derive(Debug, Deserialize)]
struct StandardTag {
    tag: String,
    name: String,
    keyword: String,
    #[serde(rename = "valueRepresentation")]
    vr: String,
    #[serde(rename = "valueMultiplicity")]
    vm: String,
    retired: String,
}

/// Every key is required: a missing key is a deserialization error, not a
/// default.
#[derive(Debug, Deserialize)]
struct ResolvedTag {
    group: u16,
    element: u16,
    element_is_block_offset: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    private_creator: Option<String>,
    keyword: String,
    name: String,
    vr: String,
    vm: String,
    description: String,
    retired: bool,
    vendors: Vec<String>,
    sources: Vec<String>,
}

// ---------------------------------------------------------------------------
// Shared field checks

fn bad_field(loc: &Loc, column: &'static str, expected: &'static str, value: &str) -> InputError {
    InputError::BadField {
        loc: loc.clone(),
        column,
        expected,
        value: value.to_owned(),
    }
}

fn check_text(loc: &Loc, column: &'static str, s: &str, max: usize) -> Result<(), InputError> {
    if s.contains('\0') {
        return Err(InputError::NulInField {
            loc: loc.clone(),
            column,
        });
    }
    if fit_len(s.len(), max).is_none() {
        return Err(InputError::FieldTooLong {
            loc: loc.clone(),
            column,
            len: s.len(),
            max,
        });
    }
    Ok(())
}

/// Validate VR/VM and the text columns, and require a creator for odd groups.
#[allow(clippy::too_many_arguments)]
fn build_row(
    loc: Loc,
    group: u16,
    element: u16,
    element_is_block_offset: bool,
    private_creator: &str,
    vr: &str,
    vm: &str,
    keyword: String,
    name: String,
    description: String,
    retired: bool,
    vendors: Vec<String>,
    sources: Vec<String>,
) -> Result<Row, InputError> {
    let public = private_creator.trim().is_empty();
    if public && group % 2 == 1 {
        return Err(InputError::MissingCreator {
            loc,
            group,
            element,
        });
    }
    let vr_code = parse_vr(vr, public).map_err(|e| bad_field(&loc, "vr", e, vr))?;
    // Store the canonical spelling so tags.csv is stable across spellings.
    let vr = if vr_code == VrCode::Unknown {
        vr.to_owned()
    } else {
        vr_code.as_str().to_owned()
    };
    let vm = parse_vm(vm, public).map_err(|e| bad_field(&loc, "vm", e, vm))?;
    check_text(&loc, "keyword", &keyword, u16::MAX as usize)?;
    check_text(&loc, "name", &name, u16::MAX as usize)?;
    check_text(&loc, "private_creator", private_creator, u16::MAX as usize)?;
    check_text(&loc, "description", &description, u32::MAX as usize)?;
    let src = sources.join("|");
    check_text(&loc, "sources", &src, u32::MAX as usize)?;
    Ok(Row {
        loc,
        group,
        element,
        element_is_block_offset,
        private_creator: private_creator.trim().to_owned(),
        vr,
        vm,
        keyword,
        name,
        description,
        retired,
        vendors,
        sources,
    })
}

// ---------------------------------------------------------------------------
// Loaders

/// Repeating-group patterns that were left out of the index.
struct Skipped {
    tags: Vec<String>,
}

fn load_standard(path: &PathBuf) -> Result<(Vec<Row>, Skipped)> {
    let origin = path.display().to_string();
    let data = std::fs::read(path).with_context(|| format!("reading {origin}"))?;
    let tags: Vec<StandardTag> =
        serde_json::from_slice(&data).with_context(|| format!("parsing {origin}"))?;
    let mut rows = Vec::with_capacity(tags.len());
    let mut skipped = Skipped { tags: Vec::new() };
    for (i, t) in tags.into_iter().enumerate() {
        let loc = Loc {
            origin: origin.clone(),
            row: i + 1,
        };
        let (g, e) = match parse_standard_tag(&t.tag) {
            Some(StdTag::Concrete(g, e)) => (g, e),
            Some(StdTag::Repeating) => {
                skipped.tags.push(t.tag);
                continue;
            }
            None => return Err(InputError::BadTag { loc, value: t.tag }.into()),
        };
        let retired = match t.retired.as_str() {
            "Y" => true,
            "N" => false,
            other => return Err(bad_field(&loc, "retired", "`Y` or `N`", other).into()),
        };
        rows.push(build_row(
            loc,
            g,
            e,
            false,
            "",
            &t.vr,
            &t.vm,
            t.keyword,
            t.name,
            String::new(),
            retired,
            vec!["standard".to_owned()],
            vec!["PS3.6".to_owned()],
        )?);
    }
    Ok((rows, skipped))
}

fn load_resolved(path: &PathBuf) -> Result<Vec<Row>> {
    let origin = path.display().to_string();
    let file = File::open(path).with_context(|| format!("reading {origin}"))?;
    let mut rows = Vec::new();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("reading {origin} line {}", i + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let loc = Loc {
            origin: origin.clone(),
            row: i + 1,
        };
        let t: ResolvedTag =
            serde_json::from_str(&line).with_context(|| format!("{loc}: invalid record"))?;
        rows.push(build_row(
            loc,
            t.group,
            t.element,
            t.element_is_block_offset,
            t.private_creator.as_deref().unwrap_or(""),
            &t.vr,
            &t.vm,
            t.keyword,
            t.name,
            t.description,
            t.retired,
            t.vendors,
            t.sources,
        )?);
    }
    Ok(rows)
}

fn load_csv(path: &PathBuf) -> Result<Vec<Row>> {
    let origin = path.display().to_string();
    let mut rdr = csv::Reader::from_path(path).with_context(|| format!("reading {origin}"))?;
    let mut rows = Vec::new();
    for (i, result) in rdr.records().enumerate() {
        let loc = Loc {
            origin: origin.clone(),
            row: i + 1,
        };
        let rec = result.with_context(|| format!("{loc}: unreadable record"))?;
        // columns: group,element,private_creator,vr,vm,keyword,name,description,retired,vendors,sources
        if rec.len() != 11 {
            return Err(bad_field(
                &loc,
                "(row)",
                "11 columns",
                &format!("{} columns", rec.len()),
            )
            .into());
        }
        let group_s = &rec[0];
        let group =
            parse_hex4(group_s).ok_or_else(|| bad_field(&loc, "group", "4 hex digits", group_s))?;
        let elem_s = &rec[1];
        let (element, element_is_block_offset) = parse_csv_element(elem_s).ok_or_else(|| {
            bad_field(
                &loc,
                "element",
                "4 hex digits, or `xx` and 2 hex digits",
                elem_s,
            )
        })?;
        let retired = match &rec[8] {
            "Y" => true,
            "N" => false,
            other => return Err(bad_field(&loc, "retired", "`Y` or `N`", other).into()),
        };
        rows.push(build_row(
            loc,
            group,
            element,
            element_is_block_offset,
            &rec[2],
            &rec[3],
            &rec[4],
            rec[5].to_owned(),
            rec[6].to_owned(),
            rec[7].to_owned(),
            retired,
            split_pipe(&rec[9]),
            split_pipe(&rec[10]),
        )?);
    }
    Ok(rows)
}

fn split_pipe(s: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![];
    }
    s.split('|').map(|x| x.to_owned()).collect()
}

// ---------------------------------------------------------------------------
// CSV export
// Columns: group,element,private_creator,vr,vm,keyword,name,description,retired,vendors,sources
// Sorted:  public tags (empty creator) first, then by creator / group / element.
// Element: concrete tags as 4-char uppercase hex; block-relative private tags as xx<2-char hex>.

fn vm_text(v: &Vm) -> String {
    if *v == strict::VM_UNDECLARED {
        String::new()
    } else if v.max == VM_UNBOUNDED && v.step > 1 {
        format!("{}-{}n", v.min, v.step)
    } else if v.max == VM_UNBOUNDED {
        format!("{}-n", v.min)
    } else if v.min == v.max {
        v.min.to_string()
    } else {
        format!("{}-{}", v.min, v.max)
    }
}

fn write_csv(path: &PathBuf, rows: &[Row]) -> Result<()> {
    let mut wtr =
        csv::Writer::from_path(path).with_context(|| format!("creating {}", path.display()))?;
    wtr.write_record([
        "group",
        "element",
        "private_creator",
        "vr",
        "vm",
        "keyword",
        "name",
        "description",
        "retired",
        "vendors",
        "sources",
    ])?;

    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        a.private_creator
            .cmp(&b.private_creator)
            .then(a.group.cmp(&b.group))
            .then(a.element.cmp(&b.element))
    });

    for r in sorted {
        let elem_str = if r.element_is_block_offset {
            format!("xx{:02X}", r.element)
        } else {
            format!("{:04X}", r.element)
        };
        wtr.write_record([
            &format!("{:04X}", r.group),
            &elem_str,
            &r.private_creator,
            &r.vr,
            &vm_text(&r.vm),
            &r.keyword,
            &r.name,
            &r.description,
            &(if r.retired { "Y" } else { "N" }).to_owned(),
            &r.vendors.join("|"),
            &r.sources.join("|"),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Compiler helpers

struct StringPool {
    bytes: Vec<u8>,
}

impl StringPool {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(1 << 20),
        }
    }
    fn intern(&mut self, s: &str) -> Result<(u32, u32)> {
        if s.is_empty() {
            return Ok((0, 0));
        }
        let off = u32::try_from(self.bytes.len())
            .map_err(|_| anyhow::anyhow!("string pool exceeds 4 GiB"))?;
        let len = u32::try_from(s.len()).map_err(|_| {
            anyhow::anyhow!("string of {} bytes exceeds the 32-bit length", s.len())
        })?;
        self.bytes.extend_from_slice(s.as_bytes());
        Ok((off, len))
    }
}

fn narrow_u16(row: &Row, column: &'static str, len: u32) -> Result<u16> {
    u16::try_from(len).map_err(|_| {
        InputError::FieldTooLong {
            loc: row.loc.clone(),
            column,
            len: len as usize,
            max: u16::MAX as usize,
        }
        .into()
    })
}

fn compile(rows: &[Row], out: &PathBuf) -> Result<()> {
    let mut pool = StringPool::new();
    let mut records: Vec<TagRecord> = Vec::with_capacity(rows.len());
    let mut entries: Vec<IndexEntry> = Vec::with_capacity(rows.len());

    for r in rows {
        let creator = if r.private_creator.is_empty() {
            None
        } else {
            Some(r.private_creator.as_str())
        };
        let canon = creator.map(canonicalize_creator);
        let ch = creator_hash(creator);
        let vr = parse_vr(&r.vr, true).map_err(|e| bad_field(&r.loc, "vr", e, &r.vr))?;
        let (k_off, k_len) = pool.intern(&r.keyword)?;
        let (n_off, n_len) = pool.intern(&r.name)?;
        let (c_off, c_len) = match canon.as_deref() {
            Some(c) => pool.intern(c)?,
            None => (0, 0),
        };
        let (d_off, d_len) = pool.intern(&r.description)?;
        let (s_off, s_len) = pool.intern(&r.sources.join("|"))?;
        records.push(TagRecord {
            group: r.group,
            element: r.element,
            element_is_block_offset: r.element_is_block_offset,
            retired: r.retired,
            vr,
            vm_min: r.vm.min,
            vm_max: r.vm.max,
            vm_step: r.vm.step,
            keyword_off: k_off,
            keyword_len: narrow_u16(r, "keyword", k_len)?,
            name_off: n_off,
            name_len: narrow_u16(r, "name", n_len)?,
            creator_off: c_off,
            creator_len: narrow_u16(r, "private_creator", c_len)?,
            description_off: d_off,
            description_len: d_len,
            sources_off: s_off,
            sources_len: s_len,
        });
        entries.push(IndexEntry {
            group: r.group,
            element: r.element,
            creator_hash: ch,
            record_idx: u32::try_from(records.len() - 1)
                .map_err(|_| anyhow::anyhow!("more than 2^32 records"))?,
        });
    }

    entries.sort_by_key(|e| (e.group, e.element, e.creator_hash));

    let dict = Dictionary {
        index: entries,
        records,
        strings: pool.bytes,
    };

    eprintln!(
        "compiling: {} tags, {} index entries, {} bytes in string pool",
        dict.records.len(),
        dict.index.len(),
        dict.strings.len()
    );

    let body =
        rkyv::to_bytes::<Error>(&dict).map_err(|e| anyhow::anyhow!("rkyv serialize: {e}"))?;
    let hdr = FileHeader::new(body.len() as u64);

    let mut f = File::create(out)?;
    f.write_all(&hdr.to_bytes())?;
    f.write_all(&body)?;
    f.flush()?;

    eprintln!(
        "wrote {} ({} bytes body + {} header)",
        out.display(),
        body.len(),
        FileHeader::SIZE
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Normalisation + deduplication
//
// Two things in the input data need handling before compilation:
//
// 1. Some private tags in resolved_pydicom_backfilled.jsonl have
//    element_is_block_offset=false and a concrete element address (e.g. 0x1011
//    for block 0x10, offset 0x11). The lookup API always uses the block offset
//    (low byte, 0x11), so these tags would be unfindable in the compiled index.
//    Fix: normalise all private-tag elements to their block offset (low byte).
//
// 2. Some creator strings that differ only in case (e.g. "SPI RELEASE 1" vs
//    "SPI Release 1") canonicalise to the same string and therefore produce the
//    same creator_hash, so two rows can share one index key. Rows that agree
//    on VR and name are merged (the entry with PDF sources, then a real name,
//    is kept) and counted. Rows that disagree on VR, or on name after
//    case-folding, are an error: the data must say which one is right.
//    Two different canonical creators with the same hash are also an error.

#[derive(Default)]
struct DedupReport {
    merged: usize,
}

fn is_placeholder_name(n: &str) -> bool {
    n.is_empty() || n == "Unknown"
}

fn describe(r: &Row) -> String {
    format!(
        "({:04X},{}{:02X}) creator {:?}: {} name {:?} vr {:?}",
        r.group,
        if r.element_is_block_offset { "xx" } else { "" },
        r.element & 0xFF,
        r.private_creator,
        r.loc,
        r.name,
        r.vr
    )
}

fn normalize_and_dedup(rows: Vec<Row>) -> Result<(Vec<Row>, DedupReport), InputError> {
    let mut map: BTreeMap<(u16, u16, u32), Row> = BTreeMap::new();
    let mut conflicts: Vec<String> = Vec::new();
    let mut report = DedupReport::default();
    for mut r in rows {
        // Normalise private-tag element to the block offset (low byte).
        if !r.private_creator.is_empty() && !r.element_is_block_offset {
            r.element &= 0xFF;
            r.element_is_block_offset = true;
        }
        let ch = creator_hash(if r.private_creator.is_empty() {
            None
        } else {
            Some(r.private_creator.as_str())
        });
        let key = (r.group, r.element, ch);
        let replace = match map.get(&key) {
            None => true,
            Some(prev) => {
                if canonicalize_creator(&prev.private_creator)
                    != canonicalize_creator(&r.private_creator)
                {
                    return Err(InputError::HashCollision {
                        group: r.group,
                        element: r.element,
                        hash: ch,
                        a: prev.private_creator.clone(),
                        b: r.private_creator.clone(),
                    });
                }
                let prev_vr = parse_vr(&prev.vr, true).ok();
                let curr_vr = parse_vr(&r.vr, true).ok();
                if prev_vr != curr_vr {
                    conflicts.push(format!(
                        "  VR differs:\n    {}\n    {}",
                        describe(prev),
                        describe(&r)
                    ));
                } else if !is_placeholder_name(&prev.name)
                    && !is_placeholder_name(&r.name)
                    && prev.name.trim().to_lowercase() != r.name.trim().to_lowercase()
                {
                    conflicts.push(format!(
                        "  name differs:\n    {}\n    {}",
                        describe(prev),
                        describe(&r)
                    ));
                }
                report.merged += 1;
                let curr_pdf = r.sources.iter().any(|s| s != "pydicom");
                let prev_pdf = prev.sources.iter().any(|s| s != "pydicom");
                if curr_pdf && !prev_pdf {
                    true // PDF source beats pydicom-only
                } else if !curr_pdf && prev_pdf {
                    false // keep existing PDF entry
                } else {
                    // Same source type: prefer a real name over a placeholder.
                    !is_placeholder_name(&r.name) && is_placeholder_name(&prev.name)
                }
            }
        };
        if replace {
            map.insert(key, r);
        }
    }
    if !conflicts.is_empty() {
        return Err(InputError::Conflicts(conflicts));
    }
    // BTreeMap iterates in key order, so the result is already sorted by
    // (group, element, creator_hash) - matching the index sort in compile().
    Ok((map.into_values().collect(), report))
}

// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let args = Args::parse();

    let rows: Vec<Row> = if let Some(p) = args.csv.as_ref() {
        load_csv(p)?
    } else {
        let mut rows = Vec::new();
        if let Some(p) = args.standard.as_ref() {
            let (std_rows, skipped) = load_standard(p)?;
            if !skipped.tags.is_empty() {
                eprintln!(
                    "skipped {} repeating-group/element patterns (the index addresses concrete tags only): {}",
                    skipped.tags.len(),
                    skipped.tags.join(" ")
                );
            }
            rows.extend(std_rows);
        }
        if let Some(p) = args.resolved.as_ref() {
            rows.extend(load_resolved(p)?);
        }
        rows
    };

    eprintln!("loaded {} rows", rows.len());

    let (rows, report) = normalize_and_dedup(rows)?;
    eprintln!(
        "after dedup: {} rows ({} duplicates with identical VR and name merged)",
        rows.len(),
        report.merged
    );

    if let Some(p) = args.export_csv.as_ref() {
        write_csv(p, &rows)?;
        eprintln!("exported CSV to {}", p.display());
    }

    if let Some(p) = args.out.as_ref() {
        compile(&rows, p)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(creator: &str, name: &str) -> Row {
        Row {
            loc: Loc {
                origin: "t".into(),
                row: 1,
            },
            group: 0x0029,
            element: 0x08,
            element_is_block_offset: true,
            private_creator: creator.to_owned(),
            vr: "US".into(),
            vm: Vm {
                min: 1,
                max: 1,
                step: 1,
            },
            keyword: String::new(),
            name: name.to_owned(),
            description: String::new(),
            retired: false,
            vendors: vec![],
            sources: vec![],
        }
    }

    #[test]
    fn distinct_creators_with_equal_hash_are_an_error() {
        // Birthday search for two canonical creators with the same FNV-1a 32.
        let mut seen: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
        let (a, b) = (0u32..)
            .find_map(|i| {
                let c = format!("C{i}");
                let h = creator_hash(Some(&c));
                seen.insert(h, c.clone()).map(|prev| (prev, c))
            })
            .unwrap();
        assert_ne!(canonicalize_creator(&a), canonicalize_creator(&b));
        let err = match normalize_and_dedup(vec![row(&a, "X"), row(&b, "X")]) {
            Err(e) => e,
            Ok(_) => panic!("collision not detected"),
        };
        assert!(matches!(err, InputError::HashCollision { .. }), "{err}");
    }

    #[test]
    fn vm_text_round_trips_through_the_parser() {
        for t in ["1", "3", "1-2", "0-n", "1-n", "2-2n", "47-47n"] {
            let vm = parse_vm(t, false).unwrap();
            assert_eq!(vm_text(&vm), t);
        }
        assert_eq!(vm_text(&strict::VM_UNDECLARED), "");
    }
}
