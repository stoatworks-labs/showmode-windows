#!/bin/bash
# Packages a release from the Mac: builds both executables with scripts/build.sh, then writes
# dist/ShowMode-<version>-windows-x64.zip and dist/ShowMode-<version>-windows-arm64.zip, each
# holding ShowMode.exe, LICENSE.txt and THIRD-PARTY-NOTICES.txt.
#
# The executable inside is always ShowMode.exe: the showmode: handler is only taken back
# when its command names a ShowMode.exe (integration.rs), so a download renamed with its
# version would never update its own registration.
#
# Upload by hand afterwards (gh release create/upload). The builds are unsigned until there's
# a code-signing certificate.
set -euo pipefail
cd "$(dirname "$0")/.."

python3 scripts/third-party-notices.py --check
scripts/build.sh

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
for arch in x64 arm64; do
    stage="dist/.stage-$arch"
    zip="dist/ShowMode-$version-windows-$arch.zip"
    rm -rf "$stage" "$zip"
    mkdir -p "$stage"
    cp "dist/ShowMode-$version-$arch.exe" "$stage/ShowMode.exe"
    cp LICENSE "$stage/LICENSE.txt"
    cp THIRD-PARTY-NOTICES.txt "$stage/"
    (cd "$stage" && zip -qX "../$(basename "$zip")" ShowMode.exe LICENSE.txt THIRD-PARTY-NOTICES.txt)
    rm -rf "$stage"
    echo "wrote $zip"
done
