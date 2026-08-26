#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Turns results.json into the Markdown a pull request gets to read.

    python3 scripts/report.py results.json [--baseline base-results.json]

With one file it reports the state of things. With two it reports the change,
which is what a pull request actually wants to know -- and the two halves are
treated differently on purpose:

**Sizes are exact.** They are deterministic and identical across runs, so any
difference is a real difference and is reported as one, in bytes and per cent,
with no threshold.

**Speeds are not**, and they are reported at a coarser grain because of it.
Size moves are broken out per corpus category; speed moves are not, because the
machine will not support it. Measured here across three runs of identical code:

    spread between runs        p50     p90     p99    worst
    per codec/stage/category  4.14%  12.21%  22.72%  26.10%
    per codec/stage           2.03%   4.43%   5.31%   5.31%

A per-category speed table would have been reporting the runner, not the code:
at that grain a quarter of the value is noise. Aggregated over the whole corpus
it is five per cent, and a threshold above that is worth printing. So the speed
section works per (codec, stage) and calls nothing a change until it clears both
the spread the run measured for itself and SPEED_FLOOR below.

Calling a 2 % move a regression on a machine that moves 5 % by itself would
train everyone to ignore the report, which is the one failure mode that matters
here. Nothing fails a build either way: it reports; a human decides.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

# A speed change has to clear both the measured spread and this floor before it
# is called anything. 8 % sits above the 5.31 % worst case measured between runs
# of identical code (see the module docstring) with enough headroom that a
# quieter or busier runner does not start manufacturing regressions. Re-measure
# it if the corpus or the runner changes: it is calibration, not a preference.
SPEED_FLOOR = 0.08


def load(path: Path) -> dict:
    return json.loads(path.read_text())


def by_category(results: dict) -> dict:
    """Aggregate to (codec, stage, category): the level a reader can act on.

    Per-sample rows are too many to read and per-codec totals hide which kind
    of input moved, which is usually the whole story.
    """
    cat = {s["name"]: s["category"] for s in results["corpus"]["samples"]}
    out: dict = defaultdict(lambda: {
        "input": 0, "json": 0, "raw": 0, "escapes": 0,
        "enc": 0.0, "encw": 0.0, "iqr": 0.0, "dec": 0.0,
    })
    for m in results["measurements"]:
        k = (m["codec"], m["stage"], cat[m["sample"]])
        e = out[k]
        n = m["input_bytes"]
        e["input"] += n
        e["json"] += m["json_bytes"]
        e["raw"] += m["encoded_bytes"]
        e["escapes"] += m["escapes"]
        e["enc"] += m["encode_rel"]["ns"] * n
        e["dec"] += m["decode_rel"]["ns"] * n
        e["iqr"] += m["encode_rel"]["iqr_ns"] * n
    for e in out.values():
        if e["input"]:
            e["enc"] /= e["input"]
            e["dec"] /= e["input"]
            e["iqr"] /= e["input"]
    return out


def totals(results: dict) -> dict:
    """Whole-corpus totals per (codec, stage), for the summary table."""
    out: dict = defaultdict(lambda: {"input": 0, "json": 0, "raw": 0, "escapes": 0,
                                     "enc": 0.0, "iqr": 0.0})
    for m in results["measurements"]:
        e = out[(m["codec"], m["stage"])]
        n = m["input_bytes"]
        e["input"] += n
        e["json"] += m["json_bytes"]
        e["raw"] += m["encoded_bytes"]
        e["escapes"] += m["escapes"]
        e["enc"] += m["encode_rel"]["ns"] * n
        e["iqr"] += m["encode_rel"]["iqr_ns"] * n
    for e in out.values():
        if e["input"]:
            e["enc"] /= e["input"]
            e["iqr"] /= e["input"]
    return out


def pct(new: float, old: float) -> float:
    return (new / old - 1.0) * 100.0 if old else 0.0


def state_report(r: dict, stage: str) -> list[str]:
    t = totals(r)
    rows = sorted(
        ((codec, e) for (codec, st), e in t.items() if st == stage),
        key=lambda kv: kv[1]["json"] / max(kv[1]["input"], 1),
    )
    if not rows:
        return [f"_no measurements at stage `{stage}`_"]
    out = [
        f"#### At `{stage}`",
        "",
        "| codec | size in JSON | raw size | escaped chars | encode vs base64 |",
        "|---|--:|--:|--:|--:|",
    ]
    for codec, e in rows:
        out.append(
            f"| `{codec}` | {e['json'] / e['input']:.4f} | {e['raw'] / e['input']:.4f} "
            f"| {e['escapes'] or '—'} | {e['enc']:.2f}× ±{e['iqr'] / 2:.2f} |"
        )
    return out


def diff_report(new: dict, old: dict) -> list[str]:
    """Head against base. Sizes exactly, speeds only past the measured noise."""
    n, o = by_category(new), by_category(old)
    shared = sorted(set(n) & set(o))

    out: list[str] = []

    # --- size: per category, exactly ---------------------------------
    size_moves = [(k, n[k], o[k]) for k in shared
                  if n[k]["json"] != o[k]["json"] or n[k]["raw"] != o[k]["raw"]]

    out.append("#### Size — exact, no threshold")
    out.append("")
    if not size_moves:
        out.append("No size changed, on any codec, at any stage, for any category.")
    else:
        out.append("| codec | stage | category | JSON bytes | change | escaped chars |")
        out.append("|---|---|---|--:|--:|--:|")
        size_moves.sort(key=lambda x: -abs(x[1]["json"] - x[2]["json"]))
        for (codec, stage, cat), a, b in size_moves[:40]:
            d = a["json"] - b["json"]
            out.append(
                f"| `{codec}` | `{stage}` | {cat} | {a['json']:,} | "
                f"{d:+,} ({pct(a['json'], b['json']):+.2f} %) | "
                f"{a['escapes']:,} ({a['escapes'] - b['escapes']:+,}) |"
            )
        if len(size_moves) > 40:
            out.append(f"| … | | | | _{len(size_moves) - 40} more_ | |")
    out.append("")

    # --- speed: per codec and stage, over the whole corpus ------------
    # Deliberately coarser than the size table; see the module docstring for
    # the measurement that settled the grain.
    tn, to = totals(new), totals(old)
    speed_moves, quiet = [], 0
    for k in sorted(set(tn) & set(to)):
        a, b = tn[k], to[k]
        gap = a["enc"] - b["enc"]
        noise = max((a["iqr"] + b["iqr"]) / 2, SPEED_FLOOR * max(b["enc"], 1e-9))
        if abs(gap) > noise:
            speed_moves.append((k, a, b, gap, noise))
        elif gap:
            quiet += 1

    out.append("#### Speed — whole corpus, only what clears the measured spread")
    out.append("")
    if not speed_moves:
        out.append(
            f"Nothing moved further than this machine's own noise. "
            f"({quiet} codec/stage combinations changed by less than that; on a "
            f"shared runner such a move is not a result.)"
        )
    else:
        out.append("| codec | stage | encode vs base64 | change | called a change past |")
        out.append("|---|---|--:|--:|--:|")
        speed_moves.sort(key=lambda x: -abs(x[3]))
        for (codec, stage), a, b, gap, noise in speed_moves:
            arrow = "slower" if gap > 0 else "faster"
            out.append(
                f"| `{codec}` | `{stage}` | {b['enc']:.2f}× → {a['enc']:.2f}× "
                f"| **{pct(a['enc'], b['enc']):+.1f} % {arrow}** | ±{noise:.3f} |"
            )
        out.append("")
        out.append(f"_{quiet} other combinations moved by less than the noise floor._")
    out.append("")

    gone = sorted(set(o) - set(n))
    added = sorted(set(n) - set(o))
    if gone or added:
        out.append("")
        if gone:
            out.append(f"_{len(gone)} cell(s) present on the base branch are missing here._")
        if added:
            out.append(f"_{len(added)} cell(s) are new._")
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("results", type=Path)
    ap.add_argument("--baseline", type=Path, default=None)
    ap.add_argument("--stage", default="zstd:1",
                    help="which compression stage the state table shows")
    ap.add_argument("--title", default="Benchmark")
    args = ap.parse_args()

    new = load(args.results)
    meta = new["meta"]
    corpus = new["corpus"]["samples"]
    total = sum(s["bytes"] for s in corpus)

    lines = [f"## {args.title}", ""]
    lines.append(
        f"{len(corpus)} samples · {total:,} bytes · {meta['rounds']} rounds · "
        f"`{meta['cpu']}`"
    )
    lines.append("")

    if args.baseline and args.baseline.exists():
        old = load(args.baseline)
        lines.append(
            "Head against the base branch, both measured in this job on this "
            "machine — which is why the two are comparable at all."
        )
        lines.append("")
        lines += diff_report(new, old)
        lines.append("")
        lines.append("<details><summary>Where things stand on the head</summary>")
        lines.append("")
        lines += state_report(new, args.stage)
        lines.append("")
        lines.append("</details>")
    else:
        lines += state_report(new, args.stage)

    lines.append("")
    revs = ", ".join(f"{k} `{v[:12]}`" for k, v in (meta.get("codec_revisions") or {}).items())
    lines.append(f"<sub>{revs} · {meta['rustc']} · {meta['generated']}</sub>")

    sys.stdout.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
