"""Exception hierarchy and Dict surface of dicom_map."""
from __future__ import annotations

import struct

import pytest

import dicom_map

DMAP_PATH = "tags.dmap"


def test_hierarchy():
    assert issubclass(dicom_map.DmapError, ValueError)
    assert issubclass(dicom_map.DmapIOError, dicom_map.DmapError)
    assert issubclass(dicom_map.DmapIOError, OSError)
    assert issubclass(dicom_map.DmapFormatError, dicom_map.DmapError)
    assert issubclass(dicom_map.UnsupportedVersionError, dicom_map.DmapFormatError)
    assert issubclass(dicom_map.BadArchiveError, dicom_map.DmapFormatError)


def test_missing_file_is_dmap_io_error_and_oserror(tmp_path):
    with pytest.raises(dicom_map.DmapIOError) as ei:
        dicom_map.open(tmp_path / "nope.dmap")
    assert isinstance(ei.value, OSError)
    assert ei.value.errno is not None
    assert "nope.dmap" in str(ei.value.filename)


def test_unsupported_version_carries_numbers(tmp_path):
    data = bytearray(open(DMAP_PATH, "rb").read())
    data[4:6] = struct.pack("<H", 9)
    p = tmp_path / "v9.dmap"
    p.write_bytes(data)
    with pytest.raises(dicom_map.UnsupportedVersionError) as ei:
        dicom_map.open(p)
    assert (ei.value.got_major, ei.value.supported_major) == (9, 4)


def test_later_minor_opens(tmp_path):
    data = bytearray(open(DMAP_PATH, "rb").read())
    data[6:8] = struct.pack("<H", 3)
    p = tmp_path / "minor.dmap"
    p.write_bytes(data)
    assert len(dicom_map.open(p)) > 19_000


def test_truncated_file_is_bad_archive(tmp_path):
    data = open(DMAP_PATH, "rb").read()
    p = tmp_path / "trunc.dmap"
    p.write_bytes(data[:-5])
    with pytest.raises(dicom_map.BadArchiveError) as ei:
        dicom_map.open(p)
    assert ei.value.check == "body length"


def test_bad_magic_is_format_error(tmp_path):
    p = tmp_path / "magic.dmap"
    p.write_bytes(b"X" * 64)
    with pytest.raises(dicom_map.DmapFormatError):
        dicom_map.open(p)


def test_iteration_and_keys():
    d = dicom_map.open(DMAP_PATH)
    first = next(iter(d))
    assert set(first) >= {"group", "element", "creator", "vr", "vm", "flags", "sources"}
    assert sum(1 for _ in d) == len(d)
    t = d.lookup(0x7FE0, 0x0010)
    assert t["vr"] == "OB or OW"
    assert d.lookup(0x0021, 0x1008, "Siemens: Thorax/Multix FD Lab Settings") is not None
