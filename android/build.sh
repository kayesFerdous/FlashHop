#!/bin/sh
# Builds android/akhri_pasta.apk without Gradle, in seconds:
#   cargo (shared Rust core) -> kotlinc -> R8 -> aapt2 -> zipalign -> apksigner
# Needs: Android SDK (+NDK), a JDK, and the Rust Android toolchain + kotlinc from
# ./setup-tools.sh (kept in ~/.local/share/akhri-android, separate from your normal Rust).
set -eu
cd "$(dirname "$0")"

SDK=${ANDROID_HOME:-$HOME/Android/Sdk}
NDK=$(ls -d "$(ls -d "$SDK"/ndk/* | sort -V | tail -1)"/toolchains/llvm/prebuilt/*/bin | head -1)
BT=$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)
JAR=$SDK/platforms/android-36/android.jar
TOOLS=${AKHRI_TOOLS:-$HOME/.local/share/akhri-android}
TC=$(ls -d "$TOOLS"/rustup/toolchains/*/ | head -1)
OUT=build
rm -rf $OUT && mkdir -p $OUT/apk/lib

# 1. Shared Rust core for phones (arm64) and the emulator (x86_64).
for pair in aarch64-linux-android:arm64-v8a x86_64-linux-android:x86_64; do
  t=${pair%%:*} abi=${pair##*:}
  T=$(echo "$t" | tr 'a-z-' 'A-Z_')
  env RUSTC="$TC/bin/rustc" "CARGO_TARGET_${T}_LINKER=$NDK/${t}30-clang" \
    "$TC/bin/cargo" build -q -p akhri_android --release --target "$t"
  mkdir -p $OUT/apk/lib/$abi && cp ../target/$t/release/libakhri.so $OUT/apk/lib/$abi/
done

# 2. Kotlin -> classes -> one dex. R8 shrinks the Kotlin stdlib to what's actually used.
"$TOOLS/kotlinc/bin/kotlinc" src -cp "$JAR" -no-jdk -no-reflect -jvm-target 17 -nowarn -d $OUT/classes.jar
"$BT/aapt2" link -o $OUT/app.apk -I "$JAR" --manifest AndroidManifest.xml --proguard $OUT/aapt-rules.pro
java -cp "$BT/lib/d8.jar" com.android.tools.r8.R8 --release --min-api 30 --lib "$JAR" \
  --pg-conf rules.pro --pg-conf $OUT/aapt-rules.pro --output $OUT/apk \
  $OUT/classes.jar "$TOOLS/kotlinc/lib/kotlin-stdlib.jar"

# 3. Package. Native libs go in uncompressed and 16 KB aligned so Android maps them in place.
jar -u -M -f $OUT/app.apk -C $OUT/apk classes.dex
jar -u -0 -M -f $OUT/app.apk -C $OUT/apk lib
"$BT/zipalign" -f -P 16 4 $OUT/app.apk $OUT/aligned.apk
# Signing: a release key from AKHRI_KEYSTORE/_PASS/_KEY_ALIAS (CI secrets), else the standard debug key.
KS=${AKHRI_KEYSTORE:-$HOME/.android/debug.keystore}
PASS=${AKHRI_KEYSTORE_PASS:-android}
ALIAS=${AKHRI_KEY_ALIAS:-androiddebugkey}
if [ ! -f "$KS" ]; then
  mkdir -p "$(dirname "$KS")"
  keytool -genkeypair -keystore "$KS" -storepass android -keypass android -alias androiddebugkey \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US" >/dev/null 2>&1
fi
"$BT/apksigner" sign --ks "$KS" --ks-pass "pass:$PASS" \
  --ks-key-alias "$ALIAS" --key-pass "pass:$PASS" --out akhri_pasta.apk $OUT/aligned.apk
ls -l akhri_pasta.apk
