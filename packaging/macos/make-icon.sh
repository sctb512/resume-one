#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
iconset="$(mktemp -d /tmp/resume-one-icon.XXXXXX)/ResumeOne.iconset"
trap 'rm -r "$(dirname "$iconset")"' EXIT
mkdir "$iconset"

base="$(dirname "$iconset")/base.png"
swift "$repo_root/packaging/macos/render-icon.swift" \
    "$repo_root/assets/fonts/NotoSerifSC-SemiBold.ttf" "$base"

for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$base" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$base" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns --output "$repo_root/assets/app-icon.icns" "$iconset"
