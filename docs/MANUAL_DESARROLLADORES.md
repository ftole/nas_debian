# Manual del Desarrollador: Guía de Trabajo Colaborativo Multi-Agente

> [!IMPORTANT]
> Este manual está diseñado para que **Frank**, **Beto** y **Chris** puedan programar en el servidor `nasdeb` de forma simultánea, utilizando agentes de IA (**OpenCode**, **Antigravity**, **Kimi**), sin pisarse el código, sin bloqueos de permisos y con total orden en Git.

---

## 1. Mapa de Espacios de Trabajo y Asignaciones

Cada programador tiene su propio usuario de Linux, su propia carpeta física en disco (**Worktree**) y su propia rama en Git. **Nadie trabaja en la carpeta de los demás.**

```text
/opt/dev/workspaces/
├── storage/    ---> Asignado a FRANK  (Rama: feature/storage)
├── backups/    ---> Asignado a BETO   (Rama: feature/backups)
└── frontend/   ---> Asignado a CHRIS  (Rama: feature/frontend)
```

---

## 2. Fichas de Trabajo por Desarrollador

###  Ficha de FRANK (Arquitectura y Gestión de Discos)
* **Usuario:** `frank`
* **Directorio de trabajo:** `/opt/dev/workspaces/storage`
* **Rama Git asignada:** `feature/storage`
* **Herramientas de IA disponibles:** `agy` (Antigravity CLI), `opencode`
* **Archivos y módulos permitidos:**
  - `crates/nas-engine/src/storage/` (Lógica LVM, Btrfs, particiones)
  - `crates/nas-web/src/routes/storage.rs` (Endpoints `/api/storage/*`)
  - `crates/nas-web/templates/views/storage.html` (Vista de discos)
  - `crates/nas-web/assets/js/modules/storage.js` (Lógica JS de discos)
* **Cómo empezar a trabajar:**
  ```bash
  cd /opt/dev/workspaces/storage
  agy    # O escribe: opencode
  ```

---

###  Ficha de BETO (Motor de Respaldos y Telemetría)
* **Usuario:** `beto`
* **Directorio de trabajo:** `/opt/dev/workspaces/backups`
* **Rama Git asignada:** `feature/backups`
* **Herramientas de IA disponibles:** `opencode`, `kimi`
* **Archivos y módulos permitidos:**
  - `crates/nas-engine/src/backup/` (Rsync, snapshots, compresión Zstandard)
  - `crates/nas-web/src/routes/backups.rs` (Endpoints `/api/backups/*`)
  - `crates/nas-web/templates/views/backups.html` (Vista de respaldos)
  - `crates/nas-web/assets/js/modules/backups.js` (Lógica JS de respaldos)
* **Cómo empezar a trabajar:**
  ```bash
  cd /opt/dev/workspaces/backups
  opencode    # O escribe: kimi
  ```

---

###  Ficha de CHRIS (Frontend y Modularización de Vistas)
* **Usuario:** `chris`
* **Directorio de trabajo:** `/opt/dev/workspaces/frontend`
* **Rama Git asignada:** `feature/frontend`
* **Herramientas de IA disponibles:** `opencode`, `kimi`
* **Archivos y módulos permitidos:**
  - `crates/nas-web/templates/views/` (Fragmentación de vistas HTML)
  - `crates/nas-web/templates/modals/` (Modales independientes)
  - `crates/nas-web/assets/js/modules/` (Módulos JS independientes)
  - `crates/nas-web/assets/css/` (Estilos visuales Slate UI)
* **Cómo empezar a trabajar:**
  ```bash
  cd /opt/dev/workspaces/frontend
  kimi    # O escribe: opencode
  ```

---

## 3. El Flujo de Trabajo Diario (Los 4 Pasos Obligatorios)

Cada desarrollador sigue esta rutina diaria para garantizar calidad y cero errores:

### Paso 1: Entrar a su espacio y actualizar cambios de main
Antes de empezar a codificar en el día:
```bash
cd /opt/dev/workspaces/<tu_carpeta>
git fetch origin
git merge origin/main
```

### Paso 2: Trabajar con su Agente de IA
Lanzas tu agente preferido y le pides la tarea específica:
```bash
opencode   # o agy, o kimi
```
> **Consejo para pedirle tareas al agente:**  
> *"Trabaja únicamente dentro de la carpeta asignada a nuestro módulo. No modifiques archivos de otros módulos. Al terminar verifica que `cargo check` compile sin errores."*

### Paso 3: Validar que el código no tenga errores
Antes de guardar o hacer commit, debes correr las pruebas:
```bash
# 1. Comprobar que no hay errores de sintaxis
cargo check --workspace --all-targets

# 2. Comprobar que las pruebas pasan
cargo test
```
Si algo falla, dile a tu agente que lo corrija antes de hacer commit.

### Paso 4: Guardar y Subir a GitHub
Cuando la tarea esté lista y verificada:
```bash
# 1. Ver qué archivos modificaste
git status

# 2. Agregar tus cambios
git add <archivo_modificado>

# 3. Hacer el commit con mensaje descriptivo en español
git commit -m "feat: implementar wizard de adicion de discos al pool"

# 4. Subir a tu rama en GitHub
git push origin <tu_rama>
```

---

## 4. Reglas de Oro del Proyecto (`AGENTS.md`)

Para mantener la disciplina técnica de nivel empresarial:

1. **Regla de Commits Granulares:** Debe existir **un commit independiente por cada archivo modificado**. Si alteras 3 archivos, realiza 3 commits por separado.
2. **Mensajes en Español Estándar:**
   - `feat: <descripción en español>` (nueva funcionalidad)
   - `fix: <descripción en español>` (corrección de errores)
   - `docs: <descripción en español>` (documentación)
   - `refactor: <descripción en español>` (mejoras internas)
   - `test: <descripción en español>` (nuevas pruebas)
3. **Cero Dependencias Externas en Runtime:** El panel debe funcionar **100% offline**, sin llamar a CDNs de internet, fuentes de Google Fonts ni paquetes npm pesados.
4. **Protección del Disco del SO:** Ningún código de almacenamiento debe formatear ni alterar discos que contengan la raíz (`/`) o `/boot`.

---

## 5. Cómo se Integran los Cambios a `main` (Cutover)

Cuando Frank, Beto o Chris terminan su función y sus pruebas pasan:
1. En GitHub, abren un **Pull Request** desde su rama (`feature/storage`, etc.) hacia `main`.
2. Frank (o el equipo) revisa el PR y aprueba la fusión (*Merge*).
3. En el servidor, para desplegar la nueva versión consolidada en vivo:
   ```bash
   cd /opt/dev/nas_debian
   git pull origin main
   sudo ./packaging/install.sh
   ```
4. El servidor se actualiza automáticamente y el servicio queda activo en `https://10.10.1.222`.

---

## 6. Preguntas Frecuentes y Solución de Problemas

* **¿Qué hago si me sale `Permission denied` al editar un archivo?**  
  Asegúrate de pertenecer al grupo `devs` ejecutando `groups`. Si no aparece `devs`, pídele a Frank que ejecute `sudo usermod -aG devs tu_usuario`.
* **¿Qué pasa si dos personas tocan un archivo compartido por accidente?**  
  Al estar trabajando en ramas distintas (`feature/storage` vs `feature/backups`), Git no romperá el código de nadie. Cuando se haga el Pull Request, GitHub indicará qué línea colisionó para resolverla en 1 minuto.
* **¿Cómo salgo de OpenCode, Antigravity o Kimi?**  
  Escribe `/exit` o presiona `Ctrl+C` / `Ctrl+D`.
