"""Memory-mapped O(log n) DICOM tag dictionary.

The Rust extension lives at :mod:`dicom_map._dicom_map`; everything from it
is re-exported at package level for backwards-compatible imports::

    import dicom_map
    d = dicom_map.open("tags.dmap")
    d.lookup(0x0008, 0x0005)

The :func:`patch_pydicom` adapter lets pydicom resolve our private tag
entries automatically::

    import dicom_map, pydicom
    dicom_map.patch_pydicom("tags.dmap")
    ds = pydicom.dcmread("scan.dcm")    # private tags now have proper names/VRs
"""
from __future__ import annotations

from ._dicom_map import Dict, __version__, open  # noqa: F401
from .pydicom_adapter import patch_pydicom, unpatch_pydicom  # noqa: F401

__all__ = ["Dict", "open", "patch_pydicom", "unpatch_pydicom", "__version__"]
