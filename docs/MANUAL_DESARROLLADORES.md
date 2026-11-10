# Manual de Trabajo: Programar, Testear y Desplegar

> [!NOTE]
> Este manual está escrito para cualquier desarrollador (`dev1`, `dev2`, etc.), sin importar su nivel de experiencia en Git o Linux. Sigue el flujo natural de trabajo en 3 etapas: **1. Programar -> 2. Testear -> 3. Desplegar**.

---

## Dónde está el proyecto en el servidor

Todos los desarrolladores trabajan sobre el repositorio ubicado en la ruta real del servidor:

```bash
/opt/dev/nas_debian
```

---

# El Flujo de Trabajo en 3 Pasos

```text
┌────────────────────────┐      ┌────────────────────────┐      ┌────────────────────────┐
│      1. PROGRAMAR      │ ──>  │      2. TESTEAR        │ ──>  │      3. DESPLEGAR      │
│  Crear rama y codificar│      │  Compilar y probar     │      │  Guardar, unir y lanzar│
└────────────────────────┘      └────────────────────────┘      └────────────────────────┘
```

---

## PASO 1: PROGRAMAR (Escribir el código)

Cualquier desarrollador (`dev1`) entra por SSH a su cuenta y sigue estos pasos:

### 1.1 Entrar a la carpeta del proyecto
```bash
cd /opt/dev/nas_debian
```

### 1.2 Descargar la versión más reciente
Antes de hacer nada, asegúrate de tener lo último que hayan hecho los demás:
```bash
git checkout main
git pull origin main
```

### 1.3 Crear tu propia rama de trabajo
Para no modificar la rama principal directamente, creas tu propia rama con tu nombre y tarea:
```bash
git checkout -b dev1-mi-tarea
```
*(A partir de aquí, estás trabajando en tu rama `dev1-mi-tarea`. Nada de lo que hagas afecta a los demás).*

### 1.4 Escribir código con tu Agente de IA
El proyecto está **completamente modularizado** para que varios desarrolladores puedan trabajar a la vez sin colisionar en Git:
* Si trabajas en el **panel web**, tu lógica vive en su archivo de dominio dentro de `crates/nas-web/src/routes/` (ej. `shares.rs`, `backups.rs`, `storage.rs`, `users.rs`).
* Si trabajas en la **CLI**, tu comando vive en `crates/nas-cli/src/commands/` (ej. `status.rs`, `backup.rs`, `deploy.rs`).

Abres tu herramienta de IA en la terminal:
```bash
opencode
```
*(O puedes usar `agy` o `kimi`).*

Le pides a la IA lo que necesitas delimitando el archivo:
> *"Modifica la función de almacenamiento para listar los discos disponibles. Trabaja exclusivamente en crates/nas-web/src/routes/storage.rs y crates/nas-cli/src/commands/storage.rs."*

---

## PASO 2: TESTEAR (Probar que todo funciona)

**Regla de oro:** Ningún código se sube ni se despliega si no pasa las pruebas y linters del CI.

Sal de la herramienta de IA (con `/exit` o `Ctrl+C`) y ejecuta las verificaciones del compilador:

### 2.1 Comprobar que no hay errores de sintaxis
```bash
cargo check --workspace --all-targets
```
*Si la salida dice `Finished dev profile`, el código está bien escrito y compila.*

### 2.2 Verificar formato de código (Obligatorio en CI)
```bash
cargo fmt --all --check
```
*Si marca diferencias, ejecuta `cargo fmt --all` para formatear automáticamente.*

### 2.3 Analizar con Clippy (Cero advertencias requeridas)
```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```
*Garantiza que el código cumple con las mejores prácticas y no tiene malas construcciones.*

### 2.4 Ejecutar la suite completa de pruebas
```bash
cargo test --workspace --locked
```
*Si el resultado final dice `test result: ok`, todas las pruebas pasaron exitosamente.*

Si alguna prueba o verificación falla, vuelve a abrir tu agente de IA y dile:
> *"El comando [cargo test / cargo clippy / cargo fmt] falló con este error: [pegar error]. Corrígelo en mi módulo."*

---

## PASO 3: DESPLEGAR (Guardar, subir a GitHub y activar en el servidor)

Una vez que el código pasó las pruebas de la Etapa 2, es momento de publicarlo y ponerlo en vivo en el servidor.

> [!IMPORTANT]
> **Regla de Commits Granulares:** Realiza un commit independiente por cada archivo modificado con mensaje convencional en español (`feat: ...`, `fix: ...`, `refactor: ...`).

### 3.1 Guardar tus cambios en tu rama
```bash
# 1. Ver qué archivo modificaste
git status

# 2. Agregar el archivo (uno por uno)
git add ruta/al/archivo.rs

# 3. Guardar el cambio con una descripción de lo que hiciste
git commit -m "feat: agregar listado de discos"

# 4. Subir tu rama a GitHub
git push origin dev1-mi-tarea
```

### 3.2 Pasar tus cambios a la rama principal (`main`)
Ahora unes tu trabajo a `main` para que forme parte oficial del sistema:
```bash
# 1. Cambiar a main y descargar cambios
git checkout main
git pull origin main

# 2. Unir tu rama
git merge dev1-mi-tarea

# 3. Subir el main actualizado a GitHub
git push origin main

# 4. (Opcional) Borrar tu rama porque ya se unió a main
git branch -d dev1-mi-tarea
```

### 3.3 Desplegar en vivo en el servidor NAS
Este es el paso final que **compila e instala el código nuevo en el servidor real**:

```bash
# Opción recomendada para desarrollo (rápido, compila y reinicia nas-web):
sudo ./scripts/deploy_local.sh

# O si modificaste todos los crates del workspace:
sudo ./scripts/deploy_local.sh --all
```

> [!TIP]
> **Caché del navegador:** Si modificaste la interfaz web (`app.js`, HTML o CSS), recarga la página en tu navegador con **`Ctrl + Shift + R`** (o **`Ctrl + F5`**) para invalidar la caché HTTP (`max-age=300`) y ver los cambios al instante.

*(Para aprovisionamiento general de infraestructura o actualizar plantillas de Nginx/sudoers, se utiliza `sudo ./packaging/install.sh`).*

### ¿Qué hace `deploy_local.sh`?
1. Compila el código modificado en modo optimizado de producción (`--release`).
2. Actualiza los ejecutables en el sistema (`/usr/local/bin/nas-web`, etc.).
3. Reinicia el servicio demonio `nas-web.service` de forma automática.
4. **Listo:** Al abrir `https://10.10.1.222` (o la IP del servidor) y refrescar la caché, tu cambio ya está funcionando en vivo.

---

## Chuleta de Comandos Rápidos (Para el día a día)

```bash
# 1. PROGRAMAR
cd /opt/dev/nas_debian
git checkout main && git pull origin main
git checkout -b dev1-tarea
opencode

# 2. TESTEAR (Obligatorio antes de subir)
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

# 3. DESPLEGAR
git add <archivo>
git commit -m "feat: descripcion"
git push origin dev1-tarea
git checkout main && git pull && git merge dev1-tarea && git push origin main
sudo ./scripts/deploy_local.sh
```
