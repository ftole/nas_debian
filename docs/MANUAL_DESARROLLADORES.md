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
Abres tu herramienta de IA en la terminal:
```bash
opencode
```
*(O puedes usar `agy` o `kimi`).*

Le pides a la IA lo que necesitas:
> *"Modifica la función de almacenamiento para listar los discos disponibles. Trabaja solo en los archivos de almacenamiento."*

---

## PASO 2: TESTEAR (Probar que todo funciona)

**Regla de oro:** Ningún código se sube ni se despliega si no pasa las pruebas.

Sal de la herramienta de IA (con `/exit` o `Ctrl+C`) y ejecuta las pruebas del compilador:

### 2.1 Comprobar que no hay errores de sintaxis
```bash
cargo check --workspace --all-targets
```
*Si la salida dice `Finished dev profile`, el código está bien escrito y compila.*

### 2.2 Ejecutar las pruebas automáticas
```bash
cargo test
```
*Si el resultado final dice `test result: ok`, todas las pruebas pasaron exitosamente.*

Si alguna prueba falla, vuelve a abrir tu agente de IA y dile:
> *"El comando cargo test falló con este error: [pegar error]. Corrígelo."*

---

## PASO 3: DESPLEGAR (Guardar, subir a GitHub y activar en el servidor)

Una vez que el código pasó las pruebas de la Etapa 2, es momento de publicarlo y ponerlo en vivo en el servidor.

### 3.1 Guardar tus cambios en tu rama
```bash
# 1. Ver qué archivo modificaste
git status

# 2. Agregar el archivo
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
Este es el paso final que **instala el código nuevo en el servidor real**:
```bash
sudo ./packaging/install.sh
```

### ¿Qué hace este comando de despliegue?
1. Compila todo el código nuevo en modo optimizado de producción.
2. Actualiza los ejecutables en el sistema (`/usr/local/bin/nas-web`, etc.).
3. Reinicia el servicio web en 2 segundos.
4. **Listo:** Al abrir `https://10.10.1.222` en el navegador, tu cambio ya está funcionando en vivo.

---

## Chuleta de Comandos Rápidos (Para el día a día)

```bash
# 1. PROGRAMAR
cd /opt/dev/nas_debian
git checkout main && git pull origin main
git checkout -b dev1-tarea
opencode

# 2. TESTEAR
cargo check --workspace --all-targets
cargo test

# 3. DESPLEGAR
git add <archivo>
git commit -m "feat: descripcion"
git push origin dev1-tarea
git checkout main && git pull && git merge dev1-tarea && git push origin main
sudo ./packaging/install.sh
```
