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
    cargo build --release -p resume-one-app -p resume-one-cli -p resume-one-theme-migrate
else
    cargo build -p resume-one-app -p resume-one-cli -p resume-one-theme-migrate
fi

bundle="$repo_root/target/$profile/Resume One.app"
mkdir -p "$bundle/Contents/MacOS"
cp "$repo_root/packaging/macos/Info.plist" "$bundle/Contents/Info.plist"
cp "$repo_root/target/$profile/resume-one-app" "$bundle/Contents/MacOS/resume-one-app"
cp "$repo_root/target/$profile/resume-one" "$bundle/Contents/MacOS/resume-one"
cp "$repo_root/target/$profile/resume-one-theme-migrate" "$bundle/Contents/MacOS/resume-one-theme-migrate"
plutil -lint -s "$bundle/Contents/Info.plist"
"$bundle/Contents/MacOS/resume-one" --version >/dev/null
"$bundle/Contents/MacOS/resume-one-theme-migrate" --version >/dev/null
echo "$bundle"
