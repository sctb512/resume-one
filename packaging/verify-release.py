"""Verify the native bundle layout and match its themes to the selected source."""

import argparse
from pathlib import Path, PurePosixPath
import struct
import zipfile


def verify_binary(binary, name, macos):
    header = binary.read(64)
    if macos:
        if header[:8] != bytes.fromhex("cffaedfe0c000001"):
            raise ValueError(f"{name} is not an ARM64 Mach-O executable")
        return
    if len(header) != 64 or header[:2] != b"MZ":
        raise ValueError(f"{name} is not a Windows executable")
    pe_offset = struct.unpack_from("<I", header, 0x3C)[0]
    if not 64 <= pe_offset <= 1024 * 1024:
        raise ValueError(f"{name} has an invalid PE header")
    binary.seek(pe_offset)
    if binary.read(6) != b"PE\0\0\x64\x86":
        raise ValueError(f"{name} is not a Windows x64 executable")


def verify(archive_path, themes, release_tag=None):
    if archive_path.name not in {"Resume-One-macOS-arm64-unsigned.zip", "Resume-One-Windows-x64-unsigned.zip"}:
        raise ValueError("Unrecognized release archive name")
    macos = "macOS-arm64" in archive_path.name
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
        for name in ("resume-one-app", "resume-one", "resume-one-theme-migrate"):
            with archive.open(f"{executables}/{name}{suffix}") as binary:
                verify_binary(binary, name, macos)
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
        expected = {}
        for package in sorted(themes.glob("community-*")):
            if not (package / "theme.json").is_file():
                raise ValueError(f"{package.name} is missing theme.json")
            for file in package.iterdir():
                expected[f"{theme_root}{package.name}/{file.name}"] = file
        bundled = {name for name in entries if name.startswith(f"{theme_root}community-")}
        if bundled != expected.keys():
            raise ValueError("Community theme archive paths differ from the pinned checkout")
        for name, file in expected.items():
            if archive.read(name) != file.read_bytes():
                raise ValueError(f"Theme file mismatch: {name}")
        count = sum(file.name == "theme.json" for file in expected.values())
        if count == 0:
            raise ValueError("The selected community checkout contains no packages")
        print(f"Verified {archive_path.name}, 3 executables, 20 native themes and {count} community packages.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("themes", type=Path)
    parser.add_argument("--release-tag")
    args = parser.parse_args()
    verify(args.archive, args.themes, args.release_tag)


if __name__ == "__main__":
    main()
