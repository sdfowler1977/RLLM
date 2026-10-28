# Build from source

## Layout and toolchain

This project builds directly with `rustc`, Android build tools, Java, and the NDK. It does not currently use Cargo or Gradle as the packaging entry point.

The supplied APK was built in Linux with Rust 1.99.1, JDK 17.0.21, Android platform 35, Android build-tools 35.0.0, and NDK r28c. `package.sh` currently selects the NDK's `linux-x86_64` host tools. Other host platforms need a reviewed packaging adaptation; installing the same SDK version alone does not make this shell script portable.

Install toolchains from their official distributions. You need Rust standard libraries for `aarch64-linux-android` and `x86_64-linux-android`, plus a JDK, Android SDK/build tools, NDK, Bash and `zip`.

```bash
rustup target add aarch64-linux-android x86_64-linux-android
export CLIENT_RUST="$(rustc --print sysroot)"
export CLIENT_JDK="/path/to/jdk-17"
export CLIENT_ANDROID="$HOME/Android/Sdk/platforms/android-35"
export CLIENT_TOOLS="$HOME/Android/Sdk/build-tools/35.0.0"
export CLIENT_NDK="/path/to/android-ndk-r28c"
```

Replace paths with installed locations. Point `CLIENT_NDK` to the actual r28c installation you selected. Verify each tool version locally.

## Compile a package

For a build-only check, without signing credentials:

```bash
CLIENT_UNSIGNED=1 bash package.sh
```

The script builds Rust static libraries and JNI shared libraries for both ABIs, compiles Android resources and Java, converts classes to DEX, packages the APK, and checks alignment. Outputs go under a UTC timestamped `output/` directory. An unsigned APK is not an installable release until it is signed.

## Sign your own development build

Keep signing material outside the repository. For your own new development identity, use `keytool` from the configured JDK and let it prompt for secrets:

```bash
mkdir -p "$HOME/rllm-signing"
"$CLIENT_JDK/bin/keytool" -genkeypair -keystore "$HOME/rllm-signing/local.p12" \
  -storetype PKCS12 -alias local -keyalg RSA -keysize 3072 -validity 3650

export CLIENT_KEYSTORE="$HOME/rllm-signing/local.p12"
export CLIENT_KEY_ALIAS=local
read -r -s -p 'Keystore password: ' CLIENT_KEY_PASSWORD
printf '\n'
export CLIENT_KEY_PASSWORD
bash package.sh
unset CLIENT_KEY_PASSWORD
```

The default `main` variant uses `com.local.client`, separately from the distributed `com.local.client.lab`. Your signing key is not the publisher's key. A differently signed APK cannot update the publisher's installed Lab app. Do not change, publish, or request the publisher's private key to make an update work.

For authorized maintainers using the existing Lab signing identity, set `CLIENT_VARIANT=lab` and an increasing integer `CLIENT_VERSION_CODE` before running the same script. Preserve the established signing identity. The provided Lab build has version code `1793159000`; subsequent Lab updates must increase it.

## Core checks

On a supported Linux host, from the repository root:

```bash
mkdir -p output/checks
rustc --edition=2021 --test core/lib.rs -o output/checks/core-checks
output/checks/core-checks --skip archive --skip network_project_zip_routes_to_review_without_text_decoding \
  --skip binary_network_zip_reaches_import_preview
```

The publication copy preserves product source and inline checks. Current source defines 210 checks; only focused checks were rerun for this release. See [release notes](../release/NOTES.md) for exact validation scope. All 24 runtime payloads were independently rebuilt and matched the supplied APK. Archive behavior and installed-device acceptance remain unverified.

## Verify the supplied APK

```bash
sha256sum release/RLLM-Lab.apk
"$CLIENT_TOOLS/apksigner" verify --verbose --print-certs release/RLLM-Lab.apk
```

Expected SHA-256:

```text
1c085b506ff0d74f53a5febe773be35972dae2f639645ce629f685a4e8dba4e2
```

Expected signer certificate SHA-256:

```text
a36bcaabf60626403ccffd32aaf442182efcd3cae82104f2e17c0cfc344749a2
```

The public package excludes SDKs, caches, signing credentials and private development logs. A source build needs those toolchains installed separately. No secrets are required to compile an unsigned package.
