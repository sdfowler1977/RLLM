#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")" && pwd)
# Keep the existing package and RLLM name. The user authorized the retained
# replacement signer; subsequent builds must reuse this exact identity.
CLIENT_VARIANT=${CLIENT_VARIANT:-lab}
case "$CLIENT_VARIANT" in main|lab) ;; *) printf '%s\n' 'Only the existing main or Lab application can be built.' >&2; exit 2 ;; esac
# The release number may only ever grow: Android refuses to install an APK
# whose android:versionCode is not strictly greater than the one already
# installed on the device. The version code of a package is counted per
# package id, so each package the tree publishes carries a floor of its own.
# The floor is the product's own join: the highest compiled versionCode any
# retained artifact of that package carries, joined with the version and the
# distributed baseline the publication record holds for that package. The
# route reads that floor from the product tool, so the route and the tool can
# never hold two different floors, and a floor that cannot be derived is a
# refusal rather than a default.
publication_record="$project_dir/PUBLICATION-MANIFEST.json"
release_tool="$project_dir/output/tools/apk"
# The manifest the tree ships is the source of every variant substitution.
source_manifest="$project_dir/AndroidManifest.xml"
variant=${CLIENT_VARIANT}
label="RLLM"
# The package identity each variant publishes under, read from the tree's own
# manifest: the main application keeps the identity the tree ships and the lab
# variant publishes under its own identity, so the two can be installed side
# by side and each keeps a release floor of its own. The route asks the
# product tool about that identity, so the floor it enforces is the floor of
# the package the variant actually ships.
base_package=$(sed -n -e '/^[[:space:]]*<manifest[[:space:]]/s/.*[[:space:]]package="\([^"]*\)".*/\1/p' "$source_manifest" | head -n 1)
if [[ -z "$base_package" ]]; then
 printf 'The package identity cannot be read from %s\n' "$source_manifest" >&2
 exit 2
fi
case "$CLIENT_VARIANT" in
 main) variant_package=$base_package ;;
 lab) variant_package="$base_package.$variant" ;;
esac
# The floor below is read through the product tool, so the tool the route
# consults must be the tool this source builds: the rebuild happens before the
# floor is read, never after it, because an installed output/tools/apk can
# predate the release interface this route asks about. The fingerprint window
# opens before that rebuild and closes after compilation, so the release tool
# and the aligner are built from the same tools/*.rs bytes that
# build-inputs.sha256 records: both are build inputs of this pipeline
# (tools/alignment.rs is in source_fingerprint).
source_fingerprint() {
 find "$project_dir/core" "$project_dir/platform" "$project_dir/res" "$project_dir/notices" \
      "$project_dir/AndroidManifest.xml" "$project_dir/package.sh" "$project_dir/tools/alignment.rs" -type f -print0 |
  LC_ALL=C sort -z | xargs -0 sha256sum
}
build_source=$(source_fingerprint)
bash "$project_dir/tools/build-tools.sh" "$project_dir/output/tools"
read_baseline() {
 # The floor is read from the product tool, never from a second reader of the
 # record: the tool resolves the record's own top-level members and joins them
 # with the codes the retained artifacts of that package carry. The tool named
 # here was just built from this source by the rebuild above.
 "$release_tool" --release-baseline --root "$project_dir" --package "$variant_package" 2>&1
}
if [[ ! -x "$release_tool" ]]; then
 printf 'The release tool %s is absent; build it with: bash tools/build-tools.sh output/tools\n' "$release_tool" >&2
 exit 2
fi
if ! baseline=$(read_baseline); then
 printf 'The release floor cannot be derived: %s\n' "$baseline" >&2
 exit 2
fi
if [[ ! "$baseline" =~ ^[0-9]+$ ]]; then
 printf 'The release floor cannot be derived: %s\n' "$baseline" >&2
 exit 2
fi
# The floor is a single number, read once, before anything is built.
check_version_code() {
 local code=$1
 if [[ ! "$code" =~ ^[1-9][0-9]{0,9}$ ]] || (( code > 2100000000 )); then
  printf '%s\n' 'Version code must be 1..2100000000' >&2
  return 2
 fi
 if (( code <= baseline )); then
  printf 'Version code %s is not above the release floor %s of package %s; a release must carry a higher code.\n' "$code" "$baseline" "$variant_package" >&2
  return 2
 fi
 return 0
}
# The release floor the record carries is advanced by the product tool: the
# tool rewrites the record's own top-level members, so a member of the same
# name inside another object is left exactly as it was, the recorded value
# never moves backwards, and the floor of the other package is left alone.
advance_baseline() {
 local code=$1
 local advanced
 if ! advanced=$("$release_tool" --advance-baseline "$code" --root "$project_dir" --package "$variant_package" 2>&1); then
  printf 'The release floor of package %s in %s could not be advanced to %s: %s\n' "$variant_package" "$publication_record" "$code" "$advanced" >&2
  exit 2
 fi
}
# The release number is checked before anything is built, so a refused code
# costs no build directory, no toolchain and no output.
case "$CLIENT_VARIANT" in
 main) : "${CLIENT_VERSION_CODE:?Set CLIENT_VERSION_CODE to an increasing Main version code}" ;;
 lab) : "${CLIENT_VERSION_CODE:?Set CLIENT_VERSION_CODE to an increasing Lab version code}" ;;
esac
check_version_code "$CLIENT_VERSION_CODE" || exit $?
# The floor is a first-class product surface, not only a step inside the
# build: it can be exercised on its own, without the toolchain.
if [[ "${1:-}" == '--check-version-code' ]]; then
 printf 'version code %s accepted: strictly above the release floor %s of package %s\n' "$CLIENT_VERSION_CODE" "$baseline" "$variant_package"
 exit 0
fi
# The advance is a first-class product surface too: the floor the record
# carries for this variant's package can be advanced on its own, without the
# toolchain, so the step a build performs can be exercised on its own.
if [[ "${1:-}" == '--advance-baseline' ]]; then
 advance_baseline "$CLIENT_VERSION_CODE"
 printf 'release floor of package %s advanced to %s\n' "$variant_package" "$CLIENT_VERSION_CODE"
 exit 0
fi
# The manifest a build would carry is a product surface too: rendering it
# without a build lets the variant substitution be read directly, so the main
# package identity and the substituted release number can be inspected apart
# from a whole APK.
render_manifest() {
 case "$CLIENT_VARIANT" in
  main)
   # The main application keeps its installed package identity, its launcher
   # label and its class names; only the release number is substituted, so the
   # shipped main package never carries the placeholder code 1.
   sed -e "s/android:versionCode=\"[^\"]*\"/android:versionCode=\"$CLIENT_VERSION_CODE\"/" \
       -e "s/android:versionName=\"[^\"]*\"/android:versionName=\"0.1-$variant.$CLIENT_VERSION_CODE\"/" \
       "$source_manifest"
   ;;
  lab)
   # Keep the installed package and Java/JNI names when correcting the label.
   # The identity comes from the tree's own manifest, so the package the
   # variant declares and the package the release floor is held to are one
   # string and not two.
   sed -e "s/package=\"$base_package\"/package=\"$variant_package\"/" \
       -e "s/android:label=\"RLLM\"/android:label=\"$label\"/" \
       -e "s/android:name=\"\\./android:name=\"$base_package./g" \
       -e "s/android:targetActivity=\"\\./android:targetActivity=\"$base_package./g" \
       -e "s/android:parentActivityName=\"\\./android:parentActivityName=\"$base_package./g" \
       -e "s/android:scheme=\"$base_package.oauth\"/android:scheme=\"$variant_package.oauth\"/" \
       -e "s/android:versionCode=\"[^\"]*\"/android:versionCode=\"$CLIENT_VERSION_CODE\"/" \
       -e "s/android:versionName=\"[^\"]*\"/android:versionName=\"0.1-$variant.$CLIENT_VERSION_CODE\"/" \
       "$source_manifest"
   ;;
  *) printf '%s\n' 'Unknown CLIENT_VARIANT' >&2; exit 2 ;;
 esac
}
if [[ "${1:-}" == '--render-manifest' ]]; then
 render_manifest
 exit 0
fi
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
# A build directory is named by its publication stamp, so one stamp names one
# build: refuse to reuse a directory instead of overwriting the artifacts of
# the build that already owns it.
mkdir -p "$project_dir/output"
if ! mkdir "$out"; then
 printf 'Output directory %s already exists; a publication stamp names one build.\n' "$out" >&2
 exit 2
fi
mkdir -p "$out/classes" "$out/dex" "$out/stage/lib/arm64-v8a" "$out/stage/lib/x86_64" "$out/gen"
mkdir -p "$out/tmp"
export TMPDIR="$out/tmp"
manifest="$source_manifest"
artifact="RLLM"
resource_package=()
case "$CLIENT_VARIANT" in
 main)
  render_manifest > "$out/AndroidManifest.xml"
  manifest="$out/AndroidManifest.xml"
  ;;
 lab)
  render_manifest > "$out/AndroidManifest.xml"
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
 "$CLIENT_RUST/bin/rustc" --edition=2021 --crate-name client --crate-type staticlib --remap-path-prefix "$project_dir=RLLM" --target "$target" -C opt-level=z -C codegen-units=1 -C panic=abort "$project_dir/core/lib.rs" -o "$out/lib-$abi.a"
 "$ndk_bin/${target}26-clang" -shared -fPIC -Oz "$project_dir/platform/jni/bridge.c" "$out/lib-$abi.a" -Wl,--gc-sections -Wl,--exclude-libs,ALL -Wl,-z,max-page-size=16384 -Wl,-z,relro,-z,now -ldl -lm -llog -o "$out/stage/lib/$abi/libclient.so"
 "$ndk_bin/llvm-strip" --strip-unneeded "$out/stage/lib/$abi/libclient.so"
done
"$CLIENT_TOOLS/aapt2" compile --dir "$project_dir/res" -o "$out/resources.zip"
mkdir -p "$out/pack-notices"
cp "$project_dir/notices/strata-monitor-MIT.txt" "$project_dir/notices/rust-stdlib.txt" "$out/pack-notices/"
"$CLIENT_TOOLS/aapt2" link -o "$out/resources.apk" -I "$CLIENT_ANDROID/android.jar" --manifest "$manifest" "${resource_package[@]}" -A "$out/pack-notices" --java "$out/gen" "$out/resources.zip"
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
# The native aligner is the binary the rebuild at the top of this route built,
# before the release floor was read. That rebuild sits inside the fingerprint
# window opened above and closed by the check below, so the binary that aligns
# this APK is built from the same tools/alignment.rs bytes that
# build-inputs.sha256 records.
"$project_dir/output/tools/alignment" --tool "$CLIENT_TOOLS/zipalign" "$out/package.apk" "$out/aligned.apk"
if [[ "$build_source" != "$(source_fingerprint)" ]]; then
 printf '%s\n' 'Build inputs changed during compilation; restart from the current source before signing or delivering this APK.' >&2
 exit 2
fi
printf '%s\n' "$build_source" > "$out/build-inputs.sha256"
if [[ "${CLIENT_UNSIGNED:-0}" == 1 ]]; then
 cp "$out/aligned.apk" "$out/$artifact-unsigned.apk"
 advance_baseline "$CLIENT_VERSION_CODE"
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
advance_baseline "$CLIENT_VERSION_CODE"
printf '%s\n' "$out/$artifact.apk"
