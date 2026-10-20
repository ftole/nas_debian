# Propuesta de Experiencia de Usuario (UX) • Módulo de Usuarios y Grupos

> [!IMPORTANT]
> Este documento contiene la **propuesta técnica y conceptual de rediseño de Experiencia de Usuario (UX)** para el módulo de Usuarios, Grupos y Permisos del panel web administrativo de **NAS Debian 13**.
> **Nota de Gobernanza de Código:** Esta especificación es estrictamente documental y no altera el código operativo existente de `UserController.php`, `UserService.php` ni los controladores de usuarios actuales.

---

## 1. Diagnóstico Actual y Oportunidades de Mejora

En la arquitectura actual del panel administrativo, la gestión de usuarios y grupos se presenta en dos tablas paralelas (`nas-table`) dentro de una grilla de dos columnas (`card-grid-2`), acompañadas de una matriz de permisos tabular en la parte inferior:

```
┌───────────────────────────────────────┬───────────────────────────────────────┐
│        Tabla 1: Usuarios (7 cols)     │         Tabla 2: Grupos (4 cols)      │
│  - Usuario / UID / Rol                │  - Nombre de Grupo (grp_*)            │
│  - Grupos asignados (badges de texto) │  - GID                                │
│  - Estado / Samba Sync                │  - Lista de miembros de texto         │
│  - Acciones (Editar/Clave/Bloq/Elim)  │  - Acciones (Editar/Eliminar)         │
└───────────────────────────────────────┴───────────────────────────────────────┘
                                        ▼
┌───────────────────────────────────────────────────────────────────────────────┐
│              Matriz de Permisos (Grupos × Recursos / Usuarios × Recursos)      │
│  - Celdas con clics para conmutar: Denegado ➔ Solo Lectura ➔ Lectura/Escritura│
└───────────────────────────────────────────────────────────────────────────────┘
```

### Puntos de Fricción Identificados:
1. **Mezcla visual de naturalezas de cuentas:** Las cuentas del sistema y administración del NAS (`root`, `sistemas`, `administrador`, cuentas con acceso web `can_web`) conviven en la misma lista con las cuentas exclusivas de almacenamiento departamental (personal contable, administrativo, ventas). Esto incrementa el riesgo de manipulación accidental de cuentas críticas.
2. **Asignación de grupos engorrosa:** Al editar o crear un usuario, los grupos se asignan mediante un campo de texto plano separado por comas o mediante casillas de verificación estáticas, lo cual resulta lento cuando la organización escala a decenas de grupos (`grp_contabilidad`, `grp_ventas`, `grp_recursoshumanos`, etc.).
3. **Falta de visibilidad de impacto en tiempo real:** Un administrador que agrega a un usuario a `grp_contabilidad` no ve de inmediato qué carpetas compartidas (`[CONTABILIDAD$]`, `[CAMPANA_UNO_REPORTES]`) quedarán accesibles ni con qué nivel de permisos (Lectura o Escritura).
4. **Densidad cognitiva en pantallas intermedias:** En portátiles o tablets con resoluciones de 1366×768 o 1080p, tener dos tablas anchas lado a lado satura el ancho útil de la ventana.

---

## 2. Pilares de la Nueva Experiencia Propuesta

La propuesta adopta los principios de diseño de **Slate UI** (100% offline, cero librerías externas, CSS variables puras) estructurada sobre cuatro pilares interactivos:

```
                  ┌───────────────────────────────────────────────┐
                  │    ARQUITECTURA DE EXPERIENCIA REDISEÑADA     │
                  └───────────────────────┬───────────────────────┘
                                          │
        ┌─────────────────────────────────┼─────────────────────────────────┐
        ▼                                 ▼                                 ▼
┌───────────────────────┐       ┌───────────────────┐       ┌───────────────────────┐
│  Segmentación Visual  │       │ Asignación Fluida │       │  Previsualizador en   │
│   por Naturaleza de   │       │   por Chips y     │       │   Tiempo Real de      │
│        Cuenta         │       │  Drag-and-Drop    │       │     Accesos Samba     │
└───────────────────────┘       └───────────────────┘       └───────────────────────┘
```

---

## 3. Especificación Detallada de Componentes

### 3.1 Segmentación Visual de Cuentas (Admin vs Almacenamiento NAS)

En lugar de una sola tabla mezclada, la vista incorporará una barra superior de segmentación semántica que clasifica las cuentas mediante tarjetas de resumen (*KPI Insight Cards*) y pestañas de filtrado contextual:

* **Pestaña 1: Operadores y Administradores del Sistema (Cuentas Privilegiadas):**
  * Incluye: Cuentas con roles `superadmin`, `admin` y `operator` (`grp_web`, `sudo`, `sistemas`).
  * Indicadores visuales: Insignia dorada/ámbar para Superadministrador, insignia azul cobalto para Administrador del Servidor, e insignia índigo para Operador Web.
  * Protección reforzada: Se resalta visualmente el bloqueo de eliminación de cuentas maestras (`root`, `sistemas`, `grp_superadmin`) con candados SVG interactivos y desactivación inmediata de botones destructivos.
* **Pestaña 2: Usuarios Departamentales de Almacenamiento (Samba & Dominio AD):**
  * Incluye: Cuentas exclusivas de carpetas compartidas (`grp_samba`, empleados, estaciones de trabajo).
  * Foco de diseño: Indicador claro de sincronización de contraseña con el demonio de Samba (`smbpasswd` sincronizado en verde, desincronizado en naranja), estado de pertenencia al dominio Active Directory y cuota asignada.

#### Mockup Estructural (HTML/CSS):
```html
<div class="user-category-tabs">
  <button class="category-tab active" data-category="nas">
    <svg class="icon"><use href="#icon-users"></use></svg>
    <span>Usuarios de Almacenamiento NAS</span>
    <span class="tab-counter">24</span>
  </button>
  <button class="category-tab" data-category="admin">
    <svg class="icon"><use href="#icon-shield"></use></svg>
    <span>Administradores y Operadores Web</span>
    <span class="tab-counter">3</span>
  </button>
</div>
```

---

### 3.2 Asignación Dinámica por Chips y Drag-and-Drop

Para eliminar los formularios rígidos de texto plano, se propone un componente de asignación fluida:

1. **Chips con Descarte Rápido (`Tag Chips`):**
   * Cada grupo asignado al usuario se visualiza como un *chip* estilizado (`.badge-chip`) con el color representativo del departamento y una cruz de descarte (`&times;`) para remoción en un solo clic.
   * Al hacer clic en el contenedor, se despliega un menú desplegable con búsqueda incremental instantánea que filtra los grupos disponibles con prefijo `grp_*`.
2. **Modo Drag-and-Drop Opcional (Tablero Visual de Grupos):**
   * Vista de cuadrícula opcional donde los usuarios se representan como tarjetas compactas arrastrables (*Draggable Cards*).
   * Al arrastrar la tarjeta de un usuario sobre el recuadro de un grupo (por ejemplo, soltar a `usuario.contabilidad` sobre `grp_contabilidad`), se efectúa la adición inmediata con feedback háptico visual (resplandor esmeralda `.card-drop-glow`) y llamada en segundo plano a la API REST (`POST /api/users/assign-groups`).
   * Cumple con la directiva **100% Offline**: implementado exclusivamente mediante la API nativa HTML5 Drag and Drop (`dragstart`, `dragover`, `drop`), sin dependencias de SortableJS ni librerías externas.

---

### 3.3 Previsualizador en Tiempo Real de Accesos Samba (*Live Share Access Preview*)

Uno de los problemas más comunes en la administración de redes es la incertidumbre sobre qué carpetas verá un usuario tras modificar sus grupos. 

Se incorpora un panel lateral deslizante o modal integrado de previsualización en tiempo real:

* Al seleccionar un usuario o modificar sus grupos en el formulario, un visor dinámico computa instantáneamente las ACLs efectivas contra la configuración actual de `smb.conf`:
  * **Verde (`RW`):** Carpetas donde el usuario tiene lectura y escritura (por pertenecer a `write list` o al esquema de grupo).
  * **Azul (`RO`):** Carpetas donde tiene permisos de solo lectura (por pertenecer a `valid users` pero no a `write list`).
  * **Gris / Tachado (`DENY`):** Carpetas privadas donde no tiene acceso alguno.
  * **Oculto con candado (`$`):** Carpetas administrativas ocultas para el explorador de Windows.
* Si el servidor opera en rol `BACKUP`, el previsualizador advierte automáticamente que solo los integrantes de `grp_samba` y `grp_backups` tienen acceso a los recursos con sufijo `$`.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  VISTA PREVIA DE ACCESOS SAMBA • Usuario: maria.lopez (grp_ventas, grp_samba)│
├──────────────────────────────────────────────────────────────────────────────┤
│  ✔ [VENTAS]                     Lectura y Escritura (RW)  • Esquema 1        │
│  ✔ [PUBLICO]                    Lectura y Escritura (RW)  • Esquema 4 (Guest)│
│  👁 [HISTORICO_VENTAS]          Solo Lectura (RO)         • Esquema 3        │
│  ✖ [CONTABILIDAD$]              Acceso Denegado           • Sin membresía    │
│  ✖ [BACKUPS_SERVIDORES$]        Acceso Denegado           • Restringido      │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

### 3.4 Matriz Interactiva de Permisos con Iluminación Cruzada

La matriz de permisos actual se enriquece con:

1. **Iluminación Cruzada de Fila y Columna (*Crosshair Highlighting*):**
   * Al posar el cursor sobre cualquier celda de intersección (Usuario/Grupo vs Recurso), toda la fila y toda la columna correspondiente se iluminan suavemente (`rgba(59, 130, 246, 0.08)`), evitando que el administrador desvíe la vista y se equivoque de fila al aplicar permisos sobre organizaciones con más de 20 recursos.
2. **Inspección de Herencia POSIX:**
   * Al hacer clic secundario (o botón contextual de información) sobre una celda con permiso concedido, una pequeña ventana flotante (*popover*) desglosa la procedencia técnica del permiso:
     * *Directiva Samba:* `write list = @grp_ventas`
     * *ACL POSIX en disco:* `default:group:grp_ventas:rwx`
     * *Ruta física:* `/srv/nas/VENTAS`
3. **Botón de Aplicación en Lote con Confirmación:**
   * Para evitar llamadas sucesivas por cada celda modificada, los cambios se acumulan en un estado local en borrador (*draft state*), resaltando las celdas alteradas en color ámbar parpadeante.
   * Un botón flotante inferior **"Guardar cambios de matriz (X cambios pendientes)"** envía una única transacción consolidada a la API, auditando el cambio en SQLite y aplicando `setfacl` de manera consistente.

---

## 4. Guía de Tokens y Clases CSS para Slate UI

Para asegurar compatibilidad plena con el diseño visual existente en `web/public/css/app.css` y `web/templates/layout.php`, los estilos de esta propuesta respetan la paleta de tokens oficial:

```css
/* Tokens sugeridos para el rediseño de usuarios */
.user-card-admin {
  border-left: 3px solid var(--accent-warning);
  background: linear-gradient(90deg, rgba(245, 158, 11, 0.04) 0%, var(--bg-card) 100%);
}

.user-card-nas {
  border-left: 3px solid var(--accent-primary);
  background: var(--bg-card);
}

.badge-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border-radius: var(--radius-sm);
  font-size: 11.5px;
  font-weight: 500;
  background: var(--bg-surface);
  border: 1px solid var(--border-color);
  color: var(--text-main);
}

.badge-chip .chip-remove {
  cursor: pointer;
  opacity: 0.6;
  transition: opacity 0.15s ease;
}

.badge-chip .chip-remove:hover {
  opacity: 1;
  color: var(--accent-danger);
}

/* Efecto de iluminación cruzada en matriz */
.matrix-cell-active-row,
.matrix-cell-active-col {
  background: rgba(59, 130, 246, 0.06) !important;
}

.matrix-cell-hover {
  outline: 2px solid var(--accent-primary);
  z-index: 10;
}
```

---

## 5. Plan de Implementación por Fases (Hoja de Ruta Futura)

Cuando este desarrollo sea programado para codificación en una versión posterior, se recomienda seguir el siguiente plan por etapas:

| Fase | Alcance | Archivos a Modificar | Impacto |
| :--- | :--- | :--- | :--- |
| **Fase 1** | Segmentación por Pestañas (Admin vs NAS) | `web/templates/layout.php`, `web/public/js/app.js` | Mejora puramente visual en la presentación de tablas sin alterar la API. |
| **Fase 2** | Previsualizador de Accesos Samba | `web/src/Services/SambaService.php`, `web/public/js/app.js` | Endpoint de cálculo de permisos simulados por usuario y modal informativo. |
| **Fase 3** | Tags Chips y Drag-and-Drop | `web/public/js/app.js`, `web/public/css/app.css` | Reemplazo del selector de grupos en los modales de creación y edición. |
| **Fase 4** | Iluminación Cruzada y Guardado en Lote en Matriz | `web/public/js/app.js` | Interactividad avanzada de tabla matricial y consolidación de peticiones. |

---

## 6. Conclusión y Valor Aportado

La adopción de esta propuesta transformará la administración de identidades en **NAS Debian 13**, pasando de un formulario de gestión técnica tradicional a un entorno visual fluido, intuitivo y con salvaguardas inmediatas. El administrador obtendrá **certeza visual total** antes de confirmar cambios en grupos y carpetas compartidas, reduciendo a cero los errores operativos de concesión de permisos indebidos.
