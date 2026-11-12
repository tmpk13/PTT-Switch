#!/usr/bin/env bash
# Builds the phone app as a debug-signed APK, android/build/ptt-switch.apk, with the Android
# SDK's command-line tools (aapt2, javac, d8, apksigner) and no Gradle.
set -euo pipefail
cd "$(dirname "$0")"

SDK=${ANDROID_HOME:-$HOME/Android}
TOOLS=$SDK/build-tools/34.0.0
PLATFORM=$SDK/platforms/android-33/android.jar
OUT=build

for f in "$TOOLS/aapt2" "$TOOLS/d8" "$PLATFORM"; do
    [ -e "$f" ] || { echo "missing $f (set ANDROID_HOME; needs build-tools 34.0.0, android-33)" >&2; exit 1; }
done

rm -rf "$OUT/gen" "$OUT/classes" "$OUT/dex"
mkdir -p "$OUT/gen" "$OUT/classes" "$OUT/dex"

# Resources, and R.java with their IDs for the Java sources.
"$TOOLS/aapt2" compile --dir res -o "$OUT/res.zip"
"$TOOLS/aapt2" link -I "$PLATFORM" --manifest AndroidManifest.xml --java "$OUT/gen" \
    --min-sdk-version 26 --target-sdk-version 33 --version-code 1 --version-name 0.1 \
    --debug-mode -o "$OUT/base.apk" "$OUT/res.zip"

javac --release 8 -Xlint:-options -encoding UTF-8 -d "$OUT/classes" -cp "$PLATFORM" \
    $(find src "$OUT/gen" -name '*.java')
"$TOOLS/d8" --min-api 26 --lib "$PLATFORM" --output "$OUT/dex" \
    $(find "$OUT/classes" -name '*.class')

cp "$OUT/base.apk" "$OUT/unaligned.apk"
zip -qj "$OUT/unaligned.apk" "$OUT/dex/classes.dex"
"$TOOLS/zipalign" -f 4 "$OUT/unaligned.apk" "$OUT/aligned.apk"

KS=$HOME/.android/debug.keystore
if [ ! -f "$KS" ]; then
    mkdir -p "$(dirname "$KS")"
    keytool -genkeypair -keystore "$KS" -alias androiddebugkey -storepass android \
        -keypass android -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US"
fi
"$TOOLS/apksigner" sign --ks "$KS" --ks-pass pass:android --ks-key-alias androiddebugkey \
    --key-pass pass:android --out "$OUT/ptt-switch.apk" "$OUT/aligned.apk"
rm -f "$OUT/unaligned.apk" "$OUT/aligned.apk" "$OUT/ptt-switch.apk.idsig"
echo "built android/$OUT/ptt-switch.apk ($(du -k "$OUT/ptt-switch.apk" | cut -f1) KB)"
