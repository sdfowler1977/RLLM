#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")" && pwd)
# Keep the existing package and RLLM name. The user authorized the retained
# replacement signer; subsequent builds must reuse this exact identity.
CLIENT_VARIANT=${CLIENT_VARIANT:-lab}
case "$CLIENT_VARIANT" in main|lab) ;; *) printf '%s\n' 'Only the existing main or Lab application can be built.' >&2; exit 2 ;; esac
: "${CLIENT_JDK:?Set CLIENT_JDK to a JDK 17 directory}"
: "${CLIENT_ANDROID:?Set CLIENT_ANDROID to the Android platform directory containing android.jar}"
: "${CLIENT_TOOLS:?Set CLIENT_TOOLS to Android build-tools 35.0.0}"
: "${CLIENT_NDK:?Set CLIENT_NDK to Android NDK r28c}"
: "${CLIENT_RUST:?Set CLIENT_RUST to Rust with Android standard-library targets installed}"
if [[ "${CLIENT_UNSIGNED:-0}" != 1 ]]; then
if [[ "$CLIENT_VARIANT" == lab && -n "${CLIENT_SIGNING_DIR:-}" ]]; then
 CLIENT_KEYSTORE="${CLIENT_KEYSTORE:-$CLIENT_SIGNING_DIR/rllm.p12}"
 CLIENT_KEY_ALIAS="${CLIENT_KEY_ALIAS:-rllm}"
 if [[ -z "${CLIENT_KEY_PASSWORD:-}" && -f "$CLIENT_SIGNING_DIR/rllm.password" ]]; then
  IFS= read -r CLIENT_KEY_PASSWORD < "$CLIENT_SIGNING_DIR/rllm.password" || [[ -n "$CLIENT_KEY_PASSWORD" ]]
 fi
fi
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
source_fingerprint() {
 find "$project_dir/core" "$project_dir/platform" "$project_dir/res" "$project_dir/notices" \
      "$project_dir/AndroidManifest.xml" "$project_dir/package.sh" -type f -print0 |
  LC_ALL=C sort -z | xargs -0 sha256sum
}
build_source=$(source_fingerprint)
case "$CLIENT_VARIANT" in
 main) ;;
 lab)
  : "${CLIENT_VERSION_CODE:?Set CLIENT_VERSION_CODE to an increasing Lab version code}"
  if [[ ! "$CLIENT_VERSION_CODE" =~ ^[1-9][0-9]{0,9}$ ]] || (( CLIENT_VERSION_CODE > 2100000000 )); then
   printf '%s\n' 'Version code must be 1..2100000000' >&2; exit 2
  fi
  variant=${CLIENT_VARIANT}
  label="RLLM"
  # Keep the installed package and Java/JNI names when correcting the label.
  sed -e "s/package=\"com.local.client\"/package=\"com.local.client.$variant\"/" \
      -e "s/android:label=\"RLLM\"/android:label=\"$label\"/" \
      -e 's/android:name="\./android:name="com.local.client./g' \
      -e 's/android:targetActivity="\./android:targetActivity="com.local.client./g' \
      -e 's/android:parentActivityName="\./android:parentActivityName="com.local.client./g' \
      -e "s/android:versionCode=\"[^\"]*\"/android:versionCode=\"$CLIENT_VERSION_CODE\"/" \
      -e "s/android:versionName=\"[^\"]*\"/android:versionName=\"0.1-$variant.$CLIENT_VERSION_CODE\"/" \
      "$manifest" > "$out/AndroidManifest.xml"
  manifest="$out/AndroidManifest.xml"
  resource_package=(--custom-package com.local.client)
  ;;
 *) printf '%s\n' 'Unknown CLIENT_VARIANT' >&2; exit 2 ;;
esac
expected_certificate="${CLIENT_EXPECTED_CERT_SHA256:-}"
if [[ "$CLIENT_VARIANT" == lab ]]; then
 expected_certificate=ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5
else
 expected_certificate=0986a4fa6c571459d4df9a1a153b79e9e582776e11e30e3d29593768d5520643
fi
if [[ "${CLIENT_UNSIGNED:-0}" != 1 ]]; then
 export CLIENT_KEY_PASSWORD
 "$CLIENT_JDK/bin/keytool" -exportcert -keystore "$CLIENT_KEYSTORE" -alias "$CLIENT_KEY_ALIAS" -storepass:env CLIENT_KEY_PASSWORD > "$out/signing-certificate.der"
 actual_certificate=$(sha256sum "$out/signing-certificate.der")
 actual_certificate=${actual_certificate%% *}
 if [[ -n "$expected_certificate" && "$actual_certificate" != "$expected_certificate" ]]; then
  printf '%s\n' 'Signing certificate differs from the required application identity. No APK was signed.' >&2
  exit 2
 fi
fi
ndk_bin="$CLIENT_NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
for item in 'aarch64-linux-android arm64-v8a' 'x86_64-linux-android x86_64'; do
 read -r target abi <<< "$item"
 "$CLIENT_RUST/bin/rustc" --edition=2021 --crate-name client --crate-type staticlib --remap-path-prefix "$project_dir=RLLM" --target "$target" -C opt-level=2 -C codegen-units=1 -C panic=abort "$project_dir/core/lib.rs" -o "$out/lib-$abi.a"
 "$ndk_bin/${target}26-clang" -shared -fPIC -O2 "$project_dir/platform/jni/bridge.c" "$out/lib-$abi.a" -Wl,--exclude-libs,ALL -Wl,-z,max-page-size=16384 -Wl,-z,relro,-z,now -ldl -lm -llog -o "$out/stage/lib/$abi/libclient.so"
 "$ndk_bin/llvm-strip" --strip-unneeded "$out/stage/lib/$abi/libclient.so"
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
if [[ "$build_source" != "$(source_fingerprint)" ]]; then
 printf '%s\n' 'Build inputs changed during compilation; restart from the current source before signing or delivering this APK.' >&2
 exit 2
fi
printf '%s\n' "$build_source" > "$out/build-inputs.sha256"
if [[ "${CLIENT_UNSIGNED:-0}" == 1 ]]; then
 cp "$out/aligned.apk" "$out/$artifact-unsigned.apk"
 printf '%s\n' "$out/$artifact-unsigned.apk"
 exit 0
fi
"$CLIENT_TOOLS/apksigner" sign --ks "$CLIENT_KEYSTORE" --ks-key-alias "$CLIENT_KEY_ALIAS" --ks-pass env:CLIENT_KEY_PASSWORD --v2-signing-enabled true --v3-signing-enabled true --v4-signing-enabled false --out "$out/signing.apk" "$out/aligned.apk"
# Publish the finalized signing output through a complete sequential write.
# The persisted delivery file must contain the whole signed ZIP, including its
# central directory, before any later process reads or exports it.
"$CLIENT_TOOLS/apksigner" verify --verbose "$out/signing.apk"
dd if="$out/signing.apk" of="$out/$artifact.apk" bs=1048576 conv=fsync status=none
cmp -s "$out/signing.apk" "$out/$artifact.apk"
"$CLIENT_TOOLS/apksigner" verify --verbose --print-certs "$out/$artifact.apk" > "$out/signature.txt"
cat "$out/signature.txt"
if [[ -n "$expected_certificate" ]]; then
 grep -Fqx "Signer #1 certificate SHA-256 digest: $expected_certificate" "$out/signature.txt" || { printf '%s\n' 'Signed APK identity verification failed.' >&2; exit 2; }
fi
"$CLIENT_TOOLS/zipalign" -c -P 16 4 "$out/$artifact.apk"
printf '%s\n' "$out/$artifact.apk"
