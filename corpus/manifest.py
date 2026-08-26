# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Writes corpus/data/manifest.json: what the runner reads.

The runner knows nothing about downloads, archives, seeds or injection
densities. It reads this file, which says where each sample is, what class it
belongs to and what it is -- and that is the whole interface between how the
corpus is made and how it is measured. Changing one does not touch the other.

    python3 corpus/manifest.py --groups=core,short,synthetic

The SHA-256 of every sample is recorded so that a result can be tied to the
exact bytes it was produced from. Two runs that disagree on a size, on
machines that agree on this file, disagree about the encoder.
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import corpus  # noqa: E402


def build(groups: tuple[str, ...]) -> dict:
    entries = []
    for sample, path in corpus.ensure_corpus(groups=groups):
        data = path.read_bytes()
        entries.append({
            "name": sample.name,
            "group": sample.group,
            "category": sample.category,
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
            "origin": sample.origin,
            # Relative to the manifest itself, so the runner needs no notion of
            # where the repository is checked out.
            "path": str(path.relative_to(corpus.CORPUS_DIR)),
        })
    entries.sort(key=lambda e: (e["group"], e["category"], e["name"]))
    return {"groups": list(groups), "samples": entries}


def _cli() -> None:
    groups = corpus.GROUPS
    for arg in sys.argv[1:]:
        if arg.startswith("--groups="):
            groups = tuple(g.strip() for g in arg.split("=", 1)[1].split(",") if g.strip())
    unknown = set(groups) - set(corpus.GROUPS)
    if unknown:
        raise SystemExit(f"unknown group(s): {', '.join(sorted(unknown))}")

    manifest = build(groups)
    out = corpus.CORPUS_DIR / "manifest.json"
    out.write_text(json.dumps(manifest, indent=2) + "\n")
    total = sum(e["bytes"] for e in manifest["samples"])
    print(f"{out}: {len(manifest['samples'])} samples, {total} bytes", file=sys.stderr)


if __name__ == "__main__":
    _cli()
