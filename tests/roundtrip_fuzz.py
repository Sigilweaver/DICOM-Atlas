"""Round-trip fuzz test: sample N resolved tags, look each up via the
`dicom-lookup` CLI, and assert the key fields match.

Catches silent mismatches between the scraper output and the compiled
`tags.dmap` (e.g. schema drift, encoding bugs, creator canonicalisation
changes, compiler bugs).

Exits non-zero on any mismatch.
"""
from __future__ import annotations

import json
import random
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESOLVED = ROOT / "data" / "resolved_pydicom_backfilled.jsonl"
DMAP = ROOT / "tags.dmap"
CLI = ROOT / "target" / "release" / "dicom-lookup"

SAMPLE_SIZE = 500


def _require(cond: bool, msg: str) -> None:
    if not cond:
        print(f"FAIL: {msg}", file=sys.stderr)
        sys.exit(1)


def main() -> int:
    _require(RESOLVED.exists(), f"missing {RESOLVED}")
    _require(DMAP.exists(), f"missing {DMAP} — run dmap-compile first")
    _require(CLI.exists(), f"missing {CLI} — run `cargo build --release`")

    rows = [
        json.loads(line)
        for line in RESOLVED.read_text().splitlines()
        if line.strip()
    ]
    private = [r for r in rows if r.get("private_creator")]
    _require(len(private) > SAMPLE_SIZE, "too few private tags to sample")

    rng = random.Random(0xDEADBEEF)
    sample = rng.sample(private, SAMPLE_SIZE)

    mismatches: list[str] = []
    for r in sample:
        group = f"{r['group']:04X}"
        elem_lo = r["element"] & 0xFF
        element = f"xx{elem_lo:02X}"
        creator = r["private_creator"]

        proc = subprocess.run(
            [str(CLI), "--file", str(DMAP), "--json", group, element, creator],
            capture_output=True,
            text=True,
            timeout=10,
        )
        if proc.returncode not in (0, 3):
            mismatches.append(
                f"CLI crashed for ({group},{element}) {creator!r}: "
                f"rc={proc.returncode} stderr={proc.stderr!r}"
            )
            continue
        if proc.returncode == 3:
            mismatches.append(
                f"NOT FOUND ({group},{element}) creator={creator!r}"
            )
            continue

        out = json.loads(proc.stdout.strip())
        # VR must match exactly (unless we have UN scraper-side).
        want_vr = (r.get("vr") or "UN").upper()
        got_vr = (out.get("vr") or "").upper()
        if want_vr != got_vr and want_vr != "UN":
            mismatches.append(
                f"VR mismatch ({group},{element}) {creator!r}: "
                f"scraper={want_vr} dmap={got_vr}"
            )
        # Name must match (dmap may have CamelCased keyword but .name is raw).
        # Skip the name check when the sampled row has name="Unknown" — the
        # compiler's dedup may have promoted a more informative name from a
        # duplicate entry, which is correct behaviour, not a mismatch.
        want_name = (r.get("name") or "").strip()
        got_name = (out.get("name") or "").strip()
        if want_name and want_name != "Unknown" and want_name != got_name:
            mismatches.append(
                f"name mismatch ({group},{element}) {creator!r}: "
                f"scraper={want_name!r} dmap={got_name!r}"
            )

    if mismatches:
        print(f"\n{len(mismatches)} mismatch(es) out of {SAMPLE_SIZE}:", file=sys.stderr)
        for m in mismatches[:50]:
            print(f"  {m}", file=sys.stderr)
        return 1
    print(f"round-trip OK ({SAMPLE_SIZE} samples)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
