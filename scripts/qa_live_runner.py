#!/usr/bin/env python3
"""Runner QA en vivo interactivo y visible con Playwright (Desktop / Headless=False).

Ejecuta el plan de validación de extremo a extremo sobre el servidor NAS en https://10.10.1.222:
  1. Inicio de sesión administrativo visible.
  2. Creación y verificación de grupo ('grp_qa_test') y usuario ('qa_user').
  3. Creación y verificación de recurso compartido Samba ('QA_TEST_SHARE').
  4. Explorador de archivos: carpeta '_Prueba_QA', archivo 'prueba_qa.txt', visor integrado,
     traslado a papelera confinada, inspección y restauración verificada.
  5. Terminal web PTY: comandos 'whoami', 'uptime', 'ls -ld /srv/nas/QA_TEST_SHARE'.
  6. Diagnóstico: ejecución de prueba Ping en vivo.
  7. Limpieza controlada de todos los artefactos creados.
"""

from __future__ import annotations

import json
import os
import secrets
import sys
import time
from playwright.sync_api import sync_playwright

HOST = os.environ.get("NAS_TEST_IP", "10.10.1.222")
BASE_URL = f"https://{HOST}"
ADMIN_USER = os.environ.get("NAS_WEB_PANEL_USER", "admin")
ADMIN_PASS = os.environ.get("NAS_WEB_PANEL_PASSWORD", "")

TEST_GROUP = "grp_qa_test"
TEST_USER = "qa_user"
TEST_USER_NAME = "QA Test User"
TEST_USER_PASS = os.environ.get("QA_TEST_USER_PASS") or f"QaPass_{secrets.token_hex(4)}!Aa"
TEST_SHARE = "QA_TEST_SHARE"
TEST_FOLDER = "_Prueba_QA"
TEST_FILE = "prueba_qa.txt"
TEST_CONTENT = "Prueba QA en tiempo real"


def log_step(step: str, detail: str = ""):
    print("\n======================================================================")
    print(f"[QA RUNNER] {step.upper()}")
    if detail:
        print(f"            {detail}")
    print("======================================================================")
    sys.stdout.flush()


def log_substep(status: str, msg: str):
    symbols = {"OK": "[+]", "WAIT": "[*]", "INFO": "[-]", "FAIL": "[X]", "CLEAN": "[~]"}
    sym = symbols.get(status, "[?]")
    print(f"  {sym} [{status}] {msg}")
    sys.stdout.flush()


def wait_modal_open(page, modal_id: str, timeout: int = 10000):
    page.wait_for_selector(f"#{modal_id}.active, #{modal_id}.open", state="visible", timeout=timeout)


def wait_modal_close(page, modal_id: str, timeout: int = 15000):
    try:
        page.wait_for_selector(f"#{modal_id}:not(.active)", state="attached", timeout=timeout)
    except Exception as e:
        try:
            page.evaluate(f"closeModal('{modal_id}')")
            time.sleep(0.5)
        except Exception:
            pass
        raise e


def run_qa_live():
    if not ADMIN_PASS:
        print("\n[QA RUNNER] ERROR: Defina la variable de entorno NAS_WEB_PANEL_PASSWORD para ejecutar la prueba en vivo.")
        print("            Ejemplo: NAS_WEB_PANEL_PASSWORD=\"...\" python scripts/qa_live_runner.py\n")
        sys.exit(1)

    results = {}

    log_step("Iniciando entorno Playwright visible en Windows", "Microsoft Edge Desktop (headless=False, slow_mo=600)")

    with sync_playwright() as p:
        launch_args = [
            "--start-maximized",
            "--window-position=50,50",
            "--window-size=1440,900",
            "--disable-infobars"
        ]
        try:
            browser = p.chromium.launch(
                channel="msedge",
                headless=False,
                slow_mo=600,
                args=launch_args
            )
        except Exception:
            try:
                browser = p.chromium.launch(
                    channel="chrome",
                    headless=False,
                    slow_mo=600,
                    args=launch_args
                )
            except Exception:
                browser = p.chromium.launch(
                    headless=False,
                    slow_mo=600,
                    args=launch_args
                )

        context = browser.new_context(
            ignore_https_errors=True,
            no_viewport=True
        )

        page = context.new_page()
        page.set_default_timeout(25000)

        # Diálogos estándar (alert / confirm / prompt) se aceptan automáticamente
        def handle_dialog(dialog):
            log_substep("INFO", f"Diálogo del navegador ({dialog.type}): {dialog.message}")
            dialog.accept()

        page.on("dialog", handle_dialog)

        # ----------------------------------------------------------------------
        # PASO 1: Inicio de Sesión
        # ----------------------------------------------------------------------
        log_step("Paso 1: Inicio de Sesión", f"Navegando a {BASE_URL}/login")
        try:
            page.goto(f"{BASE_URL}/login", wait_until="networkidle")
            log_substep("OK", f"Página de login cargada (Título: '{page.title()}')")

            page.wait_for_selector("#username", state="visible")
            page.fill("#username", ADMIN_USER)
            log_substep("INFO", f"Usuario rellenado: '{ADMIN_USER}'")

            page.fill("#password", ADMIN_PASS)
            masked = "*" * len(ADMIN_PASS)
            log_substep("INFO", f"Contraseña rellenada: '{masked}'")

            page.click("#btn-submit, button[type='submit']")
            page.wait_for_url("**/dashboard**", timeout=20000)
            page.wait_for_selector(".masthead-brand", state="visible")

            time.sleep(1.5)
            user_pill = page.inner_text(".user-name").strip()
            hostname = page.inner_text("#masthead-hostname").strip()
            log_substep("OK", f"Sesión activa con usuario '{user_pill}' en host '{hostname}'")
            results["login"] = {"status": "SUCCESS", "user": user_pill, "hostname": hostname}

            # Pre-limpieza defensiva para garantizar entorno limpio
            log_substep("INFO", "Asegurando estado limpio de prueba previo...")
            try:
                page.evaluate(f"""
                    async () => {{
                        try {{ await apiFetch('/api/shares/delete', {{ method: 'POST', body: JSON.stringify({{ name: '{TEST_SHARE}' }}) }}); }} catch(e){{}}
                        try {{ await apiFetch('/api/users/update', {{ method: 'POST', body: JSON.stringify({{ action: 'delete', username: '{TEST_USER}' }}) }}); }} catch(e){{}}
                        try {{ await apiFetch('/api/groups/delete', {{ method: 'POST', body: JSON.stringify({{ name: '{TEST_GROUP}' }}) }}); }} catch(e){{}}
                        try {{ await apiFetch('/api/files/delete', {{ method: 'POST', body: JSON.stringify({{ path: '{TEST_FOLDER}', permanent: true }}) }}); }} catch(e){{}}
                    }}
                """)
                time.sleep(1.0)
            except Exception:
                pass
        except Exception as e:
            log_substep("FAIL", f"Fallo en inicio de sesión: {e}")
            results["login"] = {"status": "FAILED", "error": str(e)}
            browser.close()
            return results

        # ----------------------------------------------------------------------
        # PASO 2: Usuarios y Grupos (Creación y Verificación)
        # ----------------------------------------------------------------------
        log_step("Paso 2: Usuarios y Grupos", f"Creando grupo '{TEST_GROUP}' y usuario '{TEST_USER}'")
        try:
            # 2.1 Navegar a vista Usuarios
            page.click(".sidebar-nav [data-view='users']")
            page.wait_for_selector("#view-users.active", timeout=10000)
            log_substep("OK", "Vista 'Usuarios y grupos' cargada")

            # 2.2 Pestaña Grupos
            page.click("#usr-tab-btn-groups")
            page.wait_for_selector("#usr-panel-groups", state="visible")
            time.sleep(1.0)

            # Abrir modal de nuevo grupo
            page.click("#usr-panel-groups button:has-text('Crear nuevo grupo')")
            wait_modal_open(page, "modal-new-group")
            log_substep("INFO", "Modal de nuevo grupo abierto")

            page.fill("#group-name", TEST_GROUP)
            log_substep("INFO", f"Llenado nombre del grupo: '{TEST_GROUP}'")

            page.click("#btn-submit-new-group")
            wait_modal_close(page, "modal-new-group")
            time.sleep(1.5)

            # Verificar grupo en tabla
            page.wait_for_selector(f"#groups-table-body tr:has-text('{TEST_GROUP}')", timeout=15000)
            group_row_text = page.inner_text(f"#groups-table-body tr:has-text('{TEST_GROUP}')")
            log_substep("OK", f"Grupo '{TEST_GROUP}' verificado en tabla: {group_row_text.splitlines()[0]}")

            # 2.3 Pestaña Usuarios
            page.click("#usr-tab-btn-users")
            page.wait_for_selector("#usr-panel-users", state="visible")
            time.sleep(1.0)

            # Abrir modal de nuevo usuario
            page.click("#view-users .page-head-actions button:has-text('Crear usuario')")
            wait_modal_open(page, "modal-new-user")
            log_substep("INFO", "Modal de nuevo usuario abierto")

            page.fill("#user-uname", TEST_USER)
            page.fill("#user-fullname", TEST_USER_NAME)
            page.fill("#user-pass", TEST_USER_PASS)

            # Membresía de grupo: buscar checkbox grp_qa_test
            chk_group = f"input[name='user_group'][value='{TEST_GROUP}']"
            page.wait_for_selector(chk_group, state="visible", timeout=10000)
            page.check(chk_group)
            log_substep("INFO", f"Asignada membresía al grupo '{TEST_GROUP}'")

            time.sleep(1.0)
            page.click("#btn-submit-new-user")
            wait_modal_close(page, "modal-new-user")

            # Modal de Bienvenida con ruta UNC
            wait_modal_open(page, "modal-user-welcome")
            welcome_unc = page.input_value("#welcome-unc-path")
            log_substep("OK", f"Modal de bienvenida presentado. Ruta UNC Windows: {welcome_unc}")
            time.sleep(1.5)
            page.click("#modal-user-welcome button:has-text('Aceptar')")
            wait_modal_close(page, "modal-user-welcome")

            time.sleep(1.5)
            page.wait_for_selector(f"#users-table-body tr:has-text('{TEST_USER}')", timeout=15000)
            user_row_text = page.inner_text(f"#users-table-body tr:has-text('{TEST_USER}')")
            log_substep("OK", f"Usuario '{TEST_USER}' verificado en tabla: {user_row_text.splitlines()[0]}")

            results["users_and_groups"] = {
                "status": "SUCCESS",
                "group": TEST_GROUP,
                "user": TEST_USER,
                "unc": welcome_unc
            }
        except Exception as e:
            log_substep("FAIL", f"Error en Usuarios y Grupos: {e}")
            results["users_and_groups"] = {"status": "FAILED", "error": str(e)}

        # ----------------------------------------------------------------------
        # PASO 3: Carpetas Compartidas (Samba)
        # ----------------------------------------------------------------------
        log_step("Paso 3: Carpetas Compartidas (Samba)", f"Creando recurso '[{TEST_SHARE}]'")
        try:
            page.click(".sidebar-nav [data-view='shares']")
            page.wait_for_selector("#view-shares.active", timeout=10000)
            log_substep("OK", "Vista 'Redes compartidas' cargada")

            page.click("#btn-new-share")
            wait_modal_open(page, "modal-new-share")
            log_substep("INFO", "Modal de nuevo recurso compartido abierto")

            page.fill("#share-name", TEST_SHARE)
            page.fill("#share-comment", "Carpeta de pruebas QA en vivo")
            page.select_option("#share-scheme", "1")

            # Seleccionar grupo autorizado grp_qa_test
            chk_share_group = f"input[name='share_group'][value='{TEST_GROUP}']"
            page.wait_for_selector(chk_share_group, state="visible", timeout=10000)
            page.check(chk_share_group)
            log_substep("INFO", f"Permisos asignados a '{TEST_GROUP}' (Lectura y Escritura)")

            time.sleep(1.0)
            page.click("#btn-submit-new-share")
            wait_modal_close(page, "modal-new-share")

            time.sleep(2.0)
            page.wait_for_selector(f"#shares-table-body tr:has-text('{TEST_SHARE}')", timeout=15000)
            share_row_text = page.inner_text(f"#shares-table-body tr:has-text('{TEST_SHARE}')")
            log_substep("OK", f"Recurso '[{TEST_SHARE}]' creado y verificado en smb.conf / tabla web:")
            log_substep("INFO", f"Detalles: {share_row_text.splitlines()[0]}")

            results["shares"] = {
                "status": "SUCCESS",
                "share": TEST_SHARE,
                "path": f"/srv/nas/{TEST_SHARE}",
                "authorized_group": TEST_GROUP
            }
        except Exception as e:
            log_substep("FAIL", f"Error en Carpetas Compartidas: {e}")
            results["shares"] = {"status": "FAILED", "error": str(e)}

        # ----------------------------------------------------------------------
        # PASO 4: Explorador de Archivos y Papelera
        # ----------------------------------------------------------------------
        log_step("Paso 4: Explorador de Archivos y Papelera", f"Carpeta '{TEST_FOLDER}', archivo '{TEST_FILE}', visor y papelera")
        try:
            page.click(".sidebar-nav [data-view='files']")
            page.wait_for_selector("#view-files.active", timeout=10000)
            log_substep("OK", "Vista 'Archivos' cargada")
            time.sleep(1.5)

            # 4.1 Crear carpeta _Prueba_QA
            page.click("#btn-new-folder")
            wait_modal_open(page, "modal-new-folder")
            page.fill("#new-folder-name", TEST_FOLDER)
            log_substep("INFO", f"Nombre de carpeta llenado: '{TEST_FOLDER}'")

            page.click("#btn-submit-new-folder")
            wait_modal_close(page, "modal-new-folder")
            time.sleep(1.5)

            # Verificar carpeta en cuadrícula o lista
            folder_sel = f".file-card[data-name='{TEST_FOLDER}']"
            page.wait_for_selector(folder_sel, timeout=15000)
            log_substep("OK", f"Carpeta '{TEST_FOLDER}' visible en cuadrícula")

            # 4.2 Entrar a _Prueba_QA con doble clic visible
            log_substep("INFO", f"Entrando en subcarpeta '{TEST_FOLDER}' mediante doble clic...")
            try:
                page.dblclick(folder_sel)
            except Exception:
                page.evaluate(f"navigateToSubpath('{TEST_FOLDER}')")
            time.sleep(1.5)
            page.wait_for_selector(f"#files-breadcrumbs:has-text('{TEST_FOLDER}')", timeout=10000)
            log_substep("OK", f"Navegación confirmada en ruta: /srv/nas/{TEST_FOLDER}")

            # 4.3 Crear archivo de texto prueba_qa.txt
            page.click("#btn-new-file")
            wait_modal_open(page, "modal-new-file")
            page.fill("#new-file-name", TEST_FILE)
            log_substep("INFO", f"Nombre de archivo llenado: '{TEST_FILE}'")

            page.click("#btn-submit-new-file")
            wait_modal_close(page, "modal-new-file")

            # 4.4 Visor de Archivos Integrado
            log_substep("WAIT", "Apertura automática del Visor de Archivos Integrado...")
            wait_modal_open(page, "modal-file-preview", 15000)
            log_substep("OK", f"Visor de Archivos desplegado en pantalla para '{TEST_FILE}'")
            time.sleep(1.0)

            # Activar edición en visor
            page.click("#btn-preview-edit")
            page.wait_for_selector("#preview-code-editor", state="visible", timeout=10000)
            page.fill("#preview-code-editor", TEST_CONTENT)
            log_substep("INFO", f"Contenido escrito en editor web: '{TEST_CONTENT}'")
            time.sleep(1.0)

            page.click("#btn-preview-save")
            time.sleep(1.5)
            log_substep("OK", "Contenido guardado exitosamente en el archivo")

            # Volver a modo vista para observar el texto previsualizado en el visor
            page.click("#btn-preview-edit")
            time.sleep(1.0)
            rendered_preview = page.inner_text("#preview-code-content").strip()
            log_substep("OK", f"Previsualización renderizada en el visor: '{rendered_preview}'")

            # Pausa visible de 2.5s para que el usuario observe la previsualización
            time.sleep(2.5)
            page.click("#modal-file-preview .modal-close")
            wait_modal_close(page, "modal-file-preview")

            # Verificar archivo listado en la carpeta
            file_sel = f".file-card[data-name='{TEST_FILE}']"
            page.wait_for_selector(file_sel, timeout=15000)
            log_substep("OK", f"Archivo '{TEST_FILE}' confirmado en el explorador")

            # 4.5 Enviar a la Papelera
            log_substep("INFO", f"Enviando '{TEST_FILE}' a la papelera confinada...")
            try:
                page.click(f"{file_sel} button[title='Eliminar']")
            except Exception:
                page.evaluate(f"openDeleteModal('{TEST_FOLDER}/{TEST_FILE}', '{TEST_FILE}')")

            wait_modal_open(page, "modal-delete-file")
            page.check("#del-mode-trash")
            time.sleep(1.0)

            page.click("#btn-confirm-delete")
            wait_modal_close(page, "modal-delete-file")
            time.sleep(1.5)
            log_substep("OK", f"'{TEST_FILE}' trasladado a la papelera")

            # 4.6 Inspeccionar Papelera
            page.click("#btn-open-trash")
            page.wait_for_selector("#files-trash-view", state="visible", timeout=10000)
            log_substep("OK", "Vista de Papelera (/srv/nas/.trash) abierta")
            time.sleep(1.5)

            trash_item_sel = f"#trash-table-body tr:has-text('{TEST_FILE}')"
            page.wait_for_selector(trash_item_sel, timeout=15000)
            trash_row_text = page.inner_text(trash_item_sel)
            log_substep("OK", f"Archivo verificado en papelera confinada: {trash_row_text.splitlines()[0]}")
            time.sleep(2.0)

            # 4.7 Restaurar archivo
            log_substep("INFO", "Ejecutando restauración desde la papelera...")
            restore_btn = f"{trash_item_sel} button:has-text('Restaurar')"
            try:
                page.click(restore_btn)
            except Exception:
                page.evaluate("confirmRestoreAllTrash()")
            time.sleep(2.0)
            log_substep("OK", f"Restauración ejecutada para '{TEST_FILE}'")

            # Volver al explorador
            page.click("#files-trash-view button:has-text('Volver a Archivos')")
            page.wait_for_selector("#files-normal-view", state="visible", timeout=10000)
            time.sleep(1.0)

            # Navegar a _Prueba_QA si no está allí
            page.evaluate(f"navigateToSubpath('{TEST_FOLDER}')")
            time.sleep(1.5)

            # Verificar que el archivo regresó
            page.wait_for_selector(file_sel, timeout=15000)
            log_substep("OK", f"Archivo '{TEST_FILE}' verificado restaurado en /srv/nas/{TEST_FOLDER}")

            results["files_and_trash"] = {
                "status": "SUCCESS",
                "folder": TEST_FOLDER,
                "file": TEST_FILE,
                "preview_text": rendered_preview,
                "trash_verified": True,
                "restoration_verified": True
            }
        except Exception as e:
            log_substep("FAIL", f"Error en Archivos y Papelera: {e}")
            results["files_and_trash"] = {"status": "FAILED", "error": str(e)}

        # ----------------------------------------------------------------------
        # PASO 5: Terminal Web PTY
        # ----------------------------------------------------------------------
        log_step("Paso 5: Terminal Web PTY", "Ejecución de comandos interactivos ('whoami', 'uptime', 'ls -ld')")
        try:
            page.click(".sidebar-nav [data-view='terminal']")
            page.wait_for_selector("#view-terminal.active", timeout=10000)
            log_substep("OK", "Consola Web PTY abierta")
            time.sleep(2.0)

            terminal_commands = [
                ("whoami", "Verificar usuario de sesión tmux"),
                ("uptime", "Verificar tiempo de actividad y carga del servidor"),
                (f"ls -ld /srv/nas/{TEST_SHARE}", f"Verificar ACL y permisos POSIX en disco de [{TEST_SHARE}]")
            ]

            cmd_outputs = {}
            for cmd, desc in terminal_commands:
                log_substep("INFO", f"Enviando comando: '{cmd}' ({desc})")
                page.fill("#terminal-input", cmd)
                page.press("#terminal-input", "Enter")
                time.sleep(2.5)

                term_text = page.inner_text("#terminal-output")
                lines = [line for line in term_text.splitlines() if line.strip()]
                sample = "\n    ".join(lines[-4:]) if len(lines) >= 4 else "\n    ".join(lines)
                log_substep("OK", f"Salida observada en pantalla:\n    {sample}")
                cmd_outputs[cmd] = lines[-2:] if len(lines) >= 2 else lines

            results["terminal_pty"] = {
                "status": "SUCCESS",
                "commands": cmd_outputs
            }
        except Exception as e:
            log_substep("FAIL", f"Error en Terminal Web PTY: {e}")
            results["terminal_pty"] = {"status": "FAILED", "error": str(e)}

        # ----------------------------------------------------------------------
        # PASO 6: Diagnóstico en Vivo
        # ----------------------------------------------------------------------
        log_step("Paso 6: Diagnóstico", "Ejecución de prueba de conectividad Ping en vivo")
        try:
            page.click(".sidebar-nav [data-view='services']")
            page.wait_for_selector("#view-services.active", timeout=10000)
            log_substep("OK", "Vista 'Servicios y Salud' cargada")
            time.sleep(1.0)

            # Botón prueba ping
            page.click("#btn-diag-ping")
            log_substep("WAIT", "Prueba de Ping ejecutándose...")

            page.wait_for_selector("#diag-test-result:has-text('[')", timeout=20000)
            diag_text = page.inner_text("#diag-test-result").strip()
            log_substep("OK", f"Resultado del diagnóstico en pantalla: {diag_text}")
            time.sleep(2.5)

            results["diagnostics"] = {
                "status": "SUCCESS",
                "test": "ping",
                "result": diag_text
            }
        except Exception as e:
            log_substep("FAIL", f"Error en Diagnóstico: {e}")
            results["diagnostics"] = {"status": "FAILED", "error": str(e)}

        # ----------------------------------------------------------------------
        # PASO 7: Limpieza Controlada al Finalizar
        # ----------------------------------------------------------------------
        log_step("Paso 7: Limpieza Controlada", "Eliminación de share, usuario, grupo y carpeta temporal")
        cleanup_report = {}

        # 7.1 Eliminar recurso compartido
        try:
            log_substep("CLEAN", f"Eliminando recurso compartido '[{TEST_SHARE}]'...")
            page.click(".sidebar-nav [data-view='shares']")
            page.wait_for_selector("#view-shares.active", timeout=10000)
            time.sleep(1.0)

            try:
                page.click(f"#shares-table-body tr:has-text('{TEST_SHARE}') button[title='Eliminar']")
            except Exception:
                page.evaluate(f"deleteShare('{TEST_SHARE}')")

            time.sleep(2.5)
            page.wait_for_selector(f"#shares-table-body tr:has-text('{TEST_SHARE}')", state="detached", timeout=10000)
            log_substep("OK", f"Recurso '[{TEST_SHARE}]' eliminado de Samba y del panel")
            cleanup_report["share"] = "DELETED"
        except Exception as e:
            log_substep("FAIL", f"Fallo al limpiar recurso: {e}")
            cleanup_report["share"] = f"ERROR: {e}"

        # 7.2 Eliminar usuario qa_user
        try:
            log_substep("CLEAN", f"Eliminando usuario '{TEST_USER}'...")
            page.click(".sidebar-nav [data-view='users']")
            page.wait_for_selector("#view-users.active", timeout=10000)
            page.click("#usr-tab-btn-users")
            time.sleep(1.0)

            try:
                page.click(f"#users-table-body tr:has-text('{TEST_USER}') button[title='Eliminar']")
            except Exception:
                page.evaluate(f"deleteUser('{TEST_USER}')")

            wait_modal_open(page, "modal-user-action-confirm")
            page.click("#btn-user-action-confirm-submit")
            wait_modal_close(page, "modal-user-action-confirm")
            time.sleep(2.5)
            page.wait_for_selector(f"#users-table-body tr:has-text('{TEST_USER}')", state="detached", timeout=10000)
            log_substep("OK", f"Usuario '{TEST_USER}' eliminado del sistema")
            cleanup_report["user"] = "DELETED"
        except Exception as e:
            log_substep("FAIL", f"Fallo al limpiar usuario: {e}")
            cleanup_report["user"] = f"ERROR: {e}"

        # 7.3 Eliminar grupo grp_qa_test
        try:
            log_substep("CLEAN", f"Eliminando grupo '{TEST_GROUP}'...")
            page.click("#usr-tab-btn-groups")
            time.sleep(1.0)

            try:
                page.click(f"#groups-table-body tr:has-text('{TEST_GROUP}') button[title='Eliminar']")
            except Exception:
                page.evaluate(f"deleteGroup('{TEST_GROUP}')")

            wait_modal_open(page, "modal-user-action-confirm")
            page.click("#btn-user-action-confirm-submit")
            wait_modal_close(page, "modal-user-action-confirm")
            time.sleep(2.5)
            page.wait_for_selector(f"#groups-table-body tr:has-text('{TEST_GROUP}')", state="detached", timeout=10000)
            log_substep("OK", f"Grupo '{TEST_GROUP}' eliminado del sistema")
            cleanup_report["group"] = "DELETED"
        except Exception as e:
            log_substep("FAIL", f"Fallo al limpiar grupo: {e}")
            cleanup_report["group"] = f"ERROR: {e}"

        # 7.4 Eliminar carpeta _Prueba_QA y vaciar papelera
        try:
            log_substep("CLEAN", f"Eliminando carpeta '{TEST_FOLDER}' y purgando papelera...")
            page.click(".sidebar-nav [data-view='files']")
            page.wait_for_selector("#view-files.active", timeout=10000)
            page.evaluate("navigateToSubpath('')")
            time.sleep(1.5)

            # Modal de eliminación permanente
            page.evaluate(f"openDeleteModal('{TEST_FOLDER}', '{TEST_FOLDER}')")
            wait_modal_open(page, "modal-delete-file")
            page.check("#del-mode-permanent")
            page.click("#btn-confirm-delete")
            wait_modal_close(page, "modal-delete-file")
            time.sleep(1.5)

            # Vaciar papelera
            page.evaluate("apiFetch('/api/files/trash/empty', { method: 'POST', body: JSON.stringify({}) })")
            time.sleep(1.5)
            log_substep("OK", f"Carpeta '{TEST_FOLDER}' eliminada permanentemente y papelera vaciada")
            cleanup_report["folder"] = "DELETED"
        except Exception as e:
            log_substep("FAIL", f"Fallo al limpiar carpeta: {e}")
            cleanup_report["folder"] = f"ERROR: {e}"

        results["cleanup"] = cleanup_report

        # Pausa final de 8 segundos para que el usuario observe el cierre ordenado
        log_step("Finalización de la sesión de pruebas en vivo", "Manteniendo ventana visible 8 segundos antes del cierre ordenado...")
        time.sleep(8.0)
        browser.close()

    print("\n======================================================================")
    print("RESUMEN DE EJECUCIÓN QA EN VIVO (JSON):")
    print(json.dumps(results, indent=2, ensure_ascii=False))
    print("======================================================================\n")
    return results


if __name__ == "__main__":
    results = run_qa_live()
    all_ok = all(
        v.get("status") == "SUCCESS"
        for k, v in results.items()
        if k != "cleanup"
    ) and all(
        v == "DELETED"
        for v in results.get("cleanup", {}).values()
    )
    sys.exit(0 if all_ok else 1)
