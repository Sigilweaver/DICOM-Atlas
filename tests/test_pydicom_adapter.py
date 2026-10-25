"""Tests for the pydicom adapter (dicom_map.patch_pydicom)."""
from __future__ import annotations

import pytest

pydicom = pytest.importorskip("pydicom")
import dicom_map  # noqa: E402


DMAP_PATH = "tags.dmap"
CSV_PATH = "tags.csv"


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

    added = dicom_map.patch_pydicom(DMAP_PATH, csv_path=CSV_PATH)
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

    dicom_map.patch_pydicom(DMAP_PATH, csv_path=CSV_PATH, mode="fill")
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

    dicom_map.patch_pydicom(DMAP_PATH, csv_path=CSV_PATH)
    try:
        assert creator in pdd
        assert len(pdd[creator]) > 0
    finally:
        dicom_map.unpatch_pydicom()


def test_stale_csv_is_rejected_and_pydicom_left_untouched(tmp_path):
    import csv

    pdd = _pdd()
    before = sum(len(v) for v in pdd.values())
    rows = list(csv.reader(open(CSV_PATH, newline="", encoding="utf-8")))
    header = rows[0]
    vr_col = header.index("vr")
    # Change the VR of the first private row.
    for r in rows[1:]:
        if r[header.index("private_creator")]:
            r[vr_col] = "UN" if r[vr_col] != "UN" else "OB"
            break
    bad = tmp_path / "tags.csv"
    with open(bad, "w", newline="", encoding="utf-8") as fh:
        csv.writer(fh).writerows(rows)
    with pytest.raises(ValueError, match="does not match"):
        dicom_map.patch_pydicom(DMAP_PATH, csv_path=bad)
    assert sum(len(v) for v in pdd.values()) == before


def test_empty_vr_or_vm_is_an_error_not_a_default(tmp_path):
    import csv

    rows = list(csv.reader(open(CSV_PATH, newline="", encoding="utf-8")))
    header = rows[0]
    for col in ("vr", "vm"):
        mod = [list(r) for r in rows]
        for r in mod[1:]:
            if r[header.index("private_creator")]:
                r[header.index(col)] = ""
                break
        bad = tmp_path / f"{col}.csv"
        with open(bad, "w", newline="", encoding="utf-8") as fh:
            csv.writer(fh).writerows(mod)
        with pytest.raises(ValueError, match=f"empty {col}"):
            dicom_map.patch_pydicom(csv_path=bad)
