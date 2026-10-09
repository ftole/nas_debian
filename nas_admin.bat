@echo off
setlocal enabledelayedexpansion

title Asistente de Administracion Remota NAS Debian 13

set "SCRIPT_DIR=%~dp0"
set "VENV_DIR=%SCRIPT_DIR%.venv"
set "REQ_FILE=%SCRIPT_DIR%requirements-assistant.txt"
set "PY_SCRIPT=%SCRIPT_DIR%nas_admin.py"

:: 1. Detectar interprete de Python
set "PYTHON_CMD="
where py >nul 2>&1
if !ERRORLEVEL! EQU 0 (
    set "PYTHON_CMD=py -3"
) else (
    where python >nul 2>&1
    if !ERRORLEVEL! EQU 0 (
        set "PYTHON_CMD=python"
    )
)

if "%PYTHON_CMD%"=="" (
    echo [-] ERROR: Python 3 no se encuentra instalado o no esta en el PATH.
    echo     Instale Python desde https://www.python.org/downloads/ y marque "Add Python to PATH".
    pause
    exit /b 1
)

:: 2. Crear entorno virtual si no existe
if not exist "%VENV_DIR%\Scripts\python.exe" (
    echo [*] Creando entorno virtual en %VENV_DIR%...
    %PYTHON_CMD% -m venv "%VENV_DIR%"
    if !ERRORLEVEL! NEQ 0 (
        echo [-] Error al crear el entorno virtual.
        pause
        exit /b 1
    )
)

:: 3. Verificar e instalar dependencias silenciosamente si faltan
"%VENV_DIR%\Scripts\python.exe" -c "import paramiko, keyring, colorama" >nul 2>&1
if !ERRORLEVEL! NEQ 0 (
    echo [*] Instalando dependencias necesarias (paramiko, keyring, colorama)...
    "%VENV_DIR%\Scripts\python.exe" -m pip install -q -r "%REQ_FILE%"
    if !ERRORLEVEL! NEQ 0 (
        echo [-] Error al instalar dependencias desde %REQ_FILE%.
        pause
        exit /b 1
    )
)

:: 4. Ejecutar el asistente con los argumentos recibidos
"%VENV_DIR%\Scripts\python.exe" "%PY_SCRIPT%" %*
exit /b %ERRORLEVEL%
