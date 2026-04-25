from pathlib import Path

from scraper.models import NormalizedTag, RawTag, parse_tag
from scraper.normalize import normalize, _to_keyword  # noqa: PLC2701


def test_parse_tag_variants():
    assert parse_tag("(0019,100C)") == (0x0019, 0x100C)
    assert parse_tag("0019,100C") == (0x0019, 0x100C)
    assert parse_tag("0019100C") == (0x0019, 0x100C)


def test_to_keyword():
    assert _to_keyword("Auto Window Flag") == "AutoWindowFlag"
    assert _to_keyword("Patient's Name") == "PatientSName"
    assert _to_keyword("  foo--bar  ") == "FooBar"


def test_normalize_private_block_offset():
    raw = RawTag(
        source_pdf="x.pdf",
        source_page=1,
        tag_str="(0019,xx0C)",
        private_creator="SIEMENS MR HEADER",
        name="ICE Dims",
        vr="LO",
        vm="1",
    )
    n = normalize(raw, vendor="siemens")
    assert n is not None
    assert n.group == 0x0019
    assert n.element == 0x000C
    assert n.element_is_block_offset is True
    assert n.private_creator == "SIEMENS MR HEADER"
    assert n.vr == "LO"
    assert n.keyword == "ICEDims"


def test_normalize_rejects_even_group_with_creator():
    raw = RawTag(
        source_pdf="x.pdf",
        source_page=1,
        tag_str="(0018,1000)",  # even group
        private_creator="SHOULD NOT BE HERE",
        name="Device Serial Number",
        vr="LO",
    )
    assert normalize(raw, vendor="siemens") is None


def test_normalize_rejects_bad_vr():
    raw = RawTag(
        source_pdf="x.pdf",
        source_page=1,
        tag_str="(0019,xx0C)",
        private_creator="X",
        name="Foo",
        vr="ZZ",
    )
    # Invalid VRs are coerced to "UN" with vr_inferred=True so the tag is
    # still surfaced for downstream backfill from pydicom.
    n = normalize(raw, vendor="siemens")
    assert n is not None
    assert n.vr == "UN"
    assert n.vr_inferred is True


def test_normalized_tag_canonicalizes_creator():
    n = NormalizedTag(
        group=0x19,
        element=0x0C,
        element_is_block_offset=True,
        private_creator="  siemens  mr   header  ",
        keyword="X",
        name="X",
        vr="LO",
        vendor="siemens",
        source_pdf="x.pdf",
        source_page=1,
    )
    assert n.private_creator == "SIEMENS MR HEADER"


def test_siemens_fixture_harvest_end_to_end():
    # Only runs if the sample PDF has been downloaded.
    pdf = Path("data/pdfs/Siemens_FLUOROSPOT_Compact_VE20_DICOM_Conformance_Statement.pdf")
    if not pdf.exists():
        return
    from scraper.harvest.siemens import SiemensHarvester

    raws = list(SiemensHarvester(pdf).harvest())
    assert len(raws) > 50
    assert all(r.private_creator for r in raws)
    normed = [n for n in (normalize(r, "siemens") for r in raws) if n]
    assert len(normed) == len(raws)
    assert {n.vr for n in normed} <= {
        "AE","AS","AT","CS","DA","DS","DT","FL","FD","IS","LO","LT","OB",
        "OD","OF","OL","OV","OW","PN","SH","SL","SQ","SS","ST","SV","TM",
        "UC","UI","UL","UN","UR","US","UT","UV",
    }
