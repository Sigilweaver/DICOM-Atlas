"""Exceptions raised by :mod:`dicom_map`.

Every error is a :class:`DmapError` (a ``ValueError``). Catch the base class to
handle anything this package raises, or a subclass for one cause. New
subclasses can be added in later releases.
"""
from __future__ import annotations


class DmapError(ValueError):
    """Base class for all errors raised by dicom_map."""


class DmapIOError(DmapError, OSError):
    """The file could not be read (missing, permissions, ...).

    Also an ``OSError``: ``errno``, ``strerror`` and ``filename`` are set.
    """

    def __init__(self, *args: object) -> None:
        super().__init__(*args)
        if len(args) >= 2 and isinstance(args[0], int):
            self.errno = args[0]
            self.strerror = str(args[1])
            self.filename = args[2] if len(args) >= 3 else None
        else:
            self.errno = None
            self.strerror = str(args[0]) if args else ""
            self.filename = None

    def __str__(self) -> str:
        if self.errno is not None:
            return f"[Errno {self.errno}] {self.strerror}: {self.filename!r}"
        return self.strerror


class DmapFormatError(DmapError):
    """The file is not a ``.dmap`` archive this version can read."""


class UnsupportedVersionError(DmapFormatError):
    """The archive's format major is not the one this version reads.

    Attributes: ``got_major``, ``got_minor``, ``supported_major``.
    """

    got_major: int
    got_minor: int
    supported_major: int


class BadArchiveError(DmapFormatError):
    """The archive is damaged or inconsistent.

    ``check`` names the failed check (for example ``"index order"``).
    """

    check: str
