@echo off
cd "%~dp0"
if exist "%UserProfile%\Bin\code_scroller.exe" copy /y "C:\tmp\_rust\code_scroller\target\release\code_scroller.exe" "%UserProfile%\Bin\"
