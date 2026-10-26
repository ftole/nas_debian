# Contrato de paridad PHP ↔ Rust

> Objetivo: garantizar que el panel Rust sirve **la página real**, no una
> reinterpretación. Este contrato se genera automáticamente y se valida en CI.

## 1. Fuente de verdad

El panel PHP (`web/`) es la referencia hasta la firma de paridad:

| Archivo | Rol | Hash (F0) |
| :--- | :--- | :--- |
| `web/templates/layout.php` | Estructura del panel (masthead, sidebar, 14 vistas) | `assets.sha256` |
| `web/templates/login.php` | Pantalla de acceso y su CSS | `assets.sha256` |
| `web/public/css/app.css` | Identidad visual (Slate UI, temas) | `assets.sha256` |
| `web/public/js/app.js` | Comportamiento del panel (5.400 líneas) | `assets.sha256` |
| `web/public/js/vendor/*` | Visores offline (xlsx, jszip, docx-preview) | `assets.sha256` |

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
| `tests/parity/contract.json` | IDs, clases, `data-*`, globales `window.NAS_*`, formularios | 527 IDs · 156 clases |
| `tests/parity/selectors.txt` | Selectores e IDs que `app.js` exige en el DOM | 340 IDs · 30 selectores |
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
| **F3** | 100 % de `endpoints.txt` implementado con contrato equivalente |
| **F4** | Checklist funcional de las 14 vistas sin errores de consola |
| **F5** | CI ejecuta `parity_extract.py --check` y la suite DOM/selectores |
| **F6** | Comparación en vivo PHP vs Rust antes de retirar `web/` a `legacy/` |

### F4 — Verificación funcional (completada)

`scripts/qa_ui_audit.py` audita la interfaz portada en dos modos:

| Modo | Comando | Cobertura |
| :--- | :--- | :--- |
| Estático (CI) | `python3 scripts/qa_ui_audit.py --static` | 340 IDs de `app.js`, 115 manejadores inline, 14 vistas frente a `layout.html` |
| En vivo | `NAS_WEB_PANEL_PASSWORD=… python3 scripts/qa_ui_audit.py` | lo anterior + 24 endpoints GET de carga de vista |

Resultados en `10.10.1.222`:

```text
[ids]      app.js consulta 340 IDs; faltan 0
[handlers] 115 inline; sin definición: 0
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
