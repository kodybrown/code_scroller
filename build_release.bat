@echo off
cd "%~dp0"
cargo build --release
if %errorlevel% neq 0 (
  exit /b %errorlevel%
)
copy /y "C:\tmp\_rust\code_scroller\target\release\code_scroller.exe" "."
copy /y "C:\tmp\_rust\code_scroller\target\release\code_scroller" "."
