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

:func:`patch_pydicom` walks every record in a compiled ``.dmap`` dictionary
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


def _iter_dmap_private_records(d: _Dict) -> Iterable[tuple[str, int, int, bool, str, str, str, str]]:
    """Yield ``(creator, group, element, is_block_offset, vr, vm, name, keyword)``
    for every private record in the dictionary.

    The Rust ``Dict`` doesn't currently expose a full iterator (lookup-only),
    so we rely on the CSV being co-located. If that's missing we fall back to
    iterating via the lookup interface - but since lookup needs a key, the
    practical fallback is "this won't work without the CSV". A future schema
    revision will add a `Dict.iter_private()` method to remove this dependency.
    """
    raise NotImplementedError(
        "Iteration over the binary dict is not yet exposed; use "
        "iter_records_from_csv() for now."
    )


def _iter_records_from_csv(csv_path: str | os.PathLike[str]):
    """Stream private records from the canonical ``tags.csv``.

    Yields tuples ``(creator, group, element, is_block_offset, vr, vm,
    name, keyword)``. Public (creator-less) tags are skipped.
    """
    import csv as _csv

    with open(csv_path, newline="", encoding="utf-8") as fh:
        reader = _csv.DictReader(fh)
        for row in reader:
            creator = row.get("private_creator", "").strip()
            if not creator:
                continue
            elem_str = row.get("element", "")
            if elem_str.lower().startswith("xx"):
                element = int(elem_str[2:], 16)
                is_block = True
            else:
                element = int(elem_str, 16)
                is_block = False
            yield (
                creator,
                int(row["group"], 16),
                element,
                is_block,
                row.get("vr", "") or "UN",
                row.get("vm", "") or "1",
                row.get("name", "") or "",
                row.get("keyword", "") or "",
            )


def patch_pydicom(
    dmap_path: str | os.PathLike[str] | None = None,
    *,
    csv_path: str | os.PathLike[str] | None = None,
    mode: PatchMode = "fill",
) -> int:
    """Register dicom-map's private tag entries into pydicom's runtime dict.

    Parameters
    ----------
    dmap_path
        Path to ``tags.dmap``. Used only to validate the dictionary opens
        cleanly. The actual entry list is read from ``csv_path`` (because the
        binary format does not yet expose iteration).
    csv_path
        Path to ``tags.csv``. If omitted, defaults to ``<dmap_path>.csv``-style
        sibling lookup - i.e. ``tags.csv`` next to ``tags.dmap``.
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
        If pydicom is not installed.
    FileNotFoundError
        If neither a discoverable CSV nor an explicit ``csv_path`` is found.
    """
    try:
        import pydicom._private_dict as _pd
    except ImportError as exc:  # pragma: no cover
        raise ImportError(
            "patch_pydicom() requires pydicom; install with `pip install pydicom`."
        ) from exc

    # Locate the CSV if not given.
    if csv_path is None:
        if dmap_path is not None:
            sibling = os.path.join(os.path.dirname(os.fspath(dmap_path)), "tags.csv")
            if os.path.exists(sibling):
                csv_path = sibling
        if csv_path is None:
            raise FileNotFoundError(
                "patch_pydicom() needs tags.csv to enumerate entries. Pass "
                "csv_path=... explicitly or place tags.csv next to tags.dmap."
            )

    # Validate the dmap opens cleanly if provided (sanity check; we don't
    # otherwise read from it yet).
    if dmap_path is not None:
        _ = _open(os.fspath(dmap_path))

    global _ORIGINAL_PYDICOM
    if _ORIGINAL_PYDICOM is None:
        # Snapshot once so unpatch can restore exactly.
        _ORIGINAL_PYDICOM = {
            creator: dict(tags) for creator, tags in _pd.private_dictionaries.items()
        }

    pdd: dict[str, dict[str, tuple[str, str, str, str]]] = _pd.private_dictionaries
    added = 0

    for creator, group, element, is_block, vr, vm, name, keyword in _iter_records_from_csv(csv_path):
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
