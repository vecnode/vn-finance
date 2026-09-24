@echo off
rem  vn-finance - double-click launcher for the local panel.
rem
rem  Same job as `node src/cli.ts web`, with one difference: the CLI hands the
rem  address to the default browser (Firefox on this machine), and this script
rem  opens Chrome. The address carries the per-run session token, so it is read
rem  back from what the server prints instead of being written down here.
rem
rem  The server runs in the background of THIS console window, so closing the
rem  window stops it. Options are passed through to the `web` command:
rem  `run.bat --port 7800`.

setlocal EnableExtensions EnableDelayedExpansion
chcp 65001 >nul
title vn-finance - painel local
cd /d "%~dp0"

set "LOG=%TEMP%\vn-finance-painel.log"
set "URL="
set "FALHOU="

if exist "%LOG%" del "%LOG%" >nul 2>&1

where node >nul 2>&1
if errorlevel 1 (
  echo.
  echo   Node.js não foi encontrado no PATH.
  echo   O vn-finance precisa do Node 22.18 ou superior: https://nodejs.org/
  echo.
  pause
  exit /b 1
)

rem Chrome first, so the fallback below only fires when it is really missing.
set "CHROME="
for /f "tokens=2*" %%a in ('reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe" /ve 2^>nul ^| findstr /i "REG_SZ"') do set "CHROME=%%b"
if not defined CHROME for /f "tokens=2*" %%a in ('reg query "HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe" /ve 2^>nul ^| findstr /i "REG_SZ"') do set "CHROME=%%b"
if not defined CHROME if exist "%ProgramFiles%\Google\Chrome\Application\chrome.exe" set "CHROME=%ProgramFiles%\Google\Chrome\Application\chrome.exe"
if not defined CHROME if exist "%ProgramFiles(x86)%\Google\Chrome\Application\chrome.exe" set "CHROME=%ProgramFiles(x86)%\Google\Chrome\Application\chrome.exe"
if not defined CHROME if exist "%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe" set "CHROME=%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe"

echo.
echo   A iniciar o painel do vn-finance...
echo.

rem The server writes its output to a log instead of this window: the address,
rem which carries the session token, has to be read before Chrome can open it.
rem `--no-open` keeps the CLI from opening the default browser as well.
start "" /b cmd /c "node src\cli.ts web --no-open %* > "%LOG%" 2>&1"

rem Wait for the address line. A server that cannot start reports it as a line
rem beginning with "erro", so a failure does not wait out the whole timeout.
for /l %%i in (1,1,15) do (
  if not defined URL (
    for /f "tokens=1" %%u in ('findstr /c:"http://127.0.0.1" "%LOG%" 2^>nul') do set "URL=%%u"
    if not defined URL (
      findstr /b /c:"erro " "%LOG%" >nul 2>&1 && set "FALHOU=1"
      if not defined FALHOU ping -n 2 127.0.0.1 >nul
    )
  )
)

if not defined URL (
  echo   O painel não arrancou. O servidor disse:
  echo.
  if exist "%LOG%" findstr /v /c:"--no-open" "%LOG%"
  echo.
  echo   Se a porta 7717 já estiver ocupada, tenta: run.bat --port 7800
  echo.
  pause
  exit /b 1
)

if defined CHROME (
  start "" "%CHROME%" "!URL!"
) else (
  echo   Chrome não encontrado; a abrir no navegador por omissão.
  start "" "!URL!"
)

rem The rest of the banner is printed a few lines after the address: give the
rem server that moment before copying its own output into this window.
ping -n 3 127.0.0.1 >nul
findstr /v /c:"--no-open" "%LOG%"
echo.

echo   O painel está a correr. Fecha esta janela para parar o servidor.
echo.
pause >nul

rem Best effort, so the window closes either way: stop whatever still listens on
rem the port this run used.
for /f "tokens=3 delims=:" %%a in ("!URL!") do for /f "tokens=1 delims=/" %%p in ("%%a") do set "PORTA=%%p"
if defined PORTA for /f "tokens=5" %%p in ('netstat -ano ^| findstr /c:"LISTENING" ^| findstr /c:":!PORTA! "') do taskkill /f /pid %%p >nul 2>&1

exit /b 0
