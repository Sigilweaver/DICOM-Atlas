"""pydicom adapter: register dicom-map's private tag dictionary into pydicom
so existing pydicom workflows resolve our entries automatically.

Usage
-----

    import dicom_map, pydicom
    dicom_map.patch_pydicom("tags.dmap")     # one-time, at import / startup

    ds = pydicom.dcmread("scan.dcm")
    elem = ds[0x0021, 0x1008]                # private tag
    print(elem.name, elem.VR)                # now resolved via dicom-map

How it works
------------

pydicom keeps its private tag dictionary at
:mod:`pydicom._private_dict` as ``private_dictionaries``: a nested dict of
``{creator: {"GGGGxxEE": (vr, vm, name, keyword)}}``. When pydicom reads a
private element it looks up ``(creator, group, element & 0xFF)`` in that
dict and uses the result for ``DataElement.name`` and (in some code paths)
``DataElement.VR``.

patch_pydicom walks every record in a compiled ``.dmap`` dictionary
and installs the private entries into pydicom's runtime dict. By default
existing pydicom entries are kept (``mode="fill"``) so we only add
coverage; pass ``mode="override"`` to make our data take precedence.

The patch is process-local and reversible via :func:`unpatch_pydicom`.
"""
from __future__ import annotations

import os
from typing import Iterable, Literal

from ._dicom_map import Dict as _Dict
from ._dicom_map import open as _open
from .errors import DmapError

PatchMode = Literal["fill", "override"]

# Snapshot of pydicom's original dict for unpatching.
_ORIGINAL_PYDICOM: dict | None = None
# Track which (creator, key) entries we added so unpatch can remove only ours.
_ADDED_KEYS: set[tuple[str, str]] = set()


def _format_pydicom_key(group: int, element: int, is_block_offset: bool) -> str:
    """Return pydicom's ``GGGGxxEE`` (block-relative) or ``GGGGEEEE`` form."""
    if is_block_offset:
        return f"{group:04X}xx{element:02X}"
    return f"{group:04X}{element:04X}"


def _iter_private_records(d: _Dict) -> Iterable[tuple[str, int, int, bool, str, str, str, str]]:
    """Yield ``(creator, group, element, is_block_offset, vr, vm, name, keyword)``
    for every private record in the dictionary. Public records are skipped by
    design (they have no creator)."""
    for rec in d:
        creator = rec["creator"]
        if not creator:
            continue
        if not rec["vr"] or rec["vr"] == "??" or not rec["vm"]:
            raise DmapError(
                f"private tag {rec['group']:04X},{rec['element']:02X} under {creator!r} "
                f"has no usable VR/VM ({rec['vr']!r}, {rec['vm']!r})"
            )
        yield (
            creator,
            rec["group"],
            rec["element"],
            rec["block_offset"],
            rec["vr"],
            rec["vm"],
            rec["name"],
            rec["keyword"],
        )


def patch_pydicom(
    dmap_path: str | os.PathLike[str],
    *,
    mode: PatchMode = "fill",
) -> int:
    """Register dicom-map's private tag entries into pydicom's runtime dict.

    Parameters
    ----------
    dmap_path
        Path to ``tags.dmap``. Every private record is read from it.
    mode
        ``"fill"`` (default): only add entries that pydicom doesn't already
        have for a given (creator, key). Existing pydicom data is preserved.
        ``"override"``: replace any colliding pydicom entry with ours.

    Returns
    -------
    int
        Number of entries added or replaced.

    Raises
    ------
    ImportError
        If pydicom (or its ``_private_dict`` module) is not available.
    DmapError
        If the file cannot be opened (see :mod:`dicom_map.errors`) or a
        private record has no usable VR or VM. pydicom is left untouched.
    ValueError
        If ``mode`` is not ``"fill"`` or ``"override"``.
    """
    if mode not in ("fill", "override"):
        raise ValueError(f"mode must be 'fill' or 'override', not {mode!r}")
    try:
        import pydicom._private_dict as _pd
    except ImportError as exc:  # pragma: no cover
        raise ImportError(
            "patch_pydicom() requires pydicom; install with `pip install dicom-map[pydicom]`."
        ) from exc
    pdd = getattr(_pd, "private_dictionaries", None)
    if not isinstance(pdd, dict):
        raise ImportError(
            "pydicom._private_dict.private_dictionaries is missing; this pydicom "
            "version is not supported by patch_pydicom()"
        )

    # Read everything before touching pydicom.
    records = list(_iter_private_records(_open(os.fspath(dmap_path))))

    global _ORIGINAL_PYDICOM
    if _ORIGINAL_PYDICOM is None:
        # Snapshot once so unpatch can restore exactly.
        _ORIGINAL_PYDICOM = {creator: dict(tags) for creator, tags in pdd.items()}

    added = 0
    for creator, group, element, is_block, vr, vm, name, keyword in records:
        key = _format_pydicom_key(group, element, is_block)
        bucket = pdd.setdefault(creator, {})
        if key in bucket and mode == "fill":
            continue
        bucket[key] = (vr, vm, name, keyword)
        _ADDED_KEYS.add((creator, key))
        added += 1

    return added


def unpatch_pydicom() -> int:
    """Reverse a previous :func:`patch_pydicom` call.

    Restores pydicom's private dictionary to the state it had immediately
    before the first ``patch_pydicom()`` call in this process. Returns the
    number of entries removed/restored. No-op if patch was never applied.
    """
    global _ORIGINAL_PYDICOM
    if _ORIGINAL_PYDICOM is None:
        return 0

    try:
        import pydicom._private_dict as _pd
    except ImportError:
        return 0

    pdd = _pd.private_dictionaries
    pdd.clear()
    pdd.update({creator: dict(tags) for creator, tags in _ORIGINAL_PYDICOM.items()})
    n = len(_ADDED_KEYS)
    _ADDED_KEYS.clear()
    _ORIGINAL_PYDICOM = None
    return n
