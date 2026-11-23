@echo off
rem Builds RLLM for Windows: the ARM64, x64 and x86 desktop executables, RLLM-Setup.exe
rem (installs the native build for the PC's processor), RLLM-Setup-x64.exe and the
rem x64 zip. Every executable is signed with the RLLM certificate when it is installed.
rem Each desktop executable embeds desktop.manifest, including longPathAware.
setlocal
set "project_dir=%~dp0"
if "%project_dir:~-1%"=="\" set "project_dir=%project_dir:~0,-1%"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
rustup target add i686-pc-windows-msvc
if errorlevel 1 exit /b 1
set "out=%project_dir%\output\desktop"
set "windows=%project_dir%\platform\windows"
set "vcvars=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"
set "rust=--edition 2021 --remap-path-prefix "%project_dir%=RLLM" -C opt-level=2 -C codegen-units=16"
if not exist "%out%\x64" mkdir "%out%\x64"
if not exist "%out%\package" mkdir "%out%\package"
if not exist "%windows%\rllm.ico" (
  echo platform\windows\rllm.ico is missing. Write it with: RLLM-Desktop.exe --write-icon platform\windows\rllm.ico
  exit /b 1
)

call "%vcvars%" arm64 >nul
if errorlevel 1 exit /b 1
rc /nologo /fo "%out%\rllm.res" "%windows%\rllm.rc"
if errorlevel 1 exit /b 1
rc /nologo /d RLLM_SETUP /fo "%out%\setup.res" "%windows%\rllm.rc"
if errorlevel 1 exit /b 1
copy /y "%project_dir%\desktop.manifest" "%out%\app.manifest" >nul
if errorlevel 1 exit /b 1
pushd "%out%"
echo 1 24 "app.manifest">app.rc
rc /nologo /fo app.res app.rc
set "rc_err=%errorlevel%"
popd
if not "%rc_err%"=="0" exit /b 1
rustc %rust% --crate-name client --crate-type bin -C link-arg="%out%\rllm.res" -C link-arg="%out%\app.res" "%project_dir%\core\desktop.rs" -o "%out%\RLLM-Desktop.exe"
if errorlevel 1 exit /b 1
call :sign "%out%\RLLM-Desktop.exe" || exit /b 1
endlocal & set "project_dir=%project_dir%" & set "out=%out%" & set "windows=%windows%" & set "vcvars=%vcvars%"

setlocal
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
set "rust=--edition 2021 --remap-path-prefix "%project_dir%=RLLM" -C opt-level=2 -C codegen-units=16 --target x86_64-pc-windows-msvc"
call "%vcvars%" arm64_amd64 >nul
if errorlevel 1 exit /b 1
rustc %rust% --crate-name client --crate-type bin -C link-arg="%out%\rllm.res" -C link-arg="%out%\app.res" "%project_dir%\core\desktop.rs" -o "%out%\x64\RLLM-Desktop.exe"
if errorlevel 1 exit /b 1
call :sign "%out%\x64\RLLM-Desktop.exe" || exit /b 1
rem The setup is an x64 program so that it starts on both kinds of PC.
set "RLLM_ARM64=%out%\RLLM-Desktop.exe"
set "RLLM_X64=%out%\x64\RLLM-Desktop.exe"
rustc %rust% --crate-name setup --crate-type bin --cfg rllm_arm64 -C link-arg="%out%\setup.res" "%windows%\setup.rs" -o "%out%\RLLM-Setup.exe"
if errorlevel 1 exit /b 1
call :sign "%out%\RLLM-Setup.exe" || exit /b 1
rustc %rust% --crate-name setup --crate-type bin -C link-arg="%out%\setup.res" "%windows%\setup.rs" -o "%out%\x64\RLLM-Setup-x64.exe"
if errorlevel 1 exit /b 1
call :sign "%out%\x64\RLLM-Setup-x64.exe" || exit /b 1
endlocal & set "project_dir=%project_dir%" & set "out=%out%" & set "windows=%windows%" & set "vcvars=%vcvars%"

call :build_x86
if errorlevel 1 exit /b 1

setlocal
rem The x64 zip carries the x64 setup, the public signing certificate and a short guide.
set "stage=%out%\package\RLLM-Windows-x64"
if exist "%stage%" rmdir /s /q "%stage%"
mkdir "%stage%"
copy /y "%out%\x64\RLLM-Setup-x64.exe" "%stage%\RLLM-Setup-x64.exe" >nul
copy /y "%windows%\README-x64.txt" "%stage%\Read me.txt" >nul
copy /y "%project_dir%\release\RLLM.cer" "%stage%\RLLM.cer" >nul
if exist "%out%\RLLM-Windows-x64.zip" del /q "%out%\RLLM-Windows-x64.zip"
tar -a -c -f "%out%\RLLM-Windows-x64.zip" -C "%out%\package" RLLM-Windows-x64
if errorlevel 1 exit /b 1
echo %out%\RLLM-Desktop.exe
echo %out%\x64\RLLM-Desktop.exe
echo %out%\x86\RLLM-Desktop.exe
echo %out%\RLLM-Setup.exe
echo %out%\x64\RLLM-Setup-x64.exe
echo %out%\RLLM-Windows-x64.zip
exit /b 0

:build_x86
setlocal
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
set "rust=--edition 2021 --remap-path-prefix "%project_dir%=RLLM" -C opt-level=2 -C codegen-units=16 --target i686-pc-windows-msvc"
call "%vcvars%" arm64_x86 >nul
if errorlevel 1 goto :x86_failed
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
if not exist "%out%\x86" mkdir "%out%\x86"
rem On 32-bit the pointer-sized window-long import is the long import.
rustc %rust% --crate-name client --crate-type bin -C link-arg="%out%\rllm.res" -C link-arg="%out%\app.res" -C link-arg=/ALTERNATENAME:__imp__SetWindowLongPtrW@12=__imp__SetWindowLongW@12 "%project_dir%\core\desktop.rs" -o "%out%\x86\RLLM-Desktop.exe"
if errorlevel 1 goto :x86_failed
call :sign "%out%\x86\RLLM-Desktop.exe"
if errorlevel 1 goto :x86_failed
endlocal
exit /b 0
:x86_failed
endlocal
exit /b 1

:sign
set "SIGNTOOL=C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\arm64\signtool.exe"
if not exist "%SIGNTOOL%" set "SIGNTOOL=signtool.exe"
certutil -user -store My "RLLM" | findstr /C:"Subject:" | findstr /C:"RLLM" >nul
if errorlevel 1 (
  echo The RLLM signing certificate is not installed. %~nx1 was left unsigned.
  exit /b 0
)
"%SIGNTOOL%" sign /fd SHA256 /n "RLLM" /tr http://timestamp.digicert.com /td SHA256 /q "%~1"
exit /b %errorlevel%
