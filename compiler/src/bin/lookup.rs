//! `dicom-lookup` - query a `.dmap` file from the command line.
//!
//! Examples:
//!
//! ```text
//! dicom-lookup 0008 0005
//! dicom-lookup 0021 xx01 GEMS_XR3DCAL_01
//! dicom-lookup --file tags.dmap --json 0021 xx01 "Siemens: Thorax/Multix FD Lab Settings"
//! ```
//!
//! When built with `--features embedded`, the binary contains a baked-in copy
//! of `tags.dmap` and does not require an external file unless `--file` is
//! given explicitly.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use dicom_map::DmapDict;

#[derive(Parser, Debug)]
#[command(
    name = "dicom-lookup",
    about = "Look up a DICOM tag in a .dmap file.",
    version
)]
struct Args {
    /// Path to the .dmap file. Defaults to $DMAP_FILE then ./tags.dmap.
    /// When built with the `embedded` feature, the file is optional; the
    /// binary contains a baked-in dictionary that is used when no path is given.
    #[arg(short, long)]
    file: Option<PathBuf>,

    /// Emit machine-readable JSON instead of the human-readable table.
    #[arg(long)]
    json: bool,

    /// 4-hex-digit group, e.g. `0021`.
    group: String,

    /// 4-hex-digit element, e.g. `0008` or `xx08` (`xx` is stripped; only the low byte is used).
    element: String,

    /// Private creator string. Omit for public tags.
    creator: Option<String>,
}

fn parse_hex4(label: &str, s: &str) -> Result<u16> {
    let clean: String = s
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(4)
        .collect();
    if clean.is_empty() {
        return Err(anyhow!("{label} `{s}` contains no hex digits"));
    }
    u16::from_str_radix(&clean, 16).with_context(|| format!("parsing {label} `{s}`"))
}

fn resolve_dmap_path(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    if let Ok(e) = std::env::var("DMAP_FILE") {
        if !e.is_empty() {
            return PathBuf::from(e);
        }
    }
    PathBuf::from("tags.dmap")
}

fn render_human(v: dicom_map::TagView<'_>, group: u16, element: u16, creator: Option<&str>) {
    println!("tag        : ({group:04X},{element:04X})");
    if v.is_block_offset() {
        println!(
            "            private element block offset (GGGG,xx{:02X})",
            element & 0xFF
        );
    }
    if let Some(c) = creator {
        println!("creator    : {c}");
    } else if !v.creator().is_empty() {
        println!("creator    : {}", v.creator());
    }
    if !v.keyword().is_empty() {
        println!("keyword    : {}", v.keyword());
    }
    println!("name       : {}", v.name());
    println!("VR         : {}", v.vr());
    if !v.description().is_empty() {
        println!("description: {}", v.description());
    }
    let srcs = v.sources_raw();
    if !srcs.is_empty() {
        println!("sources    : {}", srcs.replace('|', "\n             "));
    }
    if v.retired() {
        println!("retired    : yes");
    }
}

fn render_json(v: dicom_map::TagView<'_>, group: u16, element: u16, creator: Option<&str>) {
    // Hand-rolled JSON to avoid a serde dep here.
    let esc = |s: &str| {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    };
    let creator_out = creator.unwrap_or(v.creator());
    println!(
        "{{\"group\":\"{:04X}\",\"element\":\"{:04X}\",\"creator\":{},\"keyword\":{},\"name\":{},\"vr\":{},\"description\":{},\"retired\":{},\"block_offset\":{},\"sources\":[{}]}}",
        group,
        element,
        esc(creator_out),
        esc(v.keyword()),
        esc(v.name()),
        esc(v.vr()),
        esc(v.description()),
        v.retired(),
        v.is_block_offset(),
        v.sources().map(|s| esc(s)).collect::<Vec<_>>().join(","),
    );
}

fn load_dict(explicit: Option<PathBuf>) -> DmapDict {
    // When built with the `embedded` feature and no explicit path is given,
    // use the baked-in dictionary without touching the filesystem.
    #[cfg(feature = "embedded")]
    if explicit.is_none() {
        if std::env::var_os("DMAP_FILE").filter(|v| !v.is_empty()).is_none() {
            return dicom_map::embedded::embedded();
        }
    }

    let path = resolve_dmap_path(explicit);
    match DmapDict::open(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot open {}: {e}", path.display());
            std::process::exit(1);
        }
    }
}

fn main() -> ExitCode {
    let args = Args::parse();

    let group = match parse_hex4("group", &args.group) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let element = match parse_hex4("element", &args.element) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let dict = load_dict(args.file);

    let creator_ref = args.creator.as_deref();
    match dict.lookup(group, element, creator_ref) {
        Some(v) => {
            if args.json {
                render_json(v, group, element, creator_ref);
            } else {
                render_human(v, group, element, creator_ref);
            }
            ExitCode::SUCCESS
        }
        None => {
            if args.json {
                println!("null");
            } else {
                eprintln!(
                    "not found: ({group:04X},{element:04X}){}",
                    creator_ref
                        .map(|c| format!(" under creator \"{c}\""))
                        .unwrap_or_default()
                );
            }
            ExitCode::from(3)
        }
    }
}
