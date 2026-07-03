//! `dmap-compile` - freeze DICOM tag records into a .dmap file.
//!
//! Sources (pick any combination):
//!   --standard <path>    PS3.6 attributes.json (public tags)
//!   --resolved <path>    ResolvedTag JSON-L from scraper.resolve (private tags)
//!   --csv      <path>    tags.csv produced by --export-csv (replaces both above)
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
};
use rkyv::ser::serializers::AllocSerializer;
use rkyv::ser::Serializer;
use serde::Deserialize;

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
    group: u16,
    element: u16,
    element_is_block_offset: bool,
    private_creator: String,
    vr: String,
    vm: String,
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

#[derive(Debug, Deserialize)]
struct ResolvedTag {
    group: u16,
    element: u16,
    #[serde(default)]
    element_is_block_offset: bool,
    private_creator: Option<String>,
    #[serde(default)]
    keyword: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    vr: String,
    #[serde(default)]
    vm: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    retired: bool,
    #[serde(default)]
    vendors: Vec<String>,
    #[serde(default)]
    sources: Vec<String>,
}

// ---------------------------------------------------------------------------
// Loaders

fn load_standard(path: &PathBuf) -> Result<Vec<Row>> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let tags: Vec<StandardTag> = serde_json::from_slice(&data)?;
    let mut rows = Vec::with_capacity(tags.len());
    for t in tags {
        let id = format!(
            "{:0>8}",
            t.tag
                .chars()
                .filter(|c| c.is_ascii_hexdigit())
                .collect::<String>()
        );
        let Some((g, e)) = parse_tag_id(&id, &t.tag) else {
            continue;
        };
        rows.push(Row {
            group: g,
            element: e,
            element_is_block_offset: false,
            private_creator: String::new(),
            vr: t.vr,
            vm: t.vm,
            keyword: t.keyword,
            name: t.name,
            description: String::new(),
            retired: t.retired == "Y",
            vendors: vec!["standard".to_owned()],
            sources: vec!["PS3.6".to_owned()],
        });
    }
    Ok(rows)
}

fn load_resolved(path: &PathBuf) -> Result<Vec<Row>> {
    let file = File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut rows = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let t: ResolvedTag = serde_json::from_str(&line)?;
        rows.push(Row {
            group: t.group,
            element: t.element,
            element_is_block_offset: t.element_is_block_offset,
            private_creator: t.private_creator.unwrap_or_default(),
            vr: t.vr,
            vm: t.vm,
            keyword: t.keyword,
            name: t.name,
            description: t.description,
            retired: t.retired,
            vendors: t.vendors,
            sources: t.sources,
        });
    }
    Ok(rows)
}

fn load_csv(path: &PathBuf) -> Result<Vec<Row>> {
    let mut rdr =
        csv::Reader::from_path(path).with_context(|| format!("reading {}", path.display()))?;
    let mut rows = Vec::new();
    for result in rdr.records() {
        let rec = result?;
        // columns: group,element,private_creator,vr,vm,keyword,name,description,retired,vendors,sources
        let group = u16::from_str_radix(rec.get(0).unwrap_or(""), 16).unwrap_or(0);
        let elem_str = rec.get(1).unwrap_or("");
        let (element, element_is_block_offset) = parse_element_str(elem_str);
        let private_creator = rec.get(2).unwrap_or("").to_owned();
        let vr = rec.get(3).unwrap_or("").to_owned();
        let vm = rec.get(4).unwrap_or("1").to_owned();
        let keyword = rec.get(5).unwrap_or("").to_owned();
        let name = rec.get(6).unwrap_or("").to_owned();
        let description = rec.get(7).unwrap_or("").to_owned();
        let retired = rec.get(8).unwrap_or("N") == "Y";
        let vendors = split_pipe(rec.get(9).unwrap_or(""));
        let sources = split_pipe(rec.get(10).unwrap_or(""));
        rows.push(Row {
            group,
            element,
            element_is_block_offset,
            private_creator,
            vr,
            vm,
            keyword,
            name,
            description,
            retired,
            vendors,
            sources,
        });
    }
    Ok(rows)
}

fn parse_element_str(s: &str) -> (u16, bool) {
    if let Some(hex) = s.strip_prefix("xx").or_else(|| s.strip_prefix("XX")) {
        let e = u16::from_str_radix(hex, 16).unwrap_or(0);
        (e, true)
    } else {
        let e = u16::from_str_radix(s, 16).unwrap_or(0);
        (e, false)
    }
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
            &r.vm,
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

fn parse_tag_id(id: &str, tag: &str) -> Option<(u16, u16)> {
    if id.len() == 8 {
        let g = u16::from_str_radix(&id[0..4], 16).ok()?;
        let e = u16::from_str_radix(&id[4..8], 16).ok()?;
        return Some((g, e));
    }
    let clean: String = tag.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.len() == 8 {
        let g = u16::from_str_radix(&clean[0..4], 16).ok()?;
        let e = u16::from_str_radix(&clean[4..8], 16).ok()?;
        return Some((g, e));
    }
    None
}

fn parse_vm(vm: &str) -> (u8, u8) {
    let vm = vm.trim();
    if let Some((a, b)) = vm.split_once('-') {
        let lo = a.parse::<u8>().unwrap_or(1);
        let hi = if b == "n" {
            0xFF
        } else {
            b.parse::<u8>().unwrap_or(lo)
        };
        return (lo, hi);
    }
    let n = vm.parse::<u8>().unwrap_or(1);
    (n, n)
}

struct StringPool {
    bytes: Vec<u8>,
}

impl StringPool {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(1 << 20),
        }
    }
    fn intern(&mut self, s: &str) -> (u32, u32) {
        if s.is_empty() {
            return (0, 0);
        }
        let off = self.bytes.len() as u32;
        self.bytes.extend_from_slice(s.as_bytes());
        (off, s.len() as u32)
    }
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
        let vr = VrCode::from_str2(&r.vr);
        let (vm_min, vm_max) = parse_vm(&r.vm);
        let (k_off, k_len) = pool.intern(&r.keyword);
        let (n_off, n_len) = pool.intern(&r.name);
        let (c_off, c_len) = match canon.as_deref() {
            Some(c) => pool.intern(c),
            None => (0, 0),
        };
        let (d_off, d_len) = pool.intern(&r.description);
        let (s_off, s_len) = pool.intern(&r.sources.join("|"));
        records.push(TagRecord {
            group: r.group,
            element: r.element,
            element_is_block_offset: r.element_is_block_offset,
            retired: r.retired,
            vr,
            vm_min,
            vm_max,
            keyword_off: k_off,
            keyword_len: k_len as u16,
            name_off: n_off,
            name_len: n_len as u16,
            creator_off: c_off,
            creator_len: c_len as u16,
            description_off: d_off,
            description_len: d_len,
            sources_off: s_off,
            sources_len: s_len,
        });
        entries.push(IndexEntry {
            group: r.group,
            element: r.element,
            creator_hash: ch,
            record_idx: (records.len() - 1) as u32,
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

    let mut ser = AllocSerializer::<{ 1 << 20 }>::default();
    ser.serialize_value(&dict)
        .map_err(|e| anyhow::anyhow!("rkyv serialize: {e}"))?;
    let body = ser.into_serializer().into_inner();
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
// Two issues in the input data require pre-processing before compilation:
//
// 1. Some private tags in resolved_pydicom_backfilled.jsonl have
//    element_is_block_offset=false and a concrete element address (e.g. 0x1011
//    for block 0x10, offset 0x11). The lookup API always uses the block offset
//    (low byte, 0x11), so these tags would be unfindable in the compiled index.
//    Fix: normalise all private-tag elements to their block offset (low byte).
//
// 2. Some creator strings that differ only in case (e.g. "SPI RELEASE 1" vs
//    "SPI Release 1") canonicalise to the same string and therefore produce the
//    same creator_hash. When both appear in the input for the same tag, the
//    compiled index contains two entries with identical (group, element, hash)
//    keys, and binary_search returns whichever one happens to land first.
//    Fix: after normalisation, deduplicate by the index key, keeping the entry
//    with the most informative data (PDF source > pydicom-only; named > Unknown).

fn normalize_and_dedup(rows: Vec<Row>) -> Vec<Row> {
    let mut map: BTreeMap<(u16, u16, u32), Row> = BTreeMap::new();
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
                let curr_pdf = r.sources.iter().any(|s| s != "pydicom");
                let prev_pdf = prev.sources.iter().any(|s| s != "pydicom");
                if curr_pdf && !prev_pdf {
                    true // PDF source beats pydicom-only
                } else if !curr_pdf && prev_pdf {
                    false // keep existing PDF entry
                } else {
                    // Same source type: prefer a real name over "Unknown".
                    r.name != "Unknown" && prev.name == "Unknown"
                }
            }
        };
        if replace {
            map.insert(key, r);
        }
    }
    // BTreeMap iterates in key order, so the result is already sorted by
    // (group, element, creator_hash) - matching the index sort in compile().
    map.into_values().collect()
}

// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let args = Args::parse();

    let rows: Vec<Row> = if let Some(p) = args.csv.as_ref() {
        load_csv(p)?
    } else {
        let mut rows = Vec::new();
        if let Some(p) = args.standard.as_ref() {
            rows.extend(load_standard(p)?);
        }
        if let Some(p) = args.resolved.as_ref() {
            rows.extend(load_resolved(p)?);
        }
        rows
    };

    eprintln!("loaded {} rows", rows.len());

    let rows = normalize_and_dedup(rows);
    eprintln!("after dedup: {} rows", rows.len());

    if let Some(p) = args.export_csv.as_ref() {
        write_csv(p, &rows)?;
        eprintln!("exported CSV to {}", p.display());
    }

    if let Some(p) = args.out.as_ref() {
        compile(&rows, p)?;
    }

    Ok(())
}
