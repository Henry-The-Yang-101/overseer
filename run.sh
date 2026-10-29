#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if command -v cargo >/dev/null 2>&1; then
    cargo_bin=cargo
elif [ -x "$HOME/.cargo/bin/cargo" ]; then
    cargo_bin="$HOME/.cargo/bin/cargo"
else
    echo 'Rust 1.95+ is required. Install it from https://rustup.rs.' >&2
    exit 1
fi
cd "$root"
exec "$cargo_bin" run --quiet --release -- "$@"
