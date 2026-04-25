"""Cross-reference resolved tags with pydicom's bundled private dictionary.

Reports:
  - agreements: both have same VR
  - disagreements: both present, different VR
  - pydicom-only: present in pydicom but not our scrape (for given creator+tag)
  - scrape-only: present in scrape but not pydicom

Also writes data/resolved_pydicom_backfilled.jsonl where any of our tags
with vr_inferred=True (or missing VR) get pydicom's VR filled in when a
matching entry exists and creators match.
"""
from __future__ import annotations
import json
from collections import Counter, defaultdict
from pathlib import Path

from pydicom._private_dict import private_dictionaries as _PD


def _norm_creator(s: str | None) -> str:
    return (s or "").strip().upper()


# Build index: {normalized_creator: {"GGGGxxEE" : (vr, vm, name)}}
_IDX: dict[str, dict[str, tuple[str, str, str]]] = {}
for creator, entries in _PD.items():
    nc = _norm_creator(creator)
    m = _IDX.setdefault(nc, {})
    for key, meta in entries.items():
        vr = (meta[0] or "").strip()
        vm = (meta[1] or "").strip() if len(meta) > 1 else ""
        name = (meta[2] or "").strip() if len(meta) > 2 else ""
        m[key.lower()] = (vr, vm, name)


def _key(group: int, element_lo: int) -> str:
    return f"{group:04x}xx{element_lo:02x}"


def lookup(creator: str | None, group: int, element_lo: int):
    m = _IDX.get(_norm_creator(creator))
    if not m:
        return None
    return m.get(_key(group, element_lo))


def main():
    resolved_path = Path("data/resolved.jsonl")
    rows = [json.loads(ln) for ln in resolved_path.read_text().splitlines() if ln.strip()]
    agree = dis = scrape_only = 0
    vr_inferred_match = vr_inferred_mismatch = vr_inferred_nohit = 0
    by_vendor_agree: Counter[str] = Counter()
    by_vendor_total: Counter[str] = Counter()
    disagreements_sample: list[tuple] = []

    # Collect keys that ARE present in pydicom for given creator, indexed on
    # (creator, key) so we can compute pydicom-only.
    seen_scrape_keys: set[tuple[str, str]] = set()

    out_rows = []
    for r in rows:
        creator = r.get("private_creator")
        grp = r["group"]
        elem_lo = r["element"]
        ours_vr = r.get("vr")
        # VR-inferred flag isn't directly in resolved; we treat VR=="UN" as
        # likely inferred (plus we re-read raw later if needed).
        vr_inferred = r.get("vr_inferred", False) or ours_vr == "UN"

        vendor_list = r.get("vendors") or []
        for v in vendor_list:
            by_vendor_total[v] += 1

        if not creator:
            out_rows.append(r)
            continue

        seen_scrape_keys.add((_norm_creator(creator), _key(grp, elem_lo)))
        hit = lookup(creator, grp, elem_lo)
        if hit is None:
            scrape_only += 1
            if vr_inferred:
                vr_inferred_nohit += 1
            out_rows.append(r)
            continue
        pvr, pvm, pname = hit
        # Backfill empty/missing name and vm from pydicom regardless of VR agreement.
        if pname and not (r.get("name") or "").strip():
            r["name"] = pname
            r.setdefault("notes", []).append("name backfilled from pydicom")
        if pvm and not (r.get("vm") or "").strip():
            r["vm"] = pvm
            r.setdefault("notes", []).append("vm backfilled from pydicom")
        if not ours_vr and pvr:
            r["vr"] = pvr
            r["vr_inferred"] = False
            r.setdefault("notes", []).append("VR backfilled from pydicom (was empty)")
            ours_vr = pvr
        if pvr and ours_vr and pvr == ours_vr:
            agree += 1
            for v in vendor_list:
                by_vendor_agree[v] += 1
        elif pvr and ours_vr and pvr != ours_vr:
            dis += 1
            if vr_inferred:
                # Backfill from pydicom since ours was inferred.
                r["vr"] = pvr
                r["vr_inferred"] = False
                r.setdefault("notes", []).append(
                    f"VR backfilled from pydicom (was {ours_vr})"
                )
                vr_inferred_match += 1
            else:
                vr_inferred_mismatch += 1
                if len(disagreements_sample) < 15:
                    disagreements_sample.append(
                        (creator, grp, elem_lo, ours_vr, pvr, r.get("name"))
                    )
        out_rows.append(r)

    # pydicom-only
    pydicom_only = 0
    for nc, m in _IDX.items():
        for k in m:
            if (nc, k) not in seen_scrape_keys:
                pydicom_only += 1

    print(f"Total resolved tags:      {len(rows)}")
    print(f"Tags with creator:        {sum(1 for r in rows if r.get('private_creator'))}")
    print(f"  agreed with pydicom:    {agree}")
    print(f"  disagreed (kept ours):  {vr_inferred_mismatch}")
    print(f"  backfilled inferred VR: {vr_inferred_match}")
    print(f"  scrape-only:            {scrape_only}  (of which {vr_inferred_nohit} have UN/inferred VR)")
    print(f"  pydicom-only:           {pydicom_only}")
    print()
    print("Per-vendor agreement:")
    for v, total in by_vendor_total.most_common():
        agreed = by_vendor_agree[v]
        pct = 100 * agreed / total if total else 0
        print(f"  {v:12s} {agreed:5d}/{total:5d}  ({pct:5.1f}%)")
    print()
    print("Sample disagreements:")
    for d in disagreements_sample:
        print(f"  creator={d[0]!r} ({d[1]:04x},xx{d[2]:02x}) ours={d[3]} pydicom={d[4]} name={d[5]!r}")

    out_path = Path("data/resolved_pydicom_backfilled.jsonl")
    with out_path.open("w") as fh:
        for r in out_rows:
            fh.write(json.dumps(r) + "\n")
    print(f"\nWrote {out_path} ({len(out_rows)} rows)")


if __name__ == "__main__":
    main()
