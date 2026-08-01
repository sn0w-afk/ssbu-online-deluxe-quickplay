#!/usr/bin/env bash
# Build libssbu_online_deluxe.nro without cargo-skyline.
#
# Prerequisites:
#   - A rustup toolchain named "skyline-v3" (base nightly + skyline-rs custom
#     std, i.e. what `cargo skyline update-std` sets up), with the target json
#     at ~/.cargo/skyline/aarch64-skyline-switch.json
#   - The elf2nro helper (built once from ./elf2nro)
set -e

export SKYLINE_ADD_NRO_HEADER=1
export RUSTFLAGS="--cfg skyline_std_v3"

cd "$(dirname "$0")"

if [ ! -f ./elf2nro/target/release/elf2nro.exe ]; then
    (cd elf2nro && cargo build --release)
fi

rustup run skyline-v3 cargo build --release \
  -Z json-target-spec \
  --target "$HOME/.cargo/skyline/aarch64-skyline-switch.json" \
  -Z build-std=core,alloc,std,panic_abort

./elf2nro/target/release/elf2nro.exe \
  target/aarch64-skyline-switch/release/libssbu_online_deluxe.so \
  target/aarch64-skyline-switch/release/libssbu_online_deluxe.nro

echo "Done: target/aarch64-skyline-switch/release/libssbu_online_deluxe.nro"
