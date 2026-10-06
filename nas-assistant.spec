# -*- mode: python ; coding: utf-8 -*-
# Spec de PyInstaller para el Asistente de Administración Remota NAS (Windows).
# Genera un ejecutable de consola "NAS-Assistant.exe" (onefile).
from PyInstaller.utils.hooks import collect_submodules

hiddenimports = (
    collect_submodules("paramiko")
    + collect_submodules("keyring")
    + collect_submodules("cryptography")
    + ["keyring.backends.Windows"]
)

a = Analysis(
    ["nas_admin.py"],
    pathex=[],
    binaries=[],
    datas=[],
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name="NAS-Assistant",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)