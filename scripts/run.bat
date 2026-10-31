@echo off
rem  vn-finance - construir e abrir a janela, num duplo-clique.
rem
rem  Compila a versao de distribuicao e corre-a: e o mesmo que
rem      cargo build --release -p vnfin
rem      .\target\release\vnfin.exe
rem
rem  Nao ha nada a instalar. O cofre (~\.vn-finance) e criado no primeiro arranque,
rem  se ainda nao existir, e os dados ficam la a espera da proxima vez.
rem
rem  Se o binario ja estiver compilado, o cargo nao faz nada e arranca logo.

setlocal EnableExtensions
chcp 65001 >nul
title vn-finance
cd /d "%~dp0.."

where cargo >nul 2>&1
if errorlevel 1 (
  echo.
  echo   Nao encontrei o cargo no PATH.
  echo   Instala o Rust 1.95 ou superior: https://rustup.rs/
  echo.
  pause
  exit /b 1
)

echo.
echo   A compilar o vn-finance para distribuicao...
echo.

cargo build --release -p vnfin
if errorlevel 1 (
  echo.
  echo   A compilacao falhou. O erro esta acima.
  echo.
  pause
  exit /b 1
)

echo.
echo   A abrir a janela. Fecha-a para sair.
echo.

start "" "%~dp0..\target\release\vnfin.exe" %*
exit /b 0
