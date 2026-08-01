@echo off
setlocal
cd /d "%~dp0"
if exist "%~dp0..\tools\bin\luac_mspm0.exe" set "MSPM0_LUAC=%~dp0..\tools\bin\luac_mspm0.exe"
if exist "%~dp0dist\bin\luac_mspm0.exe" set "MSPM0_LUAC=%~dp0dist\bin\luac_mspm0.exe"
if exist "%~dp0dist\Lua IDE.exe" (
  start "" /D "%~dp0dist" "%~dp0dist\Lua IDE.exe"
  exit /b 0
)
if exist "%~dp0target\x86_64-pc-windows-gnullvm\debug\mspm0-lua-ide.exe" (
  if not exist "%~dp0target\x86_64-pc-windows-gnullvm\debug\libunwind.dll" (
    if exist "%~dp0dist\libunwind.dll" copy /Y "%~dp0dist\libunwind.dll" "%~dp0target\x86_64-pc-windows-gnullvm\debug\" >nul
  )
  start "" /D "%~dp0target\x86_64-pc-windows-gnullvm\debug" "%~dp0target\x86_64-pc-windows-gnullvm\debug\mspm0-lua-ide.exe"
  exit /b 0
)
echo Build first: powershell -File build.ps1
exit /b 1
