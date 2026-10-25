"""Tests for the pydicom adapter (dicom_map.patch_pydicom)."""
from __future__ import annotations

import pytest

pydicom = pytest.importorskip("pydicom")
import dicom_map  # noqa: E402


DMAP_PATH = "tags.dmap"


def _pdd():
    from pydicom._private_dict import private_dictionaries
    return private_dictionaries


def test_open_dict():
    d = dicom_map.open(DMAP_PATH)
    assert len(d) > 19_000
    t = d.lookup(0x0008, 0x0005)
    assert t["keyword"] == "SpecificCharacterSet"


def test_patch_adds_entries_and_unpatch_restores():
    pdd = _pdd()
    before = sum(len(v) for v in pdd.values())

    added = dicom_map.patch_pydicom(DMAP_PATH)
    assert added > 0, "expected at least one entry to be added"
    after = sum(len(v) for v in pdd.values())
    assert after - before == added

    removed = dicom_map.unpatch_pydicom()
    assert removed == added
    restored = sum(len(v) for v in pdd.values())
    assert restored == before


def test_patch_fill_mode_preserves_existing_pydicom_entry():
    pdd = _pdd()
    # Pick a Siemens entry that exists in pydicom and mutate it pre-patch.
    creator = "SIEMENS CM VA0  ACQU"
    key = "0019xx10"
    assert creator in pdd and key in pdd[creator], "test fixture missing"
    before = pdd[creator][key]

    dicom_map.patch_pydicom(DMAP_PATH, mode="fill")
    try:
        assert pdd[creator][key] == before, (
            "fill mode must not overwrite an existing pydicom entry"
        )
    finally:
        dicom_map.unpatch_pydicom()


def test_patch_resolves_a_known_dicom_map_only_creator():
    pdd = _pdd()
    # GEHC_HYBRID_01 is one we have from PDFs that pydicom doesn't carry.
    creator = "GEHC_HYBRID_01"
    assert creator not in pdd, "test fixture: this creator should be pydicom-only-after-patch"

    dicom_map.patch_pydicom(DMAP_PATH)
    try:
        assert creator in pdd
        assert len(pdd[creator]) > 0
    finally:
        dicom_map.unpatch_pydicom()


def test_patch_has_no_csv_path():
    import inspect

    assert list(inspect.signature(dicom_map.patch_pydicom).parameters) == ["dmap_path", "mode"]


def test_patch_reads_the_dmap_it_was_given(tmp_path):
    """Entries come from the given dmap, not from a tags.csv next to it."""
    pdd = _pdd()
    # A tiny dmap with one private record, next to a stale tags.csv.
    import subprocess, sys, json, os

    compile_bin = os.environ.get("DMAP_COMPILE", "target/release/dmap-compile")
    if not os.path.exists(compile_bin):
        pytest.skip("dmap-compile not built")
    std = tmp_path / "std.json"
    std.write_text("[]")
    res = tmp_path / "res.jsonl"
    res.write_text(json.dumps({
        "group": 0x0029, "element": 0x08, "element_is_block_offset": True,
        "private_creator": "TEST ONLY CREATOR", "keyword": "K", "name": "Only Here",
        "vr": "US", "vm": "1-n", "description": "", "retired": False,
        "vendors": ["x"], "sources": ["a.pdf#p1"],
    }) + "\n")
    out = tmp_path / "custom.dmap"
    subprocess.run([compile_bin, "--standard", str(std), "--resolved", str(res), "--out", str(out)],
                   check=True, capture_output=True)
    (tmp_path / "tags.csv").write_text("not,a,valid,csv\n")
    try:
        assert dicom_map.patch_pydicom(out) == 1
        assert pdd["TEST ONLY CREATOR"]["0029xx08"] == ("US", "1-n", "Only Here", "K")
    finally:
        dicom_map.unpatch_pydicom()
    assert "TEST ONLY CREATOR" not in pdd


def test_patch_errors_leave_pydicom_untouched(tmp_path):
    pdd = _pdd()
    before = sum(len(v) for v in pdd.values())
    with pytest.raises(dicom_map.DmapIOError):
        dicom_map.patch_pydicom(tmp_path / "missing.dmap")
    bad = tmp_path / "bad.dmap"
    bad.write_bytes(b"not a dmap file at all, but long enough for a header....")
    with pytest.raises(dicom_map.DmapFormatError):
        dicom_map.patch_pydicom(bad)
    with pytest.raises(ValueError):
        dicom_map.patch_pydicom(DMAP_PATH, mode="bogus")
    assert sum(len(v) for v in pdd.values()) == before
