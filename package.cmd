@echo off
rem Windows host route for the Android package. It performs the steps of
rem package.sh with the Windows builds of the same toolchain (tooling\windows,
rem tooling\platform) and signs only with the identity package.sh requires.
setlocal EnableDelayedExpansion
set "project_dir=%~dp0"
if "%project_dir:~-1%"=="\" set "project_dir=%project_dir:~0,-1%"
if not defined CLIENT_VARIANT set "CLIENT_VARIANT=lab"
if /i not "%CLIENT_VARIANT%"=="lab" if /i not "%CLIENT_VARIANT%"=="main" (
  echo Only the existing main or Lab application can be built.
  goto :refused
)
if not defined CLIENT_JDK set "CLIENT_JDK=%project_dir%\tooling\windows\jdk-17.0.21.1+2"
if not defined CLIENT_ANDROID set "CLIENT_ANDROID=%project_dir%\tooling\platform\android-35"
if not defined CLIENT_TOOLS set "CLIENT_TOOLS=%project_dir%\tooling\windows\android-15"
if not defined CLIENT_NDK set "CLIENT_NDK=%project_dir%\tooling\windows\android-ndk-r28c"
if not defined CLIENT_SIGNING_DIR set "CLIENT_SIGNING_DIR=%project_dir%\private"
set "PATH=%USERPROFILE%\.cargo\bin;%CLIENT_JDK%\bin;%PATH%"
set "JAVA_HOME=%CLIENT_JDK%"
set "tools=%project_dir%\output\windows-tools"
set "release_tool=%tools%\apk.exe"

call :fingerprint "%TEMP%\rllm-build-inputs-before.txt"
if not exist "%tools%" mkdir "%tools%"
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" arm64 >nul
if errorlevel 1 goto :failed
for %%t in (alignment apk manifest) do (
  rustc --edition=2021 --crate-name %%t --remap-path-prefix "%project_dir%=RLLM" -C opt-level=2 -C codegen-units=1 "%project_dir%\tools\%%t.rs" -o "%tools%\%%t.exe"
  if errorlevel 1 goto :failed
  rustc --test --edition=2021 --crate-name %%t --remap-path-prefix "%project_dir%=RLLM" -C opt-level=2 -C codegen-units=1 "%project_dir%\tools\%%t.rs" -o "%tools%\%%t-tests.exe"
  if errorlevel 1 goto :failed
  "%tools%\%%t-tests.exe" >"%tools%\%%t-tests.log" 2>&1
  if errorlevel 1 (
    echo Release tool tests failed: %%t
    type "%tools%\%%t-tests.log"
    goto :failed
  )
  findstr /b /c:"test result: ok." "%tools%\%%t-tests.log"
)

set "base_package="
for /f "delims=" %%p in ('call "%tools%\manifest.exe" --package "%project_dir%\AndroidManifest.xml"') do set "base_package=%%p"
if not defined base_package (
  echo The package identity cannot be read from AndroidManifest.xml
  goto :refused
)
if /i "%CLIENT_VARIANT%"=="lab" (
  set "variant_package=%base_package%.lab"
  set "expected_certificate=ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5"
) else (
  set "variant_package=%base_package%"
  set "expected_certificate=0986a4fa6c571459d4df9a1a153b79e9e582776e11e30e3d29593768d5520643"
)

set "baseline="
for /f "delims=" %%b in ('call "%release_tool%" --release-baseline --root "%project_dir%" --package %variant_package% 2^>^&1') do set "baseline=%%b"
echo(!baseline!| findstr /r /x "[0-9][0-9]*" >nul || (
  echo The release floor cannot be derived: !baseline!
  goto :refused
)
if not defined CLIENT_VERSION_CODE (
  echo Set CLIENT_VERSION_CODE to an increasing version code.
  goto :refused
)
echo(%CLIENT_VERSION_CODE%| findstr /r /x "[1-9][0-9]*" >nul || (
  echo Version code must be 1..2100000000
  goto :refused
)
if %CLIENT_VERSION_CODE% GTR 2100000000 (
  echo Version code must be 1..2100000000
  goto :refused
)
if %CLIENT_VERSION_CODE% LEQ %baseline% (
  echo Version code %CLIENT_VERSION_CODE% is not above the release floor %baseline% of package %variant_package%; a release must carry a higher code.
  goto :refused
)

set "out=%project_dir%\output\android-%CLIENT_VARIANT%-%CLIENT_VERSION_CODE%"
if exist "%out%" (
  echo Output directory %out% already exists; a version code names one build.
  goto :refused
)
mkdir "%out%\classes" "%out%\dex" "%out%\stage\lib\arm64-v8a" "%out%\stage\lib\x86_64" "%out%\gen" "%out%\tmp"
set "TMP=%out%\tmp"
set "TEMP=%out%\tmp"

set "manifest=%out%\AndroidManifest.xml"
set "resource_package="
if /i "%CLIENT_VARIANT%"=="lab" set "resource_package=--custom-package %base_package%"
"%tools%\manifest.exe" --render %CLIENT_VARIANT% %CLIENT_VERSION_CODE% "%project_dir%\AndroidManifest.xml" "%manifest%"
if errorlevel 1 goto :refused

if /i not "%CLIENT_UNSIGNED%"=="1" (
  if not defined CLIENT_KEYSTORE set "CLIENT_KEYSTORE=%CLIENT_SIGNING_DIR%\rllm.p12"
  if not defined CLIENT_KEY_ALIAS set "CLIENT_KEY_ALIAS=rllm"
  if not defined CLIENT_KEY_PASSWORD if exist "%CLIENT_SIGNING_DIR%\rllm.password" set /p CLIENT_KEY_PASSWORD=<"%CLIENT_SIGNING_DIR%\rllm.password"
  if not defined CLIENT_KEY_PASSWORD (
    echo Set CLIENT_KEY_PASSWORD
    goto :refused
  )
  "%CLIENT_JDK%\bin\keytool.exe" -exportcert -keystore "!CLIENT_KEYSTORE!" -alias !CLIENT_KEY_ALIAS! -storepass:env CLIENT_KEY_PASSWORD -file "%out%\signing-certificate.der" >nul
  if errorlevel 1 goto :refused
  set "actual_certificate="
  for /f "skip=1 delims=" %%h in ('certutil -hashfile "%out%\signing-certificate.der" SHA256') do if not defined actual_certificate set "actual_certificate=%%h"
  set "actual_certificate=!actual_certificate: =!"
  if /i not "!actual_certificate!"=="%expected_certificate%" (
    echo Signing certificate differs from the required application identity. No APK was signed.
    goto :refused
  )
)

set "ndk_bin=%CLIENT_NDK%\toolchains\llvm\prebuilt\windows-x86_64\bin"
for %%i in ("aarch64-linux-android arm64-v8a" "x86_64-linux-android x86_64") do (
  for /f "tokens=1,2" %%a in (%%i) do (
    rustc --edition=2021 --crate-name client --crate-type staticlib --remap-path-prefix "%project_dir%=RLLM" --target %%a -C opt-level=z -C codegen-units=1 -C panic=abort "%project_dir%\core\lib.rs" -o "%out%\lib-%%b.a"
    if errorlevel 1 goto :failed
    call "%ndk_bin%\%%a26-clang.cmd" -shared -fPIC -Oz "%project_dir%\platform\jni\bridge.c" "%out%\lib-%%b.a" -Wl,--gc-sections -Wl,--exclude-libs,ALL -Wl,-z,max-page-size=16384 -Wl,-z,relro,-z,now -ldl -lm -llog -o "%out%\stage\lib\%%b\libclient.so"
    if errorlevel 1 goto :failed
    "%ndk_bin%\llvm-strip.exe" --strip-unneeded "%out%\stage\lib\%%b\libclient.so"
    if errorlevel 1 goto :failed
  )
)

"%CLIENT_TOOLS%\aapt2.exe" compile --dir "%project_dir%\res" -o "%out%\resources.zip"
if errorlevel 1 goto :failed
mkdir "%out%\pack-notices"
copy /y "%project_dir%\notices\strata-monitor-MIT.txt" "%out%\pack-notices\" >nul
copy /y "%project_dir%\notices\rust-stdlib.txt" "%out%\pack-notices\" >nul
"%CLIENT_TOOLS%\aapt2.exe" link -o "%out%\resources.apk" -I "%CLIENT_ANDROID%\android.jar" --manifest "%manifest%" %resource_package% -A "%out%\pack-notices" --java "%out%\gen" "%out%\resources.zip"
if errorlevel 1 goto :failed
rem javac argument files treat a backslash as an escape, so the list uses forward slashes.
(for /f "delims=" %%f in ('dir /s /b /a-d "%project_dir%\platform\*.java" "%out%\gen\*.java"') do (
  set "source=%%f"
  echo(!source:\=/!
)) > "%out%\sources.txt"
"%CLIENT_JDK%\bin\javac.exe" --release 8 -encoding UTF-8 -cp "%CLIENT_ANDROID%\android.jar" -d "%out%\classes" @"%out%\sources.txt"
if errorlevel 1 goto :failed
"%CLIENT_JDK%\bin\jar.exe" cf "%out%\classes.jar" -C "%out%\classes" .
if errorlevel 1 goto :failed
call "%CLIENT_TOOLS%\d8.bat" --lib "%CLIENT_ANDROID%\android.jar" --min-api 26 --output "%out%\dex" "%out%\classes.jar"
if errorlevel 1 goto :failed
copy /y "%out%\resources.apk" "%out%\package.apk" >nul
copy /y "%out%\dex\classes.dex" "%out%\stage\classes.dex" >nul
for %%p in (classes.dex lib\arm64-v8a\libclient.so lib\x86_64\libclient.so) do (
  if not exist "%out%\stage\%%p" (
    echo Missing APK payload: %%p
    goto :failed
  )
  for %%s in ("%out%\stage\%%p") do if %%~zs EQU 0 (
    echo Missing APK payload: %%p
    goto :failed
  )
)
pushd "%out%\stage"
"%CLIENT_TOOLS%\aapt.exe" add "%out%\package.apk" classes.dex lib/arm64-v8a/libclient.so lib/x86_64/libclient.so >nul
set "added=%errorlevel%"
popd
if not "%added%"=="0" goto :failed
"%tools%\alignment.exe" --tool "%CLIENT_TOOLS%\zipalign.exe" "%out%\package.apk" "%out%\aligned.apk"
if errorlevel 1 goto :failed

call :fingerprint "%out%\build-inputs.sha256"
fc "%out%\build-inputs.sha256" "%BEFORE%" >nul
if errorlevel 1 (
  echo Build inputs changed during compilation; restart from the current source before signing or delivering this APK.
  goto :refused
)

if /i "%CLIENT_UNSIGNED%"=="1" (
  copy /y "%out%\aligned.apk" "%out%\RLLM-unsigned.apk" >nul
  call :advance
  if errorlevel 1 goto :refused
  echo %out%\RLLM-unsigned.apk
  exit /b 0
)
call "%CLIENT_TOOLS%\apksigner.bat" sign --ks "%CLIENT_KEYSTORE%" --ks-key-alias %CLIENT_KEY_ALIAS% --ks-pass env:CLIENT_KEY_PASSWORD --v2-signing-enabled true --v3-signing-enabled true --v4-signing-enabled false --out "%out%\signing.apk" "%out%\aligned.apk"
if errorlevel 1 goto :failed
call "%CLIENT_TOOLS%\apksigner.bat" verify --verbose "%out%\signing.apk" >nul
if errorlevel 1 goto :failed
copy /y /b "%out%\signing.apk" "%out%\RLLM.apk" >nul
fc /b "%out%\signing.apk" "%out%\RLLM.apk" >nul
if errorlevel 1 goto :failed
call "%CLIENT_TOOLS%\apksigner.bat" verify --verbose --print-certs "%out%\RLLM.apk" > "%out%\signature.txt"
if errorlevel 1 goto :failed
type "%out%\signature.txt"
findstr /x /c:"Signer #1 certificate SHA-256 digest: %expected_certificate%" "%out%\signature.txt" >nul || (
  echo Signed APK identity verification failed.
  goto :refused
)
"%CLIENT_TOOLS%\zipalign.exe" -c -P 16 4 "%out%\RLLM.apk"
if errorlevel 1 goto :failed
call :advance
if errorlevel 1 goto :refused
echo %out%\RLLM.apk
exit /b 0

rem A failure inside a parenthesized block or loop leaves through these labels,
rem because cmd can discard an exit code set from inside such a block.
:failed
endlocal & exit /b 1

:refused
endlocal & exit /b 2

:advance
"%release_tool%" --advance-baseline %CLIENT_VERSION_CODE% --root "%project_dir%" --package %variant_package%
if errorlevel 1 (
  echo The release floor of package %variant_package% could not be advanced to %CLIENT_VERSION_CODE%.
  goto :refused
)
exit /b 0

:fingerprint
if not defined BEFORE set "BEFORE=%~1"
rem dir /s searches every named argument recursively, so single files are listed apart.
(
  for /f "delims=" %%f in ('dir /s /b /a-d /o:n "%project_dir%\core" "%project_dir%\platform" "%project_dir%\res" "%project_dir%\notices"') do call :digest "%%f"
  for %%f in ("%project_dir%\AndroidManifest.xml" "%project_dir%\package.sh" "%project_dir%\package.cmd" "%project_dir%\tools\alignment.rs" "%project_dir%\tools\manifest.rs") do call :digest "%%~f"
) > "%~1"
exit /b 0

:digest
set "digest="
for /f "skip=1 delims=" %%h in ('certutil -hashfile "%~1" SHA256') do if not defined digest set "digest=%%h"
set "name=%~1"
echo(!digest: =!  !name:%project_dir%\=!
exit /b 0
