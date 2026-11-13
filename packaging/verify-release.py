"""Verify the native bundle, shared fonts, and bundled themes."""

import argparse
from compression import zstd
from io import BytesIO
from pathlib import Path, PurePosixPath
import struct
import zipfile

from bundle_fonts import FONT_NAMES, FONT_SOURCE

ROOT = Path(__file__).resolve().parents[1]
ASSET_SOURCE = ROOT / "assets"
ARCHIVES = {
    "Resume-One-macOS-arm64-unsigned.zip": ("macOS", 0x0100000C),
    "Resume-One-macOS-x64-unsigned.zip": ("macOS", 0x01000007),
    "Resume-One-macOS-universal-unsigned.zip": ("macOS", None),
    "Resume-One-Windows-arm64-unsigned.zip": ("Windows", 0xAA64),
    "Resume-One-Windows-x64-unsigned.zip": ("Windows", 0x8664),
}


def verify_binary(binary, name, macos, machine):
    header = binary.read(64)
    if macos:
        if machine is None:
            if header[:8] != struct.pack(">II", 0xCAFEBABE, 2):
                raise ValueError(f"{name} is not a two-slice Mach-O")
            machines = set()
            for index in range(2):
                cpu, _, offset, size, _ = struct.unpack_from(">IIIII", header, 8 + index * 20)
                if offset < 48 or size < 32:
                    raise ValueError(f"{name} has an invalid Mach-O slice")
                binary.seek(offset)
                if binary.read(8) != struct.pack("<II", 0xFEEDFACF, cpu):
                    raise ValueError(f"{name} has a malformed Mach-O slice")
                machines.add(cpu)
            if machines != {0x0100000C, 0x01000007}:
                raise ValueError(f"{name} is missing a required architecture")
            return
        if header[:8] != struct.pack("<II", 0xFEEDFACF, machine):
            raise ValueError(f"{name} does not have the expected Mach-O architecture")
        return
    if len(header) != 64 or header[:2] != b"MZ":
        raise ValueError(f"{name} is not a Windows executable")
    pe_offset = struct.unpack_from("<I", header, 0x3C)[0]
    if not 64 <= pe_offset <= 1024 * 1024:
        raise ValueError(f"{name} has an invalid PE header")
    binary.seek(pe_offset)
    if binary.read(6) != b"PE\0\0" + struct.pack("<H", machine):
        raise ValueError(f"{name} does not have the expected PE architecture")


def verify(archive_path, release_tag=None):
    if archive_path.name not in ARCHIVES:
        raise ValueError("Unrecognized release archive name")
    os_name, machine = ARCHIVES[archive_path.name]
    macos = os_name == "macOS"
    root = "Resume One.app/Contents" if macos else "Resume One"
    resources = f"{root}/Resources" if macos else root
    executables = f"{root}/MacOS" if macos else root
    suffix = "" if macos else ".exe"
    with zipfile.ZipFile(archive_path) as archive:
        if corrupt := archive.testzip():
            raise ValueError(f"CRC check failed for {corrupt}")
        entries = [entry for entry in archive.namelist() if not entry.endswith("/")]
        if len(entries) != len(set(entries)):
            raise ValueError("The bundle has duplicate filenames")
        for name in entries:
            path = PurePosixPath(name)
            if path.is_absolute() or ".." in path.parts or "\\" in name:
                raise ValueError(f"Invalid archive path {name}")
        font_paths = {f"{resources}/assets/fonts/{name}.zst" for name in FONT_NAMES}
        if {name for name in entries if name.startswith(f"{root}/") and
                PurePosixPath(name).suffix.lower() in {".ttf", ".otf", ".woff", ".woff2", ".zst"}} != font_paths:
            raise ValueError("The bundle must contain exactly one shared copy of each fallback font")
        fonts = {}
        for name in FONT_NAMES:
            data = (FONT_SOURCE / name).read_bytes()
            with archive.open(f"{resources}/assets/fonts/{name}.zst") as file:
                compressed = file.read(len(data))
            if len(compressed) >= len(data):
                raise ValueError(f"Shared font exceeds its compressed size limit: {name}")
            with zstd.open(BytesIO(compressed), "rb") as file:
                restored = file.read(len(data) + 1)
            if restored != data:
                raise ValueError(f"Shared font contents differ from the source: {name}")
            fonts[name] = (data, compressed)
        for name in ("resume-one-app", "resume-one", "resume-one-theme-migrate"):
            member = f"{executables}/{name}{suffix}"
            with archive.open(member) as binary:
                verify_binary(binary, name, macos, machine)
            executable = archive.read(member)
            for font_name, versions in fonts.items():
                if any(data in executable for data in versions):
                    raise ValueError(f"{name} embeds a duplicate of {font_name}")
        for name in ("README.md", "README.zh-CN.md", "LICENSE", "licenses/LICENSE-BOOST-1.0.txt",
                     "vendor/ruzstd/LICENSE-MIT"):
            archive.getinfo(f"{resources}/{name}")
        document_stems = (
            "docs/user-manual",
            "docs/cli-manual",
            "licenses/THIRD_PARTY_LICENSES",
            "spec/resume-one-theme-spec-v2",
        )
        for stem in document_stems:
            for suffix in (".md", ".zh-CN.md"):
                name = f"{stem}{suffix}"
                if archive.read(f"{resources}/{name}") != (ROOT / name).read_bytes():
                    raise ValueError(f"The bundled document differs from the source: {name}")
        spec_root = f"{resources}/spec/"
        expected_specs = {
            path.relative_to(ROOT).as_posix(): path
            for path in (ROOT / "spec").rglob("*")
            if path.is_file()
        }
        if {name for name in entries if name.startswith(spec_root)} != {
            f"{resources}/{name}" for name in expected_specs
        } or f"{resources}/resume-one-theme-spec-v2.md" in entries:
            raise ValueError("The bundle must use the repository's specification layout")
        for name, path in expected_specs.items():
            if archive.read(f"{resources}/{name}") != path.read_bytes():
                raise ValueError(f"The bundled specification resource differs from the source: {name}")
        for name in ("example.resume.json", "example.en.resume.json"):
            if archive.read(f"{resources}/assets/{name}") != (ASSET_SOURCE / name).read_bytes():
                raise ValueError(f"The bundled résumé sample differs from the source: {name}")

        if release_tag is not None:
            marker = archive.read(f"{resources}/release-tag.txt").decode("ascii").strip()
            if marker != release_tag:
                raise ValueError(f"Expected packaged tag {release_tag}, got {marker}")

        theme_root = f"{resources}/assets/themes/"
        native = [name for name in entries if name.startswith(theme_root)
                  and name.removeprefix(theme_root).endswith(".theme.json")
                  and "/" not in name.removeprefix(theme_root)]
        if len(native) != 20:
            raise ValueError(f"Expected 20 native themes, got {len(native)}")
        translations = {name.removesuffix(".theme.json") + ".localizations.txt" for name in native}
        if {name for name in entries if name.startswith(theme_root)} != set(native) | translations:
            raise ValueError("The release must contain only native themes and their translations")
        print(f"Verified {archive_path.name} ({archive_path.stat().st_size / 2**20:.2f} MiB), "
              "3 executables, 2 losslessly compressed shared fonts and 20 native themes.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--release-tag")
    args = parser.parse_args()
    verify(args.archive, args.release_tag)


if __name__ == "__main__":
    main()
