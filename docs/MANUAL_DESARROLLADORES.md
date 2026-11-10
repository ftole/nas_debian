# Manual del Desarrollador: Guía Completa de Ramas, Git y Despliegue

> [!IMPORTANT]
> Este manual explica en lenguaje claro y humano cómo trabajar en equipo en el servidor `nasdeb`. Explica cómo crear ramas, cómo probar con tus agentes de IA, cómo subir tus cambios a GitHub, cómo fusionarlos en `main` y cómo desplegarlos en vivo en el servidor.

---

## 1. La Regla Básica: ¿Por qué cada quien tiene su carpeta de trabajo?

En Git, **una carpeta física solo puede mostrar UNA rama a la vez**.
Si Frank cambia de rama a `feature/almacenamiento` y Beto cambia a `feature/respaldos`, **no pueden estar dentro de la misma carpeta al mismo tiempo**, porque uno le borraría de la pantalla los archivos al otro.

Por eso, cada desarrollador trabaja en su propio espacio en disco:
* **Frank:** Entra a su carpeta `/opt/dev/workspaces/storage` (o `~/nas_debian`)
* **Beto:** Entra a su carpeta `/opt/dev/workspaces/backups` (o `~/nas_debian`)
* **Chris:** Entra a su carpeta `/opt/dev/workspaces/frontend` (o `~/nas_debian`)

Así, cada quien puede tener su propia rama abierta al mismo tiempo sin estorbarse.

---

## 2. El Flujo de Trabajo Completo (De principio a fin)

Todo desarrollador sigue este ciclo de 5 etapas para cada función o corrección que programe:

```text
  1. CREAR RAMA      2. PROGRAMAR CON IA      3. COMMIT & PUSH      4. PASAR A MAIN      5. DESPLEGAR EN VIVO
 [git checkout -b] -> [opencode / agy / kimi] -> [git push origin] -> [Pull Request / merge] -> [sudo ./packaging/install.sh]
```

---

### ETAPA 1: Empezar una tarea (Crear tu rama)

Antes de escribir una sola línea de código, te aseguras de tener la última versión de `main` y creas una rama con un nombre representativo:

```bash
# 1. Entrar a tu carpeta de trabajo
cd /opt/dev/workspaces/<tu_carpeta>

# 2. Ir a main y descargar lo más reciente de GitHub
git checkout main
git pull origin main

# 3. Crear tu rama nueva y cambiarte a ella
git checkout -b feature/mi-nueva-funcion
```

> **¿Qué hace `git checkout -b`?**  
> Crea una copia aislada de `main` llamada `feature/mi-nueva-funcion`. Todo lo que hagas ahí dentro es privado para ti hasta que decidas subirlo.

---

### ETAPA 2: Programar con tu Agente de IA y Probar

Lanzas tu agente preferido en la terminal:

```bash
# Lanza tu agente de IA
opencode    # O escribe: agy (Frank), o: kimi (Beto/Chris)
```

Le pides la tarea a tu agente:
> *"Modifica el módulo de discos para que liste la salud SMART y agregue el botón de expansión. Al terminar verifica que compile."*

#### La Prueba Obligatoria (Antes de guardar nada):
Una vez que el agente termina, **tú o el agente deben verificar que el código compila y no rompió nada**:

```bash
# 1. Comprobar que no hay errores de sintaxis
cargo check --workspace --all-targets

# 2. Correr las pruebas automatizadas del proyecto
cargo test
```

Si el resultado termina en `test result: ok`, el código está sano.

---

### ETAPA 3: Guardar tus cambios y subirlos a GitHub

Ahora guardas tus cambios en Git y los envías a GitHub:

```bash
# 1. Revisar qué archivos modificaste
git status

# 2. Agregar el archivo modificado (un commit por archivo, regla del proyecto)
git add ruta/al/archivo_modificado.rs

# 3. Guardar el cambio con mensaje descriptivo en español
git commit -m "feat: agregar deteccion visual de discos"

# 4. Subir tu rama a GitHub
git push origin feature/mi-nueva-funcion
```

A partir de este momento, tu trabajo está respaldado en la nube en GitHub.

---

### ETAPA 4: Pasar tus cambios a `main` (Integración)

Una vez que tu función está 100% probada, es momento de unirla a la rama principal (`main`). Hay dos formas:

#### Opción A (La forma recomendada: Vía Pull Request en GitHub):
1. Entras a GitHub en tu navegador: `https://github.com/ftole/nas_debian`.
2. Verás un banner amarillo arriba que dice: *`feature/mi-nueva-funcion had recent pushes`*.
3. Haces clic en el botón verde **`Compare & pull request`**.
4. Escribes un resumen de lo que hiciste y das clic en **`Create pull request`**.
5. Cuando el equipo o Frank lo aprueba, haces clic en **`Merge pull request`** y luego **`Confirm merge`**.  
   ¡Listo! Tu código ya forma parte oficial de `main`.

#### Opción B (Directo por consola en Debian):
Si prefieres unirlo directamente desde la terminal:
```bash
# 1. Cambiarte a main
git checkout main

# 2. Descargar lo último por si alguien más subió algo
git pull origin main

# 3. Unir tu rama a main
git merge feature/mi-nueva-funcion

# 4. Subir el main actualizado a GitHub
git push origin main

# 5. Borrar tu rama local porque ya quedó integrada
git branch -d feature/mi-nueva-funcion
```

---

### ETAPA 5: Desplegar en vivo en el servidor NAS (Producción)

Ya que el código nuevo está en `main`, el último paso es **hacer que el servidor NAS empiece a ejecutar la nueva versión**:

```bash
# 1. Ir a la carpeta principal del proyecto
cd /opt/dev/nas_debian

# 2. Asegurarte de tener el main con todos los cambios integrados
git checkout main
git pull origin main

# 3. Ejecutar el instalador y desplegador oficial del servidor
sudo ./packaging/install.sh
```

### ¿Qué hace `sudo ./packaging/install.sh`?
1. Compila todo el código nuevo en modo de máxima velocidad (`cargo build --release`).
2. Actualiza los binarios en `/usr/local/bin` (`nas-web`, `nas`, etc.).
3. Actualiza las reglas de Nginx y reinicia el servicio web en 2 segundos.
4. Abre `https://10.10.1.222` en tu navegador: **tu nueva función ya está viva y funcionando para todos los usuarios de la red.**

---

## 3. Resumen Rápido de Comandos (Para tener siempre a mano)

| Qué quiero hacer | Comando en la terminal |
| :--- | :--- |
| **Ver en qué rama estoy** | `git branch` |
| **Actualizar main antes de empezar** | `git checkout main && git pull origin main` |
| **Crear mi rama de trabajo** | `git checkout -b feature/nombre-de-mi-tarea` |
| **Comprobar si mi código compila** | `cargo check --workspace --all-targets` |
| **Correr las pruebas** | `cargo test` |
| **Ver qué archivos cambié** | `git status` |
| **Guardar un cambio** | `git add <archivo> && git commit -m "feat: <mensaje>"` |
| **Subir mi rama a GitHub** | `git push origin feature/nombre-de-mi-tarea` |
| **Unir mi rama a main (consola)** | `git checkout main && git pull && git merge feature/<mi-rama> && git push` |
| **Desplegar en el NAS en vivo** | `sudo ./packaging/install.sh` |
