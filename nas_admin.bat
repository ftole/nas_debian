@echo off
setlocal

title Asistente de Administracion Remota NAS Debian 13

set "SCRIPT_DIR=%~dp0"
set "VENV_DIR=%SCRIPT_DIR%.venv"
set "REQ_FILE=%SCRIPT_DIR%requirements-assistant.txt"
set "PY_SCRIPT=%SCRIPT_DIR%nas_admin.py"

:: 1. Detectar interprete de Python funcional
set "PYTHON_EXE="
set "PYTHON_ARGS="

:: 1a. Probar py launcher
py -3 -c "import sys" >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    set "PYTHON_EXE=py"
    set "PYTHON_ARGS=-3"
    goto :python_found
)

:: 1b. Probar python en PATH
python -c "import sys" >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    set "PYTHON_EXE=python"
    goto :python_found
)

:: 1c. Probar python3 en PATH
python3 -c "import sys" >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    set "PYTHON_EXE=python3"
    goto :python_found
)

:: 1d. Buscar en LocalAppData (instalador por usuario)
if exist "%LOCALAPPDATA%\Programs\Python" (
    for /f "delims=" %%I in ('dir /b /s "%LOCALAPPDATA%\Programs\Python\python.exe" 2^>nul') do (
        "%%~fI" -c "import sys" >nul 2>&1
        if not errorlevel 1 (
            set "PYTHON_EXE=%%~fI"
            goto :python_found
        )
    )
)

:: 1e. Buscar en ProgramFiles
if exist "%ProgramFiles%\Python" (
    for /f "delims=" %%I in ('dir /b /s "%ProgramFiles%\Python\python.exe" 2^>nul') do (
        "%%~fI" -c "import sys" >nul 2>&1
        if not errorlevel 1 (
            set "PYTHON_EXE=%%~fI"
            goto :python_found
        )
    )
)

:python_found
if "%PYTHON_EXE%"=="" (
    echo [-] ERROR: Python 3 no se encuentra instalado o no esta en el PATH.
    echo     Instale Python desde https://www.python.org/downloads/ y marque "Add Python to PATH".
    if "%~1"=="" pause
    exit /b 1
)

:: 2. Crear entorno virtual si no existe
if not exist "%VENV_DIR%\Scripts\python.exe" (
    echo [*] Creando entorno virtual en %VENV_DIR%...
    if defined PYTHON_ARGS (
        "%PYTHON_EXE%" %PYTHON_ARGS% -m venv "%VENV_DIR%"
    ) else (
        "%PYTHON_EXE%" -m venv "%VENV_DIR%"
    )
    if errorlevel 1 (
        echo [-] Error al crear el entorno virtual.
        if "%~1"=="" pause
        exit /b 1
    )
)

:: 3. Verificar e instalar dependencias silenciosamente si faltan
"%VENV_DIR%\Scripts\python.exe" -c "import paramiko, keyring, colorama" >nul 2>&1
if errorlevel 1 (
    echo [*] Instalando dependencias necesarias: paramiko, keyring, colorama...
    "%VENV_DIR%\Scripts\python.exe" -m pip install -q -r "%REQ_FILE%"
    if errorlevel 1 (
        echo [-] Error al instalar dependencias desde %REQ_FILE%.
        if "%~1"=="" pause
        exit /b 1
    )
)

:: 4. Ejecutar el asistente con los argumentos recibidos
"%VENV_DIR%\Scripts\python.exe" "%PY_SCRIPT%" %*
exit /b %ERRORLEVEL%
