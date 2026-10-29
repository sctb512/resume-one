#!/usr/bin/env python3
"""Convert bounded, embedded static WOFF2 faces as data, never execute theme JS.

Requires FontTools with WOFF support. The caller must first review modification
rights and reserved font names; a valid alias is not legal clearance. The source
copyright and complete license must accompany the output. Static aliases change
only naming metadata and the SFNT checksum, never glyphs, metrics or permissions.
"""
import argparse
import base64
import hashlib
import io
import json
import re
import struct
from pathlib import Path
from fontTools.ttLib import TTFont


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source_group = parser.add_mutually_exclusive_group(required=True)
    source_group.add_argument("--input", type=Path, help="bounded source with embedded WOFF2 declarations")
    source_group.add_argument("--static-font", type=Path, help="licensed static TrueType face to give a distinct per-face family")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--family", required=True)
    args = parser.parse_args()
    source_path = args.input or args.static_font
    if not source_path.is_absolute() or not args.output.is_absolute():
        parser.error("paths must be absolute")
    if not re.fullmatch(r"Resume One [A-Za-z0-9 ]{1,50}", args.family):
        parser.error("use a new Resume One family name, not an upstream reserved name")
    if "source" in args.family.lower().split():
        parser.error("Source is an upstream reserved font name")
    if args.output.exists():
        parser.error("output directory must not already exist")
    source_limit = (8 if args.static_font else 4) * 1024 * 1024
    if source_path.stat().st_size > source_limit:
        parser.error("source exceeds size limit")
    source_bytes = source_path.read_bytes()
    if args.static_font:
        if source_bytes[:4] != b"\x00\x01\x00\x00":
            parser.error("static alias input must be a standalone TrueType SFNT; CFF and collections need separate conversion")
        probe = TTFont(io.BytesIO(source_bytes), recalcTimestamp=False)
        if "fvar" in probe or probe["OS/2"].usWeightClass not in range(100, 901, 100):
            parser.error("static input must have a supported actual weight")
        weight = str(probe["OS/2"].usWeightClass)
        style = "italic" if probe["OS/2"].fsSelection & 1 else "normal"
        declarations = [(weight, style, source_bytes)]
    else:
        source = source_bytes.decode("utf8")
        declarations = re.findall(r"\{\s*weight:\s*(400|500|600|700),\s*style:\s*'(normal|italic)',\s*data:\s*'([A-Za-z0-9+/=]+)'\s*\}", source)
        if not 1 <= len(declarations) <= 8:
            parser.error("expected one to eight explicit static embedded faces")
    output = []
    for weight, style, data in declarations:
        decoded = data if args.static_font else base64.b64decode(data, validate=True)
        if len(decoded) > 8 * 1024 * 1024 or (not args.static_font and decoded[:4] != b"wOF2"):
            parser.error("invalid or oversized WOFF2 face")
        if not args.static_font and (len(decoded) < 48 or struct.unpack_from(">I", decoded, 16)[0] > 8 * 1024 * 1024):
            parser.error("decoded WOFF2 size exceeds 8 MiB before parsing")
        font = TTFont(io.BytesIO(decoded), recalcTimestamp=False)
        if "CFF " in font or "CFF2" in font:
            parser.error("CFF program names need a separately reviewed conversion; not silently renamed")
        if "fvar" in font or font["OS/2"].usWeightClass != int(weight):
            parser.error("only static faces matching the declared weight are accepted")
        family = args.family + (" Italic" if style == "italic" else "")
        subfamily = {"100":"Thin", "200":"ExtraLight", "300":"Light", "400": "Regular", "500": "Medium", "600": "SemiBold", "700": "Bold", "800":"ExtraBold", "900":"Black"}[weight]
        full = family + " " + subfamily
        postscript = full.replace(" ", "")
        if len(postscript) > 63:
            parser.error("PostScript name exceeds 63 ASCII bytes; choose a shorter unique family")
        names = {1: family, 2: subfamily, 3: postscript, 4: full, 6: postscript, 16: family, 17: subfamily, 18: full, 21: family, 22: subfamily, 25: postscript}
        for record in font["name"].names:
            value = names.get(record.nameID)
            if value is not None:
                record.string = value.encode(record.getEncoding())
        for name_id in (1,2,3,4,6,16,17):
            font["name"].setName(names[name_id],name_id,3,1,0x409)
        font.flavor = None
        buffer = io.BytesIO()
        font.save(buffer)
        content = buffer.getvalue()
        if len(content) > 8 * 1024 * 1024:
            parser.error("decoded static face exceeds theme limit")
        filename = f"{style}-{weight}.ttf"
        if any(item["path"] == filename for item, _ in output):
            parser.error("duplicate face")
        output.append(({"path": filename, "family": family, "weight": int(weight), "style": style,
                        "sha256": hashlib.sha256(content).hexdigest()}, content))
    args.output.mkdir(parents=True)
    for item, content in output:
        with (args.output / item["path"]).open("xb") as handle:
            handle.write(content)
    print(json.dumps({"sourceSha256": hashlib.sha256(source_bytes).hexdigest(),
                      "faces": [item for item, _ in output]}, indent=2))


if __name__ == "__main__":
    main()
