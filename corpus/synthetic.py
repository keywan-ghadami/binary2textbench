# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""The synthetic group: the shapes the weighting profiles are built on.

The core and Silesia groups are whole files of one kind each, and the short
group is protocol fields. Neither answers the question a caller actually has,
which is what an encoding costs *them* -- and that depends on what their data
looks like. Someone shipping log lines with the occasional stray byte is in a
different position from someone shipping ciphertext, and averaging the two
tells neither of them anything.

So this group varies one property at a time, deterministically:

  problematic-byte density
      Text that is otherwise plain ASCII, with bytes mixed in that make it
      untransportable as text -- C0 controls, DEL, and raw high bytes that
      form no valid UTF-8. This is the reason a binary-to-text encoding gets
      reached for at all, and the density is what decides whether escaping the
      text in place would have been cheaper. Four densities, from none to one
      byte in ten, at three lengths each, because a fixed overhead that
      vanishes at 256 KiB decides the answer at 64 bytes.

  mixture
      Text, JSON, source and a binary container interleaved in one payload,
      which is what a real envelope tends to hold.

  incompressibility
      Session identifiers, ciphertext, and already-compressed blobs. These are
      where a compression stage in front of the encoder earns nothing, so the
      encoding is on its own -- and they are common in exactly the traffic
      that reaches for one.

Nothing here is downloaded and nothing uses a random seed from the
environment: the filler bytes come from a SHA-256 counter stream, so a rerun
on any machine produces the same bytes or something is wrong. The base text is
taken from the core group, so the text being disturbed has a real byte
distribution rather than an invented one.
"""

from __future__ import annotations

import gzip
import hashlib
import lzma
from pathlib import Path

# Bytes that make a payload untransportable as text: the C0 controls that are
# not tab, newline or carriage return; DEL; and the high half, which as single
# bytes among ASCII is never valid UTF-8. This is the set the density classes
# below inject, and the definition the profiles mean by "problematic".
PROBLEMATIC = bytes(
    [b for b in range(0x00, 0x20) if b not in (0x09, 0x0A, 0x0D)]
    + [0x7F]
    + list(range(0x80, 0x100))
)

# One sample per (density, length). The lengths span the range where the answer
# changes: a protocol field, a log line or small record, and a payload big
# enough that per-message overhead has washed out.
DENSITIES = (
    ("text_clean", 0.0),
    ("text_sparse", 0.001),
    ("text_moderate", 0.01),
    ("text_dense", 0.10),
)
LENGTHS = (64, 4096, 262144)

# How much of each incompressible class to write.
CIPHERTEXT_BYTES = 262144
SESSION_ID_LENGTHS = (16, 24, 32)
SESSION_IDS_PER_LENGTH = 64


def keystream(label: str, n: int) -> bytes:
    """A deterministic pseudo-random byte stream, SHA-256 in counter mode.

    Not a random number generator anyone should encrypt with; the point is
    that it is reproducible from a label alone, in any language, without a
    dependency and without depending on how a given Python version happens to
    implement its own generator.
    """
    out = bytearray()
    counter = 0
    seed = label.encode()
    while len(out) < n:
        out += hashlib.sha256(seed + counter.to_bytes(8, "big")).digest()
        counter += 1
    return bytes(out[:n])


def _tile(source: bytes, n: int) -> bytes:
    """Repeat a sample up to n bytes. Repetition is what a compressor eats, so
    this is only ever used for text, where that is the honest shape anyway."""
    if not source:
        raise ValueError("empty source")
    return (source * (n // len(source) + 1))[:n]


def _inject(text: bytes, density: float, label: str) -> bytes:
    """Replace `density` of the bytes with problematic ones, at positions and
    with values drawn from the keystream. Replacing rather than inserting keeps
    the length exact, so a size comparison across densities is a comparison."""
    if density <= 0:
        return text
    count = max(1, round(len(text) * density))
    stream = keystream(f"{label}:inject", count * 5)
    out = bytearray(text)
    for i in range(count):
        # Four bytes choose the position, one chooses the replacement.
        pos = int.from_bytes(stream[i * 5:i * 5 + 4], "big") % len(out)
        out[pos] = PROBLEMATIC[stream[i * 5 + 4] % len(PROBLEMATIC)]
    return bytes(out)


def _base_text(core: dict[str, Path]) -> bytes:
    """Plain prose from the core group: the CommonMark specification, which is
    long-form English with code blocks, the closest thing the corpus has to
    ordinary text going over a wire."""
    return core["commonmark-spec.txt"].read_bytes()


def write_into(target: Path, core: dict[str, Path]) -> list[tuple[str, str, int]]:
    """Materialise the synthetic group. Returns (name, category, size) per file.

    `core` maps core-group file names to their paths; the group is built on top
    of real data rather than invented data, so it needs the core group present.
    """
    target.mkdir(parents=True, exist_ok=True)
    written: list[tuple[str, str, int]] = []

    def emit(name: str, category: str, data: bytes) -> None:
        (target / name).write_bytes(data)
        written.append((name, category, len(data)))

    text = _base_text(core)

    # --- problematic-byte density, at three lengths ---------------------
    for category, density in DENSITIES:
        for length in LENGTHS:
            label = f"{category}-{length}"
            emit(f"{label}.bin", category, _inject(_tile(text, length), density, label))

    # --- a mixed payload ------------------------------------------------
    # Segments of four kinds in one buffer, in the proportions an envelope
    # tends to carry: mostly structured text, some source, a binary tail.
    parts = [
        core["countries.min.json"].read_bytes()[:96 * 1024],
        text[:64 * 1024],
        core["requests-models.py"].read_bytes()[:32 * 1024],
        core["sql-wasm.wasm"].read_bytes()[:32 * 1024],
    ]
    stride = 4096
    mixed = bytearray()
    for offset in range(0, max(len(p) for p in parts), stride):
        for part in parts:
            mixed += part[offset:offset + stride]
    emit("mixed.bin", "mixed", bytes(mixed))

    # --- incompressible: session identifiers ----------------------------
    # Concatenated rather than one file per token: 192 files of 24 bytes would
    # drown every other sample in the per-file bookkeeping, and what is being
    # measured is the token, not the file.
    for length in SESSION_ID_LENGTHS:
        blob = keystream(f"session-ids:{length}", length * SESSION_IDS_PER_LENGTH)
        emit(f"session_ids_{length}.bin", "session_ids", blob)

    # --- incompressible: ciphertext -------------------------------------
    # A real file under a keystream XOR. The result has the length and the
    # framing of the plaintext and none of its redundancy, which is what an
    # encrypted payload looks like to an encoder.
    plain = core["requests-2.32.3.tar"].read_bytes()[:CIPHERTEXT_BYTES]
    stream = keystream("ciphertext", len(plain))
    emit("encrypted.bin", "encrypted", bytes(a ^ b for a, b in zip(plain, stream)))

    # --- incompressible: already compressed -----------------------------
    # Two containers, because the framing differs and the framing is the only
    # part an encoder can still find structure in. zstd is what the benchmark
    # itself compresses with, but it is not in the Python standard library and
    # this group must not need anything installed; xz and gzip are, and their
    # payloads are just as incompressible.
    source = core["countries.json"].read_bytes()[:1024 * 1024]
    emit("precompressed.xz", "precompressed", lzma.compress(source, preset=9))
    emit("precompressed.gz", "precompressed",
         gzip.compress(source, compresslevel=9, mtime=0))

    # --- the hard ceiling -----------------------------------------------
    # Nothing compresses this and no encoding does better than its own base
    # ratio on it, which makes it the line every other result sits above.
    emit("random.bin", "random", keystream("random", 262144))

    return written


def core_inputs() -> tuple[str, ...]:
    """Core-group file names this module reads, so the caller can make sure
    they are present before calling `write_into`."""
    return (
        "commonmark-spec.txt",
        "countries.json",
        "countries.min.json",
        "requests-models.py",
        "requests-2.32.3.tar",
        "sql-wasm.wasm",
    )
