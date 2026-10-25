import os
from typing import Iterator, TypedDict

__version__: str

class Tag(TypedDict):
    group: int
    element: int
    creator: str
    keyword: str
    name: str
    vr: str
    vm: str
    description: str
    retired: bool
    block_offset: bool
    flags: int
    sources: list[str]

class Dict:
    def lookup(self, group: int, element: int, creator: str | None = None) -> Tag | None: ...
    def meta(self) -> dict[str, str]: ...
    def __len__(self) -> int: ...
    def __iter__(self) -> Iterator[Tag]: ...
    def __enter__(self) -> Dict: ...
    def __exit__(self, exc_type: object = None, exc: object = None, tb: object = None) -> bool: ...

def open(path: str | os.PathLike[str]) -> Dict: ...
