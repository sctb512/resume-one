#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
profile="${1:-debug}"
case "$profile" in
    debug|release) ;;
    *) echo "usage: $0 [debug|release]" >&2; exit 2 ;;
esac

cd "$repo_root"
if [[ "$profile" == release ]]; then
    cargo build --release -p resume-one-app
else
    cargo build -p resume-one-app
fi

bundle="$repo_root/target/$profile/Resume One.app"
mkdir -p "$bundle/Contents/MacOS"
cp "$repo_root/packaging/macos/Info.plist" "$bundle/Contents/Info.plist"
cp "$repo_root/target/$profile/resume-one-app" "$bundle/Contents/MacOS/resume-one-app"
echo "$bundle"
