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
