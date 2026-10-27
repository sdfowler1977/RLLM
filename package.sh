#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")" && pwd)
: "${CLIENT_JDK:?Set CLIENT_JDK to a JDK 17 directory}"
: "${CLIENT_ANDROID:?Set CLIENT_ANDROID to the Android platform directory containing android.jar}"
: "${CLIENT_TOOLS:?Set CLIENT_TOOLS to Android build-tools 35.0.0}"
: "${CLIENT_NDK:?Set CLIENT_NDK to Android NDK r28c}"
: "${CLIENT_RUST:?Set CLIENT_RUST to Rust with Android standard-library targets installed}"
if [[ "${CLIENT_UNSIGNED:-0}" != 1 ]]; then
: "${CLIENT_KEYSTORE:?Set CLIENT_KEYSTORE to a private signing keystore}"
: "${CLIENT_KEY_ALIAS:?Set CLIENT_KEY_ALIAS}"
: "${CLIENT_KEY_PASSWORD:?Set CLIENT_KEY_PASSWORD}"
fi
export JAVA_HOME="$CLIENT_JDK"
export PATH="$CLIENT_JDK/bin:$PATH"
export LD_LIBRARY_PATH="$CLIENT_TOOLS/lib64:$CLIENT_RUST/lib:$CLIENT_JDK/lib:$CLIENT_JDK/lib/server:${LD_LIBRARY_PATH:-}"
out="$project_dir/output/$(date -u +%Y-%m-%d__%H-%M-%SZ)"
mkdir -p "$out/classes" "$out/dex" "$out/stage/lib/arm64-v8a" "$out/stage/lib/x86_64" "$out/gen"
mkdir -p "$out/tmp"
export TMPDIR="$out/tmp"
manifest="$project_dir/AndroidManifest.xml"
artifact="RLLM"
resource_package=()
case "${CLIENT_VARIANT:-main}" in
 main) ;;
 lab)
  : "${CLIENT_VERSION_CODE:?Set CLIENT_VERSION_CODE to an increasing Lab version code}"
  if [[ ! "$CLIENT_VERSION_CODE" =~ ^[1-9][0-9]{0,9}$ ]] || (( CLIENT_VERSION_CODE > 2100000000 )); then
   printf '%s\n' 'Version code must be 1..2100000000' >&2; exit 2
  fi
  # Original manifest and Java/JNI class names retain their original identity.
  sed -e 's/package="com.local.client"/package="com.local.client.lab"/' \
      -e 's/android:label="RLLM"/android:label="RLLM Lab"/' \
      -e 's/android:name="\./android:name="com.local.client./g' \
      -e 's/android:targetActivity="\./android:targetActivity="com.local.client./g' \
      -e 's/android:parentActivityName="\./android:parentActivityName="com.local.client./g' \
      -e "s/android:versionCode=\"[^\"]*\"/android:versionCode=\"$CLIENT_VERSION_CODE\"/" \
      -e "s/android:versionName=\"[^\"]*\"/android:versionName=\"0.1-lab.$CLIENT_VERSION_CODE\"/" \
      "$manifest" > "$out/AndroidManifest.xml"
  manifest="$out/AndroidManifest.xml"
  artifact="RLLM-Lab"
  resource_package=(--custom-package com.local.client)
  ;;
 *) printf '%s\n' 'Unknown CLIENT_VARIANT' >&2; exit 2 ;;
esac
ndk_bin="$CLIENT_NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
for item in 'aarch64-linux-android arm64-v8a' 'x86_64-linux-android x86_64'; do
 read -r target abi <<< "$item"
 "$CLIENT_RUST/bin/rustc" --edition=2021 --crate-name client --crate-type staticlib --target "$target" -C opt-level=2 -C codegen-units=1 -C panic=abort "$project_dir/core/lib.rs" -o "$out/lib-$abi.a"
 "$ndk_bin/${target}26-clang" -shared -fPIC -O2 "$project_dir/platform/jni/bridge.c" "$out/lib-$abi.a" -Wl,--exclude-libs,ALL -Wl,-z,max-page-size=16384 -Wl,-z,relro,-z,now -ldl -lm -llog -o "$out/stage/lib/$abi/libclient.so"
done
"$CLIENT_TOOLS/aapt2" compile --dir "$project_dir/res" -o "$out/resources.zip"
"$CLIENT_TOOLS/aapt2" link -o "$out/resources.apk" -I "$CLIENT_ANDROID/android.jar" --manifest "$manifest" "${resource_package[@]}" -A "$project_dir/notices" --java "$out/gen" "$out/resources.zip"
find "$project_dir/platform" "$out/gen" -name '*.java' -print > "$out/sources.txt"
"$CLIENT_JDK/bin/javac" --release 8 -cp "$CLIENT_ANDROID/android.jar" -d "$out/classes" @"$out/sources.txt"
"$CLIENT_JDK/bin/jar" cf "$out/classes.jar" -C "$out/classes" .
"$CLIENT_TOOLS/d8" --lib "$CLIENT_ANDROID/android.jar" --min-api 26 --output "$out/dex" "$out/classes.jar"
cp "$out/resources.apk" "$out/package.apk"
cp "$out/dex/classes.dex" "$out/stage/classes.dex"
# zip can succeed while skipping missing inputs; require each runtime payload.
for payload in classes.dex lib/arm64-v8a/libclient.so lib/x86_64/libclient.so; do
 test -s "$out/stage/$payload" || { printf 'Missing APK payload: %s\n' "$payload" >&2; exit 1; }
done
(cd "$out/stage" && zip -q "$out/package.apk" classes.dex lib/arm64-v8a/libclient.so lib/x86_64/libclient.so)
"$CLIENT_TOOLS/zipalign" -P 16 -f 4 "$out/package.apk" "$out/aligned.apk"
if [[ "${CLIENT_UNSIGNED:-0}" == 1 ]]; then
 cp "$out/aligned.apk" "$out/$artifact-unsigned.apk"
 printf '%s\n' "$out/$artifact-unsigned.apk"
 exit 0
fi
"$CLIENT_TOOLS/apksigner" sign --ks "$CLIENT_KEYSTORE" --ks-key-alias "$CLIENT_KEY_ALIAS" --ks-pass env:CLIENT_KEY_PASSWORD --out "$out/$artifact.apk" "$out/aligned.apk"
"$CLIENT_TOOLS/apksigner" verify --verbose "$out/$artifact.apk"
printf '%s\n' "$out/$artifact.apk"
