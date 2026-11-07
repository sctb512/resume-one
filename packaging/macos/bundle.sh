#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
profile="${1:-debug}"
case "$profile" in
    debug|release) ;;
    *) echo "usage: $0 [debug|release] [new-absolute-output.app]" >&2; exit 2 ;;
esac

bundle="${2:-$repo_root/target/$profile/Resume One.app}"
if [[ -n "${2:-}" && ( "$bundle" != /* || "$bundle" != *.app || -e "$bundle" || -L "$bundle" || ! -d "$(dirname "$bundle")" ) ]]; then
    echo "explicit output must be a new absolute .app path in an existing directory" >&2
    exit 2
fi

cd "$repo_root"
if [[ "$profile" == release ]]; then
    cargo build --locked --release -p resume-one-app -p resume-one-cli -p resume-one-theme-migrate
else
    cargo build --locked -p resume-one-app -p resume-one-cli -p resume-one-theme-migrate
fi

mkdir -p "$bundle/Contents/MacOS"
mkdir -p "$bundle/Contents/Resources"
mkdir -p "$bundle/Contents/Resources/docs" "$bundle/Contents/Resources/spec" "$bundle/Contents/Resources/assets/themes" "$bundle/Contents/Resources/assets/fonts" "$bundle/Contents/Resources/vendor/gpui-component"
cp "$repo_root/packaging/macos/Info.plist" "$bundle/Contents/Info.plist"
cp "$repo_root/assets/app-icon.icns" "$bundle/Contents/Resources/app-icon.icns"
cp "$repo_root/README.md" "$repo_root/assets/example.resume.json" "$bundle/Contents/Resources/"
cp "$repo_root/resume-one-theme-spec-v2.md" "$bundle/Contents/Resources/"
cp "$repo_root/assets/example.resume.json" "$bundle/Contents/Resources/assets/"
cp "$repo_root/vendor/gpui-component/LICENSE-APACHE" "$bundle/Contents/Resources/vendor/gpui-component/"
cp "$repo_root/assets/fonts/"OFL-*.txt "$bundle/Contents/Resources/assets/fonts/"
cp "$repo_root/docs/user-manual.md" "$repo_root/docs/cli-manual.md" "$bundle/Contents/Resources/docs/"
cp "$repo_root/resume-one-theme-spec-v2.md" "$bundle/Contents/Resources/spec/"
cp "$repo_root"/spec/* "$bundle/Contents/Resources/spec/"
cp "$repo_root"/assets/themes/*.theme.json "$bundle/Contents/Resources/assets/themes/"
# Copy the same optional package set embedded by the build.
if [[ -n "${RESUME_ONE_COMMUNITY_THEMES_DIR:-}" ]]; then
    for manifest in "$RESUME_ONE_COMMUNITY_THEMES_DIR"/community-*/theme.json; do
        test -f "$manifest"
        cp -R "$(dirname "$manifest")" "$bundle/Contents/Resources/assets/themes/"
    done
fi
cp "$repo_root/target/$profile/resume-one-app" "$bundle/Contents/MacOS/resume-one-app"
cp "$repo_root/target/$profile/resume-one" "$bundle/Contents/MacOS/resume-one"
cp "$repo_root/target/$profile/resume-one-theme-migrate" "$bundle/Contents/MacOS/resume-one-theme-migrate"
plutil -lint -s "$bundle/Contents/Info.plist"
for name in resume-one resume-one-theme-migrate; do
    codesign --force --sign - "$bundle/Contents/MacOS/$name"
done
codesign --force --sign - "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"
"$bundle/Contents/MacOS/resume-one" --version >/dev/null
"$bundle/Contents/MacOS/resume-one-theme-migrate" --version >/dev/null
echo "$bundle"
