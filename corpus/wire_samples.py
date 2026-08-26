# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Short field-level samples of the kind that dominate real traffic.

Large files make ratios look tidy, but most encoded payloads in a real
system are small: an identifier, a token, a digest, one record of JSON.
Those are also where a block encoding's fixed overhead and its rounding up
to whole groups hurt most, so they are worth measuring separately rather
than averaging away.

They are also the only thing that measures the packed bases of the
specification's section 9. Neither the core corpus nor Silesia contains a
hex dump, a column of digits or a base64 blob, which is exactly the shape
those classes exist for -- so until this file existed, thirteen of the
format's classes had never been exercised by a benchmark at all.

The set is built around that: every packed class has samples that should
land in it, at several lengths around the thresholds of section 11.1, and
alongside them the ordinary protocol fields that should *not* -- so that a
class firing where it does not belong shows up as a ratio getting worse.

All values here are invented. Phone numbers use the ranges reserved for
fiction (+1-555-01xx in North America, +49-30-23125xx in Germany), the
addresses use example.com, and every identifier, key and digest is random
bytes formatted to look like the real thing. Nothing here came from a real
system.
"""

from __future__ import annotations

# (label, category, value). The category is what the sample is meant to
# exercise, and it groups the report; it is not a claim about which class
# the encoder will actually choose.
WIRE_SAMPLES: list[tuple[str, str, str]] = [
    # --- decimal digits: class DEC, w = 4 -------------------------------
    ("customer number", "dec", "4711"),
    ("order number", "dec", "184223"),
    ("account number, 12 digits", "dec", "409318827405"),
    ("card number, 16 digits", "dec", "4111111111111111"),
    ("IMEI", "dec", "356938035643809"),
    ("epoch milliseconds", "dec", "1786425164318"),
    ("digit run, 64", "dec", "9" + "0384715926" * 6 + "371"),

    # --- hexadecimal: HEXL, HEXU, w = 4 ---------------------------------
    ("hex, 8 bytes", "hex", "deadbeefcafebabe"),
    ("hex, 16 bytes", "hex", "5f2c9a71e3b04d68af1c2e5b7d90436a"),
    ("SHA-256 digest", "hex",
     "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"),
    ("SHA-512 digest", "hex",
     "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce"
     "47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"),
    ("SHA-256, uppercase", "hex",
     "9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08"),
    ("git commit id", "hex", "6c1e3f0a9b2d4c7e8f01a2b3c4d5e6f708192a3b"),
    ("AES-256 key", "hex",
     "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0"),

    # --- hex with separators: HEXL_D, HEXU_D, w = 5 ---------------------
    ("UUID v4", "hexsep", "b0f1c2d3-4e5a-4b6c-8d9e-0f1a2b3c4d5e"),
    ("UUID, uppercase", "hexsep", "B0F1C2D3-4E5A-4B6C-8D9E-0F1A2B3C4D5E"),
    ("two UUIDs", "hexsep",
     "b0f1c2d3-4e5a-4b6c-8d9e-0f1a2b3c4d5e-"
     "7a8b9c0d-1e2f-4a3b-9c8d-7e6f5a4b3c2d"),

    # --- base32 and its dialects: B32, B32H, CROCK, w = 5 ---------------
    ("TOTP secret, base32", "b32", "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP"),
    ("base32, 40 bytes", "b32",
     "MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43UOZ3G65DFMFRGGZDFMZTWQ2LKNNWG23TP"),
    ("ULID, Crockford", "crock", "01ARZ3NDEKTSV4RRFFQ69G5FAV"),
    ("two ULIDs", "crock", "01ARZ3NDEKTSV4RRFFQ69G5FAV01BX5ZZKBKACTAV9WEVGEMMVRZ"),

    # --- base64 and base64url: B64, B64U, w = 6 -------------------------
    ("base64, 24 bytes", "b64", "SGVsbG8sIHdvcmxkISBUaGlzIGlzIGE0NA"),
    ("base64, 48 bytes", "b64",
     "VGhlIHF1aWNrIGJyb3duIGZveCBqdW1wcyBvdmVyIHRoZSBsYXp5IGRvZy4gMTIz"),
    ("base64 with padding", "b64", "SGVsbG8sIHdvcmxkIQ=="),
    ("JWT, three segments", "b64u",
     "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9."
     "eyJzdWIiOiIxODQyMjMiLCJuYW1lIjoiQWRhIExvdmVsYWNlIiwiaWF0IjoxNzY3MjI1NjAwfQ."
     "3Yv1kQ8Zr7pNc2LxWmA4hTgKdF9sBvE0uJqRnXoYiPs"),
    ("base64url, no padding", "b64u",
     "cGF5bG9hZC13aXRoLXVuZGVyc2NvcmVzX2FuZC1kYXNoZXMtaGVyZQ"),

    # --- alphanumeric identifiers: ALNUM, w = 6 -------------------------
    ("API key", "alnum", "sk7Hn2QwErTyUiOpAsDfGhJkLzXcVbNm"),
    ("nanoid", "alnum", "V1StGXR8Z5jdHi6BmyT"),
    ("session id, 40", "alnum", "a7Kd93JfQ2mZx8Lp0RtYv6BnCw4HsGe1UiOoPl5T"),
    ("upper-case code", "alnum", "XKCDQRSTUVWXYZABCDEF"),
    ("lower-case slug", "alnum", "orderconfirmationemailtemplate"),

    # --- passthrough territory: text the alphabet carries ---------------
    ("first + last name", "text", "Ada Lovelace"),
    ("name with umlauts", "text", "Anna-Lena Müller-Schmidt"),
    ("email address", "text", "ada.lovelace@example.com"),
    ("URL with query", "text",
     "https://api.example.com/v2/orders/184223?expand=items&fields=id,total"),
    ("ISO 8601 timestamp", "text", "2026-08-10T07:12:44.318Z"),
    ("IPv4 address", "text", "192.0.2.147"),
    ("IPv6 address", "text", "2001:db8:85a3::8a2e:370:7334"),
    ("MAC address", "text", "3c:5a:b4:0f:2e:91"),
    ("IBAN", "text", "DE89370400440532013000"),
    ("phone, E.164", "text", "+493023125190"),
    ("phone, formatted", "text", "+1 (415) 555-0132"),
    ("currency amount", "text", "1284.95 EUR"),
    ("CSV row", "text",
     "184223,Ada Lovelace,+493023125190,1284.95,EUR,2026-08-10,shipped"),
    ("JSON record", "text",
     '{"id":184223,"name":"Ada Lovelace","phone":"+493023125190",'
     '"total":1284.95,"currency":"EUR"}'),
    ("JSON with a digest", "text",
     '{"id":184223,"sha":'
     '"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"}'),
    ("log line", "text",
     "2026-08-10T07:12:44Z INFO  order.service  order=184223 "
     "user=ada status=shipped duration_ms=41"),
    ("SQL statement", "text",
     "SELECT id, name, total FROM orders WHERE customer_id = 184223 "
     "AND status = 'shipped' ORDER BY created_at DESC LIMIT 50"),
    ("HTTP request head", "text",
     "GET /v2/orders/184223 HTTP/1.1\r\n"
     "Host: api.example.com\r\n"
     "Accept: application/json\r\n"
     "User-Agent: acme-client/3.1.0\r\n\r\n"),
]

# Raw bytes rather than text: the case where nothing but the block coder
# applies, and the one where a short payload's rounding shows most.
BINARY_SAMPLES: list[tuple[str, str, bytes]] = [
    ("random, 8 bytes", "binary", bytes.fromhex("3f5a9c2e70b1d846")),
    ("random, 16 bytes", "binary",
     bytes.fromhex("8e1d4a7b2c9f06355ad3e8901b6c4f27")),
    ("random, 32 bytes", "binary",
     bytes.fromhex("c4a1908f7e6d5b3a2918074635f2e1d0"
                   "b9a8978665544332211f0e0d0c0b0a09")),
    # Sixty-four bytes with no structure in them. It said `"5c3e..." * 8` for
    # a while, which is an eight-byte cycle and compresses to nothing -- the
    # opposite of what a sample called random is here to test.
    ("random, 64 bytes", "binary",
     bytes.fromhex("2a5852cdeae1eeea827c60bdfb6f6ec5"
                   "2e2ce59ac5b2c0dd58ad5bc4bf6d8716"
                   "1ec8da5e6423dfd491b44ee3afe667b0"
                   "e3b41ed45b9c6f4b33d11704a65109d5")),
    ("zero run, 32 bytes", "binary", bytes(32)),
    ("zero-padded record", "binary",
     b"ORD-184223" + bytes(22) + b"shipped" + bytes(25)),
]


def samples() -> list[tuple[str, str, bytes]]:
    """Every sample as (label, category, bytes)."""
    out = [(label, cat, value.encode("utf-8")) for label, cat, value in WIRE_SAMPLES]
    out += [(label, cat, value) for label, cat, value in BINARY_SAMPLES]
    return out


def slug(label: str) -> str:
    keep = [c if c.isalnum() else "-" for c in label.lower()]
    name = "".join(keep)
    while "--" in name:
        name = name.replace("--", "-")
    return name.strip("-")


def write_into(directory) -> list[tuple[str, str, int]]:
    """Materialise every sample as its own file, so that the Go reference
    harness and the Rust prototype read them the same way the other groups
    are read. Returns (file name, category, size)."""
    from pathlib import Path

    d = Path(directory)
    d.mkdir(parents=True, exist_ok=True)
    written = []
    for i, (label, cat, data) in enumerate(samples()):
        name = f"{i:02d}-{cat}-{slug(label)}"
        (d / name).write_bytes(data)
        written.append((name, cat, len(data)))
    return written


if __name__ == "__main__":
    for label, cat, data in samples():
        print(f"{len(data):>5}  {cat:<7} {label}")
