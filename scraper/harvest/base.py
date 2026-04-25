"""Abstract base for per-vendor harvesters.

A harvester walks a single PDF and emits RawTag rows. Normalization to
NormalizedTag happens in the caller (see scraper.pipeline.run_vendor).
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import Iterator
from pathlib import Path

from scraper.models import RawTag


def squeeze_row(row: list[str | None]) -> list[str]:
    """pdfplumber often returns rows with interspersed empty cells caused by
    column bleed. Collapse consecutive empty/whitespace cells so downstream
    code can address columns by logical index."""
    cleaned: list[str] = []
    for cell in row:
        v = (cell or "").strip().replace("\n", " ")
        if v:
            cleaned.append(v)
    return cleaned


class Harvester(ABC):
    vendor: str  # subclass sets; e.g. "siemens"

    def __init__(self, pdf_path: Path):
        self.pdf_path = pdf_path

    @abstractmethod
    def harvest(self) -> Iterator[RawTag]:
        ...
