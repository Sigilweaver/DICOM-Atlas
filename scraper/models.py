"""Canonical data models for the scraper pipeline.

Three stages of tag representation:

    RawTag         -- direct output of a harvester; messy strings allowed
    NormalizedTag  -- cleaned, VR validated, private creator canonicalized
    ResolvedTag    -- deduplicated across vendors; this is what the
                      Rust compiler consumes to build the .dmap file

Public/standard DICOM tags are represented by StandardTag (mirrors the
innolitics PS3.6 JSON schema, see scraper/bootstrap.py).
"""

from __future__ import annotations

import re
from typing import Literal

from pydantic import BaseModel, Field, field_validator

# ----------------------------------------------------------------------------
# DICOM value-representation enumeration (PS3.5 § 6.2)
# ----------------------------------------------------------------------------

VR_CODES: frozenset[str] = frozenset(
    {
        "AE", "AS", "AT", "CS", "DA", "DS", "DT", "FL", "FD", "IS",
        "LO", "LT", "OB", "OD", "OF", "OL", "OV", "OW", "PN", "SH",
        "SL", "SQ", "SS", "ST", "SV", "TM", "UC", "UI", "UL", "UN",
        "UR", "US", "UT", "UV",
    }
)

TAG_RE = re.compile(r"^\(?([0-9A-Fa-f]{4}),\s*([0-9A-Fa-f]{4})\)?$")


def parse_tag(raw: str) -> tuple[int, int]:
    """Parse '(0019,100C)' or '0019,100C' or '0019100C' → (0x0019, 0x100C)."""
    s = raw.strip()
    # allow bare 8-hex form
    if len(s) == 8 and all(c in "0123456789abcdefABCDEF" for c in s):
        return int(s[:4], 16), int(s[4:], 16)
    m = TAG_RE.match(s)
    if not m:
        raise ValueError(f"unparseable tag: {raw!r}")
    return int(m.group(1), 16), int(m.group(2), 16)


def is_private_group(group: int) -> bool:
    """Private DICOM groups are odd (PS3.5 § 7.8)."""
    return (group & 1) == 1


# ----------------------------------------------------------------------------
# Stage 1: raw harvester output
# ----------------------------------------------------------------------------


class RawTag(BaseModel):
    """Whatever the harvester pulled out of a PDF row — minimal cleaning."""

    source_pdf: str            # filename / relative path
    source_page: int           # 1-based page number
    source_table: int | None = None
    tag_str: str               # e.g. "(0019,xx0C)" — may contain placeholders
    name: str | None = None
    vr: str | None = None
    vm: str | None = None
    description: str | None = None
    private_creator: str | None = None


# ----------------------------------------------------------------------------
# Stage 2: normalized
# ----------------------------------------------------------------------------


class NormalizedTag(BaseModel):
    """Validated tag ready for cross-vendor resolution."""

    group: int = Field(ge=0, le=0xFFFF)
    element: int = Field(ge=0, le=0xFFFF)
    # For private tags the element is usually written as (gggg,xxEE) where
    # xx is the block offset determined at runtime from the private creator.
    # We store the low byte and a flag.
    element_is_block_offset: bool = False
    private_creator: str | None = None
    keyword: str | None = None
    name: str
    vr: str
    vr_inferred: bool = False  # True when VR was defaulted to 'UN' by the harvester
    vm: str = "1"
    description: str = ""
    retired: bool = False
    vendor: str                # "siemens" | "ge" | "philips" | ...
    source_pdf: str
    source_page: int

    @field_validator("vr")
    @classmethod
    def _vr_ok(cls, v: str) -> str:
        v = v.strip().upper()
        if v not in VR_CODES:
            raise ValueError(f"unknown VR {v!r}")
        return v

    @field_validator("private_creator")
    @classmethod
    def _canon_creator(cls, v: str | None) -> str | None:
        if v is None:
            return None
        # canonical form: uppercase, single-space
        return " ".join(v.split()).upper()


# ----------------------------------------------------------------------------
# Stage 3: resolved (cross-vendor, deduplicated)
# ----------------------------------------------------------------------------


class ResolvedTag(BaseModel):
    """One tag, possibly seen in multiple conformance docs."""

    group: int
    element: int
    element_is_block_offset: bool
    private_creator: str | None
    keyword: str
    name: str
    vr: str
    vm: str
    description: str
    retired: bool = False
    vendors: list[str] = Field(default_factory=list)
    sources: list[str] = Field(default_factory=list)  # "pdf#page" strings


# ----------------------------------------------------------------------------
# Public / standard tags (bootstrap baseline)
# ----------------------------------------------------------------------------


class StandardTag(BaseModel):
    """Mirrors innolitics PS3.6 attributes.json entry."""

    tag: str                   # "(0008,0005)"
    name: str
    keyword: str
    valueRepresentation: str   # may be empty or "See Note" for some sequences
    valueMultiplicity: str
    retired: Literal["Y", "N"]
    id: str                    # "00080005"

    @property
    def group(self) -> int:
        return int(self.id[:4], 16)

    @property
    def element(self) -> int:
        return int(self.id[4:], 16)
