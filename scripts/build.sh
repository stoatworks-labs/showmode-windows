#!/bin/bash
# Builds Show Mode for Windows from the Mac: x64 and ARM64 release executables, cross-compiled
# with cargo-xwin (which fetches the Windows SDK and CRT on first use). Writes
# dist/ShowMode-<version>-x64.exe and dist/ShowMode-<version>-arm64.exe.
#
#   scripts/build.sh            both
#   scripts/build.sh x64        one
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
targets=("x64:x86_64-pc-windows-msvc" "arm64:aarch64-pc-windows-msvc")
want=${1:-all}

mkdir -p dist
for pair in "${targets[@]}"; do
    arch=${pair%%:*}
    triple=${pair#*:}
    [[ $want == all || $want == "$arch" ]] || continue
    cargo xwin build --release --target "$triple"
    cp "target/$triple/release/ShowMode.exe" "dist/ShowMode-$version-$arch.exe"
    echo "wrote dist/ShowMode-$version-$arch.exe"
done
