#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
profile="${1:-release}"
case "$profile" in
    debug|release) ;;
    *) echo "usage: $0 [debug|release] [absolute-output.zip] [absolute-input.app]" >&2; exit 2 ;;
esac

bundle="${3:-$repo_root/target/$profile/Resume One.app}"
output="${2:-$repo_root/target/$profile/Resume-One-macOS-unsigned.zip}"
if [[ "$bundle" != /* || "$bundle" != *.app || ! -d "$bundle" || -L "$bundle" ]]; then
    echo "missing app bundle: $bundle; run packaging/macos/bundle.sh first" >&2
    exit 3
fi
if [[ "$output" != /* || -e "$output" || -L "$output" || ! -d "$(dirname "$output")" ]]; then
    echo "output must be a new absolute ZIP path in an existing directory" >&2
    exit 2
fi

codesign --verify --deep --strict --verbose=2 "$bundle"
archive_dir="$(mktemp -d "$(dirname "$output")/.resume-one-archive.XXXXXX")"
trap 'rm -r "$archive_dir"' EXIT
archive="$archive_dir/Resume-One-macOS-unsigned.zip"
ditto -c -k --sequesterRsrc --keepParent "$bundle" "$archive"
unzip -tq "$archive" >/dev/null
mkdir "$archive_dir/unpacked"
ditto -x -k "$archive" "$archive_dir/unpacked"
codesign --verify --deep --strict --verbose=2 "$archive_dir/unpacked/Resume One.app"
mv "$archive" "$output"
echo "$output"
