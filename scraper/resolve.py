"""Resolve stage: merge NormalizedTag JSON-L files → ResolvedTag list.

Deduplication key:  (group, element_low, private_creator_canon).

When two records agree on the key we union their vendors/sources and pick
the longest non-empty description and the most specific VR. Conflicts on VR
are reported but non-fatal (we keep the first-seen and flag).
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

from rich.console import Console
from rich.table import Table

from scraper.models import NormalizedTag, ResolvedTag
from scraper.normalize import _clean_creator

try:
    # Optional: use pydicom as a tiebreaker for VR conflicts.
    from scraper.pydicom_compare import lookup as _pydicom_lookup
except Exception:  # pragma: no cover - pydicom optional at resolve time
    _pydicom_lookup = None  # type: ignore[assignment]

console = Console()


def _key(n: NormalizedTag) -> tuple:
    return (n.group, n.element, (n.private_creator or ""))


def load_jsonl(path: Path) -> list[NormalizedTag]:
    out: list[NormalizedTag] = []
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            t = NormalizedTag.model_validate_json(line)
            # Re-apply creator cleaning at load time so interim .jsonl files
            # harvested with older (dirtier) code benefit without a re-run.
            t.private_creator = _clean_creator(t.private_creator)
            out.append(t)
    return out


def resolve(
    all_tags: list[NormalizedTag],
) -> tuple[list[ResolvedTag], list[str]]:
    grouped: dict[tuple, list[NormalizedTag]] = defaultdict(list)
    for t in all_tags:
        grouped[_key(t)].append(t)

    conflicts: list[str] = []
    resolved: list[ResolvedTag] = []

    for key, members in grouped.items():
        # Vote over real (non-inferred) VRs. Each source PDF gets one vote.
        vr_votes: dict[str, int] = defaultdict(int)
        seen_pdfs_per_vr: dict[str, set[str]] = defaultdict(set)
        for m in members:
            if getattr(m, "vr_inferred", False):
                continue
            if not m.vr or m.vr == "UN":
                continue
            # One vote per distinct source PDF to avoid a single PDF that
            # repeats a row (e.g. "continued" tables) dominating the tally.
            if m.source_pdf in seen_pdfs_per_vr[m.vr]:
                continue
            seen_pdfs_per_vr[m.vr].add(m.source_pdf)
            vr_votes[m.vr] += 1

        if vr_votes:
            if len(vr_votes) == 1:
                vr = next(iter(vr_votes))
            else:
                # Majority vote. Ties broken by pydicom lookup when available,
                # else by lexicographic VR to stay deterministic.
                max_votes = max(vr_votes.values())
                top = sorted(v for v, n in vr_votes.items() if n == max_votes)
                vr = top[0]
                if len(top) > 1 and _pydicom_lookup is not None:
                    creator = members[0].private_creator
                    hit = _pydicom_lookup(
                        creator, members[0].group, members[0].element
                    )
                    if hit and hit[0] in top:
                        vr = hit[0]
                conflicts.append(
                    f"VR conflict at {key}: votes={dict(vr_votes)} chose={vr}"
                )
        else:
            # No real VR votes — every PDF entry was inferred or UN. Try
            # pydicom as a last resort before falling back to UN.
            vr = "UN"
            if _pydicom_lookup is not None:
                creator = members[0].private_creator
                hit = _pydicom_lookup(
                    creator, members[0].group, members[0].element
                )
                if hit and hit[0] and hit[0] != "UN":
                    vr = hit[0]

        # prefer longest description, longest name
        best_desc = max((m.description for m in members), key=len, default="")
        best_name = max((m.name for m in members), key=len, default="")
        best_keyword = max(
            (m.keyword or "" for m in members), key=len, default=""
        )

        vendors = sorted({m.vendor for m in members})
        sources = sorted(
            {f"{m.source_pdf}#p{m.source_page}" for m in members}
        )

        vm_values = {m.vm for m in members}
        vm = next(iter(vm_values)) if len(vm_values) == 1 else sorted(vm_values)[0]

        resolved.append(
            ResolvedTag(
                group=members[0].group,
                element=members[0].element,
                element_is_block_offset=members[0].element_is_block_offset,
                private_creator=members[0].private_creator,
                keyword=best_keyword or best_name.replace(" ", ""),
                name=best_name,
                vr=vr,
                vm=vm,
                description=best_desc,
                retired=any(m.retired for m in members),
                vendors=vendors,
                sources=sources,
            )
        )

    # canonical ordering: creator, group, element
    resolved.sort(key=lambda r: (r.private_creator or "", r.group, r.element))
    return resolved, conflicts


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "inputs", nargs="+", type=Path, help="NormalizedTag .jsonl files"
    )
    ap.add_argument("-o", "--output", type=Path, required=True)
    args = ap.parse_args(argv)

    all_tags: list[NormalizedTag] = []
    for p in args.inputs:
        all_tags.extend(load_jsonl(p))

    resolved, conflicts = resolve(all_tags)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8") as fh:
        for r in resolved:
            fh.write(r.model_dump_json() + "\n")

    t = Table(title="resolve")
    t.add_column("metric")
    t.add_column("value", justify="right")
    t.add_row("input files", str(len(args.inputs)))
    t.add_row("input tags", str(len(all_tags)))
    t.add_row("resolved tags", str(len(resolved)))
    t.add_row("VR conflicts", str(len(conflicts)))
    t.add_row("output", str(args.output))
    console.print(t)

    for c in conflicts[:10]:
        console.print(f"[yellow]{c}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
