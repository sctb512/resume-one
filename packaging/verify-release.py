"""Verify the native bundle, shared fonts, and bundled themes."""

import argparse
from pathlib import Path, PurePosixPath
import struct
import zipfile

FONT_NAMES = ("NotoSansSC-Regular.ttf", "NotoSerifSC-SemiBold.ttf")
FONT_SOURCE = Path(__file__).resolve().parents[1] / "assets" / "fonts"
ARCHIVES = {
    "Resume-One-macOS-arm64-unsigned.zip": ("macOS", 0x0100000C),
    "Resume-One-macOS-x64-unsigned.zip": ("macOS", 0x01000007),
    "Resume-One-Windows-arm64-unsigned.zip": ("Windows", 0xAA64),
    "Resume-One-Windows-x64-unsigned.zip": ("Windows", 0x8664),
}


def verify_binary(binary, name, macos, machine):
    header = binary.read(64)
    if macos:
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
        font_paths = {f"{resources}/assets/fonts/{name}" for name in FONT_NAMES}
        if {name for name in entries if name.startswith(f"{root}/") and
                PurePosixPath(name).suffix.lower() in {".ttf", ".otf"}} != font_paths:
            raise ValueError("The bundle must contain exactly one shared copy of each fallback font")
        fonts = {}
        for name in FONT_NAMES:
            data = (FONT_SOURCE / name).read_bytes()
            if archive.read(f"{resources}/assets/fonts/{name}") != data:
                raise ValueError(f"Shared font contents differ from the source: {name}")
            fonts[name] = data
        for name in ("resume-one-app", "resume-one", "resume-one-theme-migrate"):
            member = f"{executables}/{name}{suffix}"
            with archive.open(member) as binary:
                verify_binary(binary, name, macos, machine)
            executable = archive.read(member)
            for font_name, data in fonts.items():
                if data in executable:
                    raise ValueError(f"{name} embeds a duplicate of {font_name}")
        for name in ("README.md", "docs/user-manual.md", "docs/cli-manual.md", "resume-one-theme-spec-v2.md"):
            archive.getinfo(f"{resources}/{name}")

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
        if {name for name in entries if name.startswith(theme_root)} != set(native):
            raise ValueError("Only native themes may be included in the release archive")
        print(f"Verified {archive_path.name}, 3 executables, 2 shared fonts and 20 native themes.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--release-tag")
    args = parser.parse_args()
    verify(args.archive, args.release_tag)


if __name__ == "__main__":
    main()
