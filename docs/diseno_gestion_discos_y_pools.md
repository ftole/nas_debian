# Diseño de Lógica Operativa: Gestión Segura de Discos, Pools Btrfs y Unidades (`Rust Target Spec`)

> [!NOTE]
> Documento de ingeniería vivo construido a partir de casos reales de operación y recuperación sobre Debian 13 (`x86_64`).
> **Propósito:** definir paso a paso la lógica exacta de inspección, validación, montaje resiliente y saneamiento de `/etc/fstab` y permisos para replicarla limpiamente en el motor Rust (`crates/nas-engine`).
> *(Todos los identificadores, rutas concretas, direcciones y UUIDs están abstraídos con marcadores genéricos).*

---

## 1. Principios de Diseño (Reglas que debe cumplir el código Rust)

1. **Separación estricta SO vs Almacenamiento:** nunca permitir que respaldos masivos escriban sobre el sistema de archivos raíz (`/`) sin advertencia explícita cuando `/srv/nas` no esté respaldado por un disco o pool dedicado.
2. **Reconocimiento real de pools multi-dispositivo (Btrfs):** un mismo filesystem Btrfs puede estar repartido en `N` particiones/discos físicos (`<DISCO_A>1`, `<DISCO_B>1`). Ningún miembro de un pool activo puede mostrarse como "disco disponible para formatear".
3. **Resiliencia ante apagones en `/etc/fstab`:** toda unidad secundaria (`/srv/nas` o `/srv/nas/unidades/<ID>`) debe montarse con `nofail,x-systemd.device-timeout=15s`, sin opciones de desgaste/amplificación (`autodefrag`, `discard=async`).
4. **Cero operaciones recursivas sobre snapshots:** asignar permisos (`2770`) y propietario (`root:grp_samba` + ACL del usuario de servicio) **únicamente** en la carpeta raíz de la unidad y sus 3 contenedores padre (`BACKUPS_HISTORICOS`, `BACKUPS_PERMANENTES`, `LOGS_BACKUP`), jamás con `-R` sobre históricos.
5. **Solo lectura primero ante recuperación o duda:** ante montajes manuales o recuperación tras cortes eléctricos, diagnosticar sin escribir (`lsblk`, `btrfs filesystem show`, `mount -o ro`, `smartctl -H`) antes de habilitar `rw` y persistir en `/etc/fstab`.

---

## 2. Fase 1 — Inspección Integral (Solo Lectura)

Antes de que Rust (o el operador) modifique montajes o `/etc/fstab`, debe recopilar el estado completo combinando 5 fuentes:

### 2.1 Qué datos recoger y por qué

| Fuente / Comando | Qué aporta a la lógica | Edge case que resuelve |
| :--- | :--- | :--- |
| `lsblk -J -o NAME,SIZE,TYPE,FSTYPE,UUID,MOUNTPOINT` | Topología física, particiones, tipo de filesystem y punto de montaje activo | Detecta si `/srv/nas` está realmente montado o cae sobre `/` (disco del SO). |
| `btrfs device scan` + `btrfs filesystem show` | Lista cada filesystem Btrfs por su **UUID lógico de filesystem**, `Total devices N` y todos los `devid` miembros | Evita confundir el UUID individual de un miembro o tratar el segundo disco de un pool como disco libre. |
| `mount` / `/proc/mounts` | Opciones reales activas en kernel (`rw` vs `ro`, `autodefrag`, `subvol`) | Detecta si el kernel ya montó con opciones obsoletas heredadas o cayó en solo lectura (`ro`). |
| `/etc/fstab` | Persistencia declarada para el próximo arranque | Detecta **entradas huérfanas** (ej. LVMs viejos inexistentes), falta de `nofail` o parámetros dañinos. |
| `smartctl -H <DISCO>` | Estado de salud física S.M.A.R.T. | Aborta operaciones de escritura/formateo si el controlador o disco reporta fallo. |

### 2.2 Anomalías detectadas en campo que Rust debe detectar automáticamente

1. **Entrada fantasma en `/etc/fstab`:** una línea vieja (ej. `/dev/mapper/<vg>-<lv> /srv/nas ...`) apunta a un volumen lógico eliminado tras reinstalar el SO. Resultado: `/srv/nas` queda viviendo dentro de `/` (disco del SO) mientras los discos grandes se montaron en `/srv/nas/unidades/<ID>`.
2. **Montaje superpuesto (*overmount*) en `/srv/nas/unidades/<ID>`:** si dos discos distintos se formatean/montan sucesivamente bajo el mismo `<ID>` de unidad, Linux monta el segundo **encima** del primero dejando el primer disco oculto e inaccesible (aunque ambos tengan `Total devices 1` independientes). La lógica Rust debe validar `findmnt` / `/proc/mounts` sobre el directorio destino antes de montar o formatear, rechazando si ya hay otro montaje activo.
3. **Opciones obsoletas persistidas en `/etc/fstab`:** montajes previos sin `nofail` ni `x-systemd.device-timeout=15s`, y con `autodefrag` activo. Ante un fallo de cable/controladora en arranque, el sistema cae a modo emergencia sin SSH.
4. **Permisos cerrados en subdirectorios de montaje:** si un punto de montaje `/srv/nas/unidades` o `/srv/nas/unidades/<ID>` no tiene acceso `grp_samba` + ACL para el usuario del servicio web (`nas-web`), las consultas de espacio libre (`df`) o el panel fallan con `Permission denied`. Además, ejecutar `mountpoint -q` sin privilegios sobre rutas `2770` devuelve código `1` por permiso denegado y engaña a bucles de limpieza; en código se debe consultar `/proc/mounts` (legible globalmente) o elevar con `sudo`.
5. **Orden LIFO de desmontaje en rutas anidadas:** si `/srv/nas` se monta mientras existían montajes previos en `/srv/nas/unidades/<ID>`, los montajes hijos quedan ocultos bajo la raíz del nuevo montaje; es obligatorio desmontar en orden inverso (primero `/srv/nas`, luego los hijos restantes) y verificar `mount` / `/proc/mounts` antes de invocar `wipefs` o `mkfs`.
6. **Ausencia de `smartmontools` en hosts base:** cuando `smartctl` no está instalado, las validaciones físicas degradan sin diagnóstico real; el aprovisionamiento debe contemplar `smartmontools` como paquete base.

---

## 3. Fase 2 — Lógica de Saneamiento y Configuración Segura (En Curso)

Secuencia ordenada que el operador valida manualmente y que luego se portará a `crates/nas-engine`:

### Paso 1: Verificar el contenido real dentro de la unidad montada y del filesystem Btrfs
- Comprobar como `root` (`sudo`) qué directorios existen dentro de `/srv/nas/unidades/<ID>` (si ya contiene `BACKUPS_HISTORICOS`, `BACKUPS_PERMANENTES`, `LOGS_BACKUP` o snapshots previos) y el UUID exacto del filesystem Btrfs (`btrfs filesystem show`).
- Verificar qué hay en `/srv/nas` (para confirmar que no quedaron datos escritos por error sobre el disco raíz `/` mientras `/srv/nas` no estaba montado).

### Paso 2: Verificar salud física S.M.A.R.T. y eventos recientes del kernel
- Consultar `smartctl -H` de cada disco físico miembro del pool y revisar `dmesg` por errores de controladora o `I/O error`.

### Paso 3: Limpiar y corregir `/etc/fstab` con respaldo previo
- Hacer copia de respaldo `/etc/fstab.bak.<timestamp>` antes de escribir.
- **Retirar entradas muertas** (dispositivos `/dev/mapper/...` que ya no existen en el sistema).
- **Normalizar la entrada del pool Btrfs:** usar `UUID=<UUID_DEL_FILESYSTEM_BTRFS>` (obtenido de `btrfs filesystem show`), tipo `btrfs`, y opciones seguras vigentes:
  ```text
  UUID=<UUID_FS> <PUNTO_MONTAJE> btrfs rw,noatime,compress=zstd:3,space_cache=v2,nofail,x-systemd.device-timeout=15s 0 0
  ```
  *(Añadir `,ssd` si todos los miembros tienen `rotational=0`; prohibido `autodefrag` y `discard=async`).*
- Validar sin reiniciar con `systemctl daemon-reload` y `findmnt --verify --fstab`.
- Aplicar en vivo con `mount -o remount <PUNTO_MONTAJE>` (si pasó de `autodefrag` a sin `autodefrag`, comprobar con `mount | grep <PUNTO_MONTAJE>` que las opciones activas quedaron limpias).

### Paso 4: Preparar permisos y estructura `BACKUP` sin recursividad masiva
- Crear únicamente si faltan los contenedores raíz: `BACKUPS_HISTORICOS`, `BACKUPS_PERMANENTES`, `LOGS_BACKUP`.
- Aplicar `chown root:grp_samba` y `chmod 2770` + `setfacl` de acceso para `nas-web` **solo sobre los contenedores raíz** (sin `-R`).
- Si la unidad es Btrfs y no reside en `/srv/nas` sino en `/srv/nas/unidades/<ID>`, asegurar tarea mensual de `btrfs scrub` dedicada (`nice -n 19 ionice -c 3`).

### Paso 5: Validar consistencia del rol, cron de integridad y servicios
- Confirmar `/etc/nas/role` = `BACKUP` y existencia del cron mensual `/etc/cron.d/nas-btrfs-scrub` (`nice -n 19 ionice -c 3 btrfs scrub start -B <PUNTO_MONTAJE>`).
- Verificar con `btrfs filesystem show` (`Total devices N`), `btrfs filesystem df <PUNTO_MONTAJE>` (`Data: single`, `Metadata/System: RAID1` en pools multi-disco) y `df -Th <PUNTO_MONTAJE>` que la capacidad agregada coincide con la suma esperada.
- Probar estado de servicios (`nas-web`, `smbd`, `nmbd`, `nginx`) y verificar desde `nas diag` que ningún volumen aparezca en solo lectura (`ro`) y que el panel web muestre correctamente el espacio real del pool.

---

## 4. Requisitos para Replicar esta Lógica en Rust (`nas-engine`)

Al llevar este flujo a `crates/nas-engine/src/storage.rs` y `helpers/storage/units.rs`:

1. **Modelo `BtrfsPoolInfo`:** parsear `btrfs filesystem show` para agrupar múltiples dispositivos `/dev/...` bajo un mismo `fs_uuid` y marcar todos los `devid` miembros como `in_use = true` / `pool_member_of = Some(fs_uuid)`.
2. **Helper `reconcile_fstab`:**
   - Leer `/etc/fstab`, detectar y reportar líneas cuyo origen ya no existe en `/dev/`.
   - Aplicar siempre `fstab_options_for_target` (`nofail,x-systemd.device-timeout=15s`) también en `deploy/storage.rs` y `helpers/storage/units.rs` (resolviendo `DT-A08`).
3. **Guardia de destino de respaldo cuando `/srv/nas` está sobre `/`:**
   - Exponer un indicador `data_root_on_os_disk: bool` en `/api/storage/units` y advertir en la UI si existen unidades dedicadas en `/srv/nas/unidades/<ID>` para que el operador no cree tareas sobre `principal`.
4. **Auto-alta de `btrfs scrub` por unidad:** al montar una unidad adicional Btrfs en `/srv/nas/unidades/<ID>`, registrar su cron mensual con `nice -n 19 ionice -c 3` (resolviendo `DT-M12`).
5. **Bloqueo contra *overmount*:** antes de montar en `/srv/nas/unidades/<ID>`, verificar si el destino ya es un `mountpoint`; si lo es, rechazar la operación en `add_unit`.
6. **Modelado de topología multi-disco (Pool único vs Unidades independientes):**
   - **Opción Pool Unificado Btrfs:** unir `N` discos en un solo volumen (`mkfs.btrfs -d single -m raid1` para sumar capacidad protegiendo metadatos, o `-d raid1 -m raid1` si se busca espejo) montado en `/srv/nas`.
   - **Opción Unidades Independientes:** disco A montado en `/srv/nas` principal + disco B montado en `/srv/nas/unidades/<ID>`, aislando fallos físicos de un disco respecto al otro.
