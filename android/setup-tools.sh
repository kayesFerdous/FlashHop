#!/bin/sh
# One-time: installs the Rust Android toolchain and kotlinc into ~/.local/share/akhri-android.
# Separate RUSTUP_HOME/CARGO_HOME, nothing added to PATH: your normal Rust setup is untouched.
set -eu
TOOLS=${AKHRI_TOOLS:-$HOME/.local/share/akhri-android}
RUST=1.100.0 # keep equal to the desktop rustc so both apps build from the same compiler
KOTLIN=2.4.21
mkdir -p "$TOOLS" && cd "$TOOLS"

export RUSTUP_HOME=$TOOLS/rustup CARGO_HOME=$TOOLS/cargo
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y -q --no-modify-path \
  --profile minimal --default-toolchain $RUST --target aarch64-linux-android --target x86_64-linux-android

curl -sSL "https://github.com/JetBrains/kotlin/releases/download/v$KOTLIN/kotlin-compiler-$KOTLIN.zip" -o kotlinc.zip
rm -rf kotlinc && unzip -q kotlinc.zip && rm kotlinc.zip
echo "Tools ready in $TOOLS"
