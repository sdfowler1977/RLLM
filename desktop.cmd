@echo off
setlocal
set "project_dir=%~dp0"
if "%project_dir:~-1%"=="\" set "project_dir=%project_dir:~0,-1%"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
if not exist "%project_dir%\output\desktop" mkdir "%project_dir%\output\desktop"
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" arm64
if errorlevel 1 exit /b 1
rustc --edition 2021 --crate-name client --crate-type bin --remap-path-prefix "%project_dir%=RLLM" -C opt-level=2 -C codegen-units=16 "%project_dir%\core\desktop.rs" -o "%project_dir%\output\desktop\RLLM-Desktop.exe"
if errorlevel 1 exit /b 1
set "SIGNTOOL=C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\arm64\signtool.exe"
if not exist "%SIGNTOOL%" set "SIGNTOOL=signtool.exe"
certutil -user -store My "RLLM" | findstr /C:"Subject:" | findstr /C:"RLLM" >nul
if errorlevel 1 (
  echo The RLLM signing certificate is not installed. The executable was left unsigned.
) else (
  "%SIGNTOOL%" sign /fd SHA256 /n "RLLM" /tr http://timestamp.digicert.com /td SHA256 "%project_dir%\output\desktop\RLLM-Desktop.exe"
  if errorlevel 1 exit /b 1
)
echo %project_dir%\output\desktop\RLLM-Desktop.exe
