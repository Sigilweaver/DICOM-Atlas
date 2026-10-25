//! End-to-end checks that `dmap-compile` rejects input it cannot parse with
//! confidence, and reports what it leaves out.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dicom_map::DmapDict;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("dmap-strict-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
    fn write(&self, name: &str, body: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn compile(args: &[&Path]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dmap-compile"));
    for pair in args.chunks(2) {
        cmd.arg(pair[0]).arg(pair[1]);
    }
    cmd.output().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn std_json(rows: &[(&str, &str, &str, &str)]) -> String {
    // (tag, name, vr, vm)
    let items: Vec<String> = rows
        .iter()
        .map(|(tag, name, vr, vm)| {
            format!(
                r#"{{"tag":"{tag}","name":"{name}","keyword":"K","valueRepresentation":"{vr}","valueMultiplicity":"{vm}","retired":"N"}}"#
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn res_line(
    group: u16,
    element: u16,
    creator: Option<&str>,
    name: &str,
    vr: &str,
    vm: &str,
) -> String {
    let creator = creator
        .map(|c| format!("\"{c}\""))
        .unwrap_or_else(|| "null".into());
    format!(
        r#"{{"group":{group},"element":{element},"element_is_block_offset":true,"private_creator":{creator},"keyword":"K","name":"{name}","vr":"{vr}","vm":"{vm}","description":"","retired":false,"vendors":["x"],"sources":["a.pdf#p1"]}}"#
    )
}

fn run_std_res(s: &Scratch, std: &str, res: &str) -> Output {
    let std = s.write("std.json", std);
    let res = s.write("res.jsonl", res);
    let out = s.path("out.dmap");
    compile(&[
        Path::new("--standard"),
        &std,
        Path::new("--resolved"),
        &res,
        Path::new("--out"),
        &out,
    ])
}

#[test]
fn repeating_group_patterns_are_skipped_counted_and_never_misaddressed() {
    let s = Scratch::new("repeat");
    let o = run_std_res(
        &s,
        &std_json(&[
            ("(0008,0005)", "Specific Character Set", "CS", "1-n"),
            ("(60XX,0010)", "Overlay Rows", "US", "1"),
            ("(0028,04X0)", "Rows For Nth Order Coefficients", "US", "1"),
            ("(1000,XXX0)", "Escape Triplet", "US", "3"),
            ("(50XX,0010)", "Number of Points", "US", "1"),
        ]),
        "",
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let err = stderr(&o);
    assert!(err.contains("skipped 4 repeating-group"), "{err}");
    assert!(err.contains("(60XX,0010)"), "{err}");
    let d = DmapDict::open(s.path("out.dmap")).unwrap();
    assert_eq!(d.len(), 1);
    // The addresses the old parser produced for these patterns.
    for (g, e) in [
        (0x0060, 0x0010),
        (0x0002, 0x8040),
        (0x0001, 0x0000),
        (0x0050, 0x0010),
    ] {
        assert!(d.lookup(g, e, None).is_none(), "({g:04X},{e:04X})");
    }
    assert!(d.lookup(0x0008, 0x0005, None).is_some());
}

#[test]
fn standard_tag_in_unknown_shape_is_an_error() {
    for bad in ["00080005", "(0008,005)", "(0008-0005)", "(00G8,0005)"] {
        let s = Scratch::new("badtag");
        let o = run_std_res(&s, &std_json(&[(bad, "N", "CS", "1")]), "");
        assert!(!o.status.success(), "{bad}");
        let err = stderr(&o);
        assert!(err.contains("row 1") && err.contains(bad), "{err}");
    }
}

#[test]
fn private_row_without_creator_is_an_error() {
    let s = Scratch::new("nocreator");
    let o = run_std_res(
        &s,
        &std_json(&[]),
        &res_line(0x0029, 0x10, None, "Whatever", "LO", "1"),
    );
    assert!(!o.status.success());
    let err = stderr(&o);
    assert!(
        err.contains("(0029,0010)") && err.contains("no private creator"),
        "{err}"
    );

    // Whitespace-only creator is no creator either.
    let o = run_std_res(
        &s,
        &std_json(&[]),
        &res_line(0x0029, 0x10, Some("   "), "Whatever", "LO", "1"),
    );
    assert!(!o.status.success());
}

#[test]
fn private_row_with_creator_compiles() {
    let s = Scratch::new("creator");
    let o = run_std_res(
        &s,
        &std_json(&[]),
        &res_line(0x0029, 0x08, Some("ACME"), "Thing", "US or SS", "2-2n"),
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let d = DmapDict::open(s.path("out.dmap")).unwrap();
    let v = d.lookup(0x0029, 0x08, Some("acme")).unwrap();
    assert_eq!(v.vr(), "US or SS");
    assert!(d.lookup(0x0029, 0x08, None).is_none());
}

#[test]
fn resolved_missing_or_bad_fields_are_errors() {
    let good = res_line(0x0029, 0x08, Some("ACME"), "Thing", "US", "1");
    let s = Scratch::new("resfields");
    // Missing vr / vm keys.
    for key in ["vr", "vm"] {
        let mut v: serde_json::Value = serde_json::from_str(&good).unwrap();
        v.as_object_mut().unwrap().remove(key);
        let o = run_std_res(&s, &std_json(&[]), &v.to_string());
        assert!(!o.status.success(), "missing {key}");
        assert!(stderr(&o).contains(key), "{}", stderr(&o));
    }
    // Bad vr / vm values name the column and the row.
    for (vr, vm, col) in [
        ("ob", "1", "vr"),
        (" US", "1", "vr"),
        ("", "1", "vr"),
        ("US", "1-", "vm"),
        ("US", "", "vm"),
        ("US", "3-2", "vm"),
    ] {
        let line = res_line(0x0029, 0x08, Some("ACME"), "Thing", vr, vm);
        let o = run_std_res(&s, &std_json(&[]), &format!("\n{line}\n"));
        assert!(!o.status.success(), "{vr:?} {vm:?}");
        let err = stderr(&o);
        assert!(
            err.contains(&format!("column `{col}`")) && err.contains("row 2"),
            "{err}"
        );
    }
}

#[test]
fn csv_hand_edits_are_errors_not_zero() {
    let header =
        "group,element,private_creator,vr,vm,keyword,name,description,retired,vendors,sources\n";
    let ok = "0029,xx08,ACME,US,1,K,Thing,,N,v,s\n";
    let s = Scratch::new("csv");
    let out = s.path("o.dmap");
    let run = |row: &str| {
        let csv = s.write("t.csv", &format!("{header}{ok}{row}\n"));
        compile(&[Path::new("--csv"), &csv, Path::new("--out"), &out])
    };
    assert!(run("0029,xx09,ACME,US,1,K,Thing,,N,v,s").status.success());
    for (row, col) in [
        ("002G,xx09,ACME,US,1,K,T,,N,v,s", "group"),
        ("29,xx09,ACME,US,1,K,T,,N,v,s", "group"),
        ("0029,x10,ACME,US,1,K,T,,N,v,s", "element"),
        ("0029,10xx,ACME,US,1,K,T,,N,v,s", "element"),
        ("0029,xx1,ACME,US,1,K,T,,N,v,s", "element"),
        ("0029,xx09,ACME,US,1,K,T,,y,v,s", "retired"),
        ("0029,xx09,ACME,US,1,K,T,,R,v,s", "retired"),
        ("0029,xx09,ACME,US,1,K,T,,,v,s", "retired"),
        ("0029,xx09,ACME,,1,K,T,,N,v,s", "vr"),
        ("0029,xx09,ACME,US,,K,T,,N,v,s", "vm"),
    ] {
        let o = run(row);
        assert!(!o.status.success(), "{row}");
        let err = stderr(&o);
        assert!(
            err.contains(&format!("column `{col}`")) && err.contains("row 2"),
            "{row}: {err}"
        );
    }
    // Wrong column count.
    let o = run("0029,xx09,ACME,US,1");
    assert!(!o.status.success());
}

#[test]
fn duplicate_keys_that_disagree_are_errors_and_agreeing_ones_are_counted() {
    let s = Scratch::new("dups");
    // VR conflict, creators differ only in case.
    let res = [
        res_line(0x0011, 0x20, Some("SPI Release 1"), "Pregnancy", "CS", "1"),
        res_line(0x0011, 0x20, Some("SPI RELEASE 1"), "Pregnancy", "LO", "1"),
    ]
    .join("\n");
    let o = run_std_res(&s, &std_json(&[]), &res);
    assert!(!o.status.success());
    let err = stderr(&o);
    assert!(
        err.contains("VR differs") && err.contains("Pregnancy"),
        "{err}"
    );

    // Name conflict across blocks collapsed to the same offset.
    let res = [
        res_line(0x0011, 0x1020, Some("SPI"), "Alpha", "CS", "1"),
        res_line(0x0011, 0x2020, Some("SPI"), "Beta", "CS", "1"),
    ]
    .join("\n")
    // Concrete element addresses, as in the pydicom-sourced rows.
    .replace(
        "\"element_is_block_offset\":true",
        "\"element_is_block_offset\":false",
    );
    let o = run_std_res(&s, &std_json(&[]), &res);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("name differs"), "{}", stderr(&o));

    // Case-only name difference and a placeholder name are not conflicts.
    let res = [
        res_line(0x0011, 0x20, Some("SPI Release 1"), "Pregnancy", "CS", "1"),
        res_line(0x0011, 0x20, Some("SPI RELEASE 1"), "PREGNANCY", "CS", "1"),
        res_line(0x0011, 0x21, Some("SPI"), "Unknown", "CS", "1"),
        res_line(0x0011, 0x21, Some("SPI"), "Named", "CS", "1"),
    ]
    .join("\n");
    let o = run_std_res(&s, &std_json(&[]), &res);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("2 duplicates with identical VR and name merged"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn overlong_or_nul_text_is_an_error() {
    let s = Scratch::new("long");
    let long = "x".repeat(70_000);
    let o = run_std_res(
        &s,
        &std_json(&[]),
        &res_line(0x0029, 0x08, Some("ACME"), &long, "US", "1"),
    );
    assert!(!o.status.success());
    assert!(stderr(&o).contains("column `name`"), "{}", stderr(&o));

    let o = run_std_res(
        &s,
        &std_json(&[]),
        &res_line(0x0029, 0x08, Some("ACME"), "a\\u0000b", "US", "1"),
    );
    assert!(!o.status.success());
    assert!(stderr(&o).contains("NUL"), "{}", stderr(&o));
}

#[test]
fn vr_and_vm_forms_from_the_standard_are_accepted_and_typos_are_not() {
    let s = Scratch::new("stdforms");
    let o = run_std_res(
        &s,
        &std_json(&[
            ("(0028,0106)", "Smallest Image Pixel Value", "US or SS", "1"),
            ("(7FE0,0010)", "Pixel Data", "OB or OW", "1"),
            ("(0028,3006)", "LUT Data", "US or OW", "1-n or 1"),
            ("(FFFE,E000)", "Item", "See Note 2", "1"),
            ("(0008,0202)", "Retired", "", ""),
        ]),
        "",
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let d = DmapDict::open(s.path("out.dmap")).unwrap();
    assert_eq!(d.lookup(0x7FE0, 0x0010, None).unwrap().vr(), "OB or OW");
    assert_eq!(d.lookup(0x0028, 0x0106, None).unwrap().vr(), "US or SS");
    assert_eq!(d.lookup(0xFFFE, 0xE000, None).unwrap().vr(), "??");

    for vr in ["ob", " US", "US ", "OB/OW"] {
        let o = run_std_res(&s, &std_json(&[("(0008,0005)", "N", vr, "1")]), "");
        assert!(!o.status.success(), "{vr:?}");
    }
    let o = run_std_res(&s, &std_json(&[("(0008,0005)", "N", "CS", "1-")]), "");
    assert!(!o.status.success());
}
