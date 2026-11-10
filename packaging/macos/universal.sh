#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 || "$3" != /* ]]; then
    echo "usage: $0 arm64.zip x64.zip /absolute/output.zip" >&2
    exit 2
fi
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -r "$work"' EXIT
ditto -x -k "$1" "$work/arm"
ditto -x -k "$2" "$work/intel"
arm="$work/arm/Resume One.app"
intel="$work/intel/Resume One.app"
diff -qr "$arm/Contents/Resources" "$intel/Contents/Resources"
cmp "$arm/Contents/Info.plist" "$intel/Contents/Info.plist"
for name in resume-one-app resume-one resume-one-theme-migrate; do
    destination="$arm/Contents/MacOS/$name"
    xcrun lipo -create "$destination" "$intel/Contents/MacOS/$name" -output "$work/$name"
    xcrun lipo "$work/$name" -verify_arch arm64 x86_64
    mv "$work/$name" "$destination"
    codesign --force --sign - "$destination"
done
codesign --force --sign - "$arm"
bash "$repo_root/packaging/macos/archive.sh" release "$3" "$arm"
