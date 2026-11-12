# Modularización de la Parte Web (Fase 0 — Inventario y Ownership)

> Documento de trabajo para la modularización de `crates/nas-web`.
> No modifica código; define el mapa de ownership por dominio.

## 1. Estado actual

- `crates/nas-web/assets/js/app.js`: **8.636 líneas**, monolítico.
- `crates/nas-web/templates/layout.html`: **3.343 líneas**, monolítico.
- Backend (`routes/*.rs`): ya modular por dominio.

## 2. Mapa de secciones en `app.js` (ownership propuesto)

| Sección (líneas aprox.) | Dominio | Módulo destino | Owner |
|---|---|---|---|
| 1. Estado Global y Utilidades UI (10-308) | Core/UI | `lib/utils.js`, `lib/toast.js`, `lib/modal.js` | — |
| 2. Navegación entre Vistas (310-463) | Core | `app.js` (orquestador) | — |
| 3. Clientes API y Autenticación (465-536) | Core/API | `lib/api.js` | — |
| 4. Dashboard & Métricas (538-697) | Dashboard | `modules/dashboard.js` | — |
| 5. Recursos Compartidos Samba (699-1002) | Shares | `modules/shares.js` | — |
| 6. Central de Respaldos (1004-1818) | Backups | `modules/backups.js` | — |
| 7. Almacenamiento & Mantenimiento (1820-2776) | Storage | `modules/storage.js` | — |
| 8. Usuarios & Grupos (2778-3621) | Users | `modules/users.js` | — |
| 9. Servicios y Demonios (4038-4128) | Services | `modules/services.js` | — |
| 10. Registros del Sistema (4129-4363) | Logs/Audit | `modules/logs.js`, `modules/audit.js` | — |
| 11. Terminal Web (4364-4623) | Terminal | `modules/terminal.js` | — |
| 12. Actualizaciones y Reinicio (4624-5181) | System | `modules/system.js` | — |
| 13. Utilidades Generales (5182-5375) | Core | `lib/utils.js` | — |
| 15. Explorador de Archivos (5376-6798) | Files | `modules/files.js` | — |
| Papelera de Reciclaje (6799-7106) | Files | `modules/files.js` | — |
| Previsualizador (7107-7745) | Files | `modules/files.js` | — |
| 16. Active Directory (7746-8048) | Domain | `modules/domain.js` | — |
| 15. Inicialización DOM (8049-8259) | Core | `app.js` (orquestador) | — |
| 15. Notificaciones (8260-8387) | Core | `lib/toast.js` | — |
| 16. Aliases Globales (8388-8636) | Core | `app.js` (orquestador) | — |

## 3. Estructura objetivo

```
crates/nas-web/assets/js/
  app.js              # orquestador (init, routing, estado compartido)
  lib/
    api.js
    modal.js
    utils.js
    toast.js
  modules/
    dashboard.js
    shares.js
    backups.js
    storage.js
    users.js
    services.js
    logs.js
    audit.js
    terminal.js
    system.js
    files.js
    domain.js
    applications.js
    networking.js
```

## 4. Reglas de coexistencia (modularización)

- Cada módulo tiene **un owner**; no se edita un módulo ajeno sin coordinación.
- Commits atómicos por módulo.
- `app.js` se mantiene intacto hasta que todos los módulos estén probados.
- Paridad: actualizar `parity_extract.py` y `qa_ui_audit.py` para escanear todos los `.js` recursivos.
- No introducir bundlers en runtime; carga vía `<script src>` en orden: `lib/` → `modules/` → `app.js`.

## 5. Fases siguientes

- **Fase 1:** extraer helpers (`lib/`).
- **Fase 2:** extraer dominios (`modules/`).
- **Fase 3:** limpiar `app.js` (solo orquestador).
- **Fase 4:** validación completa + deploy.

## 6. Decisiones pendientes

- Asignar owners por dominio.
- Confirmar si se mantiene `layout.html` monolítico o se evalúan parciales (Fase 4).