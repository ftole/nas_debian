# Contrato de paridad PHP ↔ Rust

> Objetivo: garantizar que el panel Rust sirve **la página real**, no una
> reinterpretación. Este contrato se genera automáticamente y se valida en CI.

## 1. Fuente de verdad

El panel PHP (`web/`) es la referencia hasta la firma de paridad:

| Archivo | Rol | Hash (F0) |
| :--- | :--- | :--- |
| `tests/parity/fixtures/layout.php` | Estructura original del panel (masthead, sidebar, 14 vistas) | `assets.sha256` |
| `tests/parity/fixtures/login.php` | Pantalla de acceso original | `assets.sha256` |
| `crates/nas-web/assets/app.css` | Identidad visual (Slate UI, temas) | `assets.sha256` |
| `crates/nas-web/assets/js/app.js` | Comportamiento del panel (5.400 líneas) | `assets.sha256` |
| `crates/nas-web/assets/js/vendor/*` | Visores offline (xlsx, jszip, docx-preview) | `assets.sha256` |
| `crates/nas-web/assets/{login.css,icons.svg,favicon.svg}` | Estilos de acceso e iconografía | `assets.sha256` |

**Regla dura:** `app.css`, `app.js` y vendor se sirven **byte a byte**. Si un
cambio fuera imprescindible, se documenta como excepción y se actualiza el hash
en el mismo commit que lo justifica.

## 2. Artefactos generados

```bash
python3 scripts/parity_extract.py           # regenera
python3 scripts/parity_extract.py --check   # falla si hay deriva (CI)
```

| Archivo | Contenido | Inventario actual |
| :--- | :--- | :--- |
| `tests/parity/contract.json` | IDs, clases, `data-*`, globales `window.NAS_*`, formularios | 547 IDs · 159 clases |
| `tests/parity/selectors.txt` | Selectores e IDs que `app.js` exige en el DOM | 382 IDs · 33 selectores |
| `tests/parity/endpoints.txt` | Endpoints `/api` consumidos por el panel | 60 |
| `tests/parity/assets.sha256` | Hashes y tamaños de los assets originales | 8 archivos |

Globales que el HTML debe seguir inyectando:
`NAS_IS_ADMIN`, `NAS_IS_SUPER`, `NAS_ROLE`, `NAS_SERVER_ROLE`,
`NAS_INITIAL_SERVER_TIME`.

Atributos de datos que el JS consulta:
`data-view`, `data-modal-alias`, `data-role`, `data-perm`, `data-source`,
`data-filter`, `data-theme`.

## 3. Qué significa "idéntico"

1. **DOM:** mismas etiquetas, mismos `id`, mismas clases, mismos `data-*` y mismo
   orden estructural en cada una de las 14 vistas y en el login.
2. **Assets:** CSS, JS, vendor y favicon con hash idéntico.
3. **API:** mismas rutas, métodos y forma de respuesta
   (`success` / `message` / `data` / `error`), incluidos los nombres de campo.
4. **Flujos:** login/logout con `csrf_token`, notificaciones, modales, toasts,
   drag-and-drop, previsualización Office, explorador con ZIP y terminal.

## 4. Normalización permitida en el diff DOM

| Elemento volátil | Normalización |
| :--- | :--- |
| Valor de `csrf_token` | `<TOKEN>` |
| Reloj, hora del servidor y fechas | `<TIMESTAMP>` |
| Métricas (CPU, RAM, uso de disco, uptime) | `<METRICA>` |
| IDs de sesión y `PHPSESSID` | `<SESION>` |
| `?v=<filemtime>` de CSS/JS | `?v=<HASH>` |
| Espacios entre etiquetas | colapsados |
| Orden de atributos | equivalente |

Todo lo demás debe coincidir exactamente.

## 5. Puertas de aceptación

| Fase | Puerta |
| :--- | :--- |
| **F0** (hecha) | `contract.json`, `selectors.txt`, `endpoints.txt` y `assets.sha256` generados y verificados por `--check` |
| **F1** | Diff DOM normalizado vacío en las 14 vistas + login |
| **F2** | Hash del `app.js` servido == hash de `assets.sha256` |
| **F3** | 77 endpoints (incluidas rutas dinámicas `:id`) implementados con contrato equivalente |
| **F4** | Checklist funcional de las 14 vistas sin errores de consola |
| **F5** | CI ejecuta `parity_extract.py --check`, la suite DOM/selectores y `tests/parity.rs` |
| **F6** | Comparación en vivo PHP vs Rust antes de retirar `web/` a `legacy/` (completada) |
| **F7** | Ola A ejecutada: panel PHP/CLI/asistente retirados en su totalidad (referencia en tag `legacy-php-final`). Ola B pendiente (`deploy.sh`/`uninstall.sh`) |

### F5 — Suite de paridad en CI (completada)

`crates/nas-web/tests/parity.rs` valida el HTML servido contra
`tests/parity/contract.json` y `endpoints.txt` en cada `cargo test`:

| Prueba | Comprobación |
| :--- | :--- |
| `dashboard_preserva_ids_clases_y_globales` | 547 IDs, 159 clases, atributos `data-*`, globales y 14 secciones `view-*` |
| `login_preserva_ids_y_formulario` | IDs del formulario de acceso y campo CSRF |
| `todos_los_identificadores_de_app_js_existen` | 382 IDs consultados por `app.js` |
| `endpoints_del_contrato_existen_y_no_son_501` | 60 rutas: ninguna desaparece ni queda sin implementar |

Además el flujo `.github/workflows/rust.yml` ejecuta
`python3 scripts/parity_extract.py --check` y `python3 scripts/qa_ui_audit.py --static`.

Con F5, **no queda ningún endpoint en 501**: se implementaron
`POST /api/system/update/run` (actualizador validado por sudoers) y el alias
`POST /api/backups/`.

### F6 — Comparación en vivo PHP vs Rust (completada)

> [!NOTE]
> El panel PHP ya fue retirado del servidor y del árbol de trabajo (F7.2). Para
> reproducir la comparación se recupera desde la etiqueta `legacy-php-final` o desde
> `/root/nas-deploy-backup/retire-<fecha>/www-nas-web.tar.gz`, en un servidor de pruebas.
> El contrato (`tests/parity/`) y las pruebas `crates/nas-web/tests/parity.rs`
> siguen ejecutándose en cada `cargo test`.

Procedimiento (`scripts/php_parity_diff.py`, ejecutado en el servidor):

1. Reponer el panel PHP en un servidor de pruebas en `127.0.0.1:8081`
   (nginx + `php-fpm`, raíz de la release `legacy-php-final`). En producción ya está retirado.
2. Comparar con el panel Rust en `https://127.0.0.1` (cookie `Secure`) usando una
   cuenta del sistema con acceso Samba para PHP y una cuenta de `web_users` para Rust.
3. Comparar por página IDs, clases, atributos `data-*`, recuentos y orden de secciones;
   después la forma de las respuestas JSON de 21 endpoints.

Resultado en `<host>`:

```text
/dashboard, /files, /shares, /users, /backups, /storage, /terminal, /domain, /services
  → idéntico (547 IDs por página)

21 endpoints → misma forma
/api/audit   → solo en Rust (no existe en PHP; mejora, no regresión)
```

Como consecuencia se alinearon las respuestas del panel Rust con el contrato que
consume `app.js`:

| Endpoint | Ajuste |
| :--- | :--- |
| `/api/metrics` | `system`, `storage`, `services[]` y `counts` anidados |
| `/api/storage` | `overview` + `disks[]` con `type`, `mount`, `children_count` |
| `/api/storage/units` | unidad principal derivada de `overview_for` |
| `/api/services` | nombre legible y etiqueta `activo/inactivo` |
| `/api/diagnostics` | objeto con `overall_status`, contadores y `checks[]` |
| `/api/files` | `current_path`, `breadcrumbs[]`, `items[]` con permisos y dueño |
| `/api/files/trash` | `total`, `total_size_bytes` |
| `/api/backups` | proyección plana (`cron`, `dest_path`, `last_status`, …) |
| `/api/logs` | `user`, `ip`, `status`, `target`, `raw`, `count` |
| `/api/domain/status` | `status`, `workgroup`, `message` |
| `/api/system/*`, `/api/network`, `/api/shares`, `/api/users`, `/api/groups` | campos faltantes |

También se corrigió la fuga de marcadores `%active_group_*%` en las clases CSS y se
detecta Btrfs en `/proc/mounts` para mostrar los controles de scrub.

### F4 — Verificación funcional (completada)

`scripts/qa_ui_audit.py` audita la interfaz portada en dos modos:

| Modo | Comando | Cobertura |
| :--- | :--- | :--- |
| Estático (CI) | `python3 scripts/qa_ui_audit.py --static` | 382 IDs de `app.js`, 127 manejadores inline, 14 vistas frente a `layout.html` |
| En vivo | `NAS_WEB_PANEL_PASSWORD=… python3 scripts/qa_ui_audit.py` | lo anterior + 24 endpoints GET de carga de vista |

Resultados en `<host>`:

```text
[ids]      app.js consulta 382 IDs; faltan 0
[handlers] 127 inline; sin definición: 0
[vistas]   14 secciones; nav sin sección: 0
[api]      24 endpoints GET verificados
```

Pruebas de carga útil (payload) ejecutadas contra el panel:

| Flujo | Resultado |
| :--- | :--- |
| Archivos: crear, guardar, mover, papelera, restaurar, vaciar | OK |
| Papelera anidada restaurada a su ruta original (`ZZQA_F/sub/nota.txt`) | OK |
| Matriz de acceso Samba (`POST /api/shares/access`) | ACL aplicada en 0,14 s (`g:grp_samba:rwx` + default) |
| Diagnóstico (`POST /api/diagnostics/run`) | `status: success` |
| Dominio: descubrimiento con nombre inválido | `400 dominio inválido` |
| Respaldos: crear, probar, listar, ejecutar, eliminar | OK (F3) |

Correcciones derivadas de F4:

* `nas-share-acl` aplica ACL **no recursivas** en la raíz del recurso más `default`
  (antes recorría millones de inodos y bloqueaba la petición).
* La papelera guarda la **ruta relativa original** en `.trash/.meta/<id>` y restaura
  al directorio de origen creando los niveles necesarios.

## 6. Regla de trabajo

- Primero **portar**, después mejorar. Cualquier mejora visual o funcional va en
  un commit aparte, con su propio criterio de aceptación.
- Está prohibido introducir una vista nueva que no provenga de `layout.php`.
- `nas-web` solo renderiza plantillas; no define estilos ni comportamiento
  propios mientras dure la paridad.
