# Modos de Fallo y Resiliencia

Este documento describe cómo se comporta el sistema ante fallos y cómo verificarlo. Su objetivo es demostrar que cada fallo esperado se maneja de forma controlada: sin corromper datos, sin dejar recursos bloqueados y con un mensaje claro en el registro.

## Cómo se prueban los fallos

- **Pruebas automáticas (integración continua):** se ejecutan en GitHub Actions con `bats tests/` y `pytest tests/`. Usan directorios temporales y binarios simulados, por lo que **no tocan discos, red ni servicios reales**.
- **Verificación manual (laboratorio):** los casos que requieren el sistema real (caída de servicios, corte de energía, discos físicos) se comprueban en una máquina virtual desechable siguiendo los pasos indicados.

## Matriz de modos de fallo

### Copias de seguridad

| Fallo | Comportamiento esperado | Verificación |
| :--- | :--- | :--- |
| Espacio libre insuficiente | El runner aborta antes de copiar y registra "espacio libre insuficiente". | `failure_runners.bats` |
| `rsync` falla a mitad | Sale con error, registra el fallo y **descarta el snapshot parcial**. | `failure_runners.bats` |
| Montaje CIFS falla | Sale con error; no queda ningún punto de montaje colgado. | `failure_runners.bats` |
| Credenciales SSH rechazadas | Sale con error y lo registra. | `failure_runners.bats` |
| Ejecución simultánea de la misma tarea | La segunda ejecución se omite ("BACKUP OMITIDO") y sale sin error. | `failure_runners.bats` |
| Interrupción abrupta (corte de energía) | El bloqueo `flock` se libera al terminar el proceso; el snapshot parcial no se usa como referencia. | Manual (VM) |
| Servidor remoto inalcanzable | El montaje o el `rsync` fallan; se registra y no se altera el último snapshot válido. | Manual (VM) |
| Retención mal configurada (0) | Se normaliza a un valor mínimo de 1. | `validar_cron` / revisión de código |

### Despliegue

| Fallo | Comportamiento esperado | Verificación |
| :--- | :--- | :--- |
| Disco dedicado en uso (montado, PV o RAID) | El despliegue aborta salvo que se use `--force`. | `failure_helpers.bats` (`disco_en_uso`) |
| Disco del sistema operativo (LVM/RAID/LUKS/Btrfs) | Se identifica y se excluye del menú. | `helpers.bats` (`resolver_discos_raiz`) |
| Partición no detectada tras el particionado | Aborta para no formatear el disco completo. | Manual (VM) |
| Re-despliegue sobre un servidor configurado | Se respalda `smb.conf` antes de regenerarlo y se avisa si `/srv/nas` ya está montado. | Manual (VM) |
| Servicio que no arranca (`smbd`, `wsdd2`, `cockpit`) | Se reporta `[OK]`/`[!]` por servicio al finalizar. | Manual (VM) |
| `cron` inactivo | Se habilita y arranca; si no, se avisa. | Manual (VM) |
| Extensión `.deb` alterada | La verificación SHA256 falla y la extensión se omite. | Manual (VM) |
| Archivo `sudoers` inválido | `visudo -c` lo descarta. | Manual (VM) |

### Usuarios y recursos compartidos

| Fallo | Comportamiento esperado | Verificación |
| :--- | :--- | :--- |
| Nombre de usuario inválido | Se rechaza con un mensaje de formato. | Revisión de código |
| Nombre o ruta de recurso inválidos | Se rechaza antes de escribir la configuración. | Revisión de código |
| Comentario con saltos de línea | Se limpia antes de escribirlo en `smb.conf`. | Revisión de código |
| Recurso con varios grupos (esquema 1) | Se aplican ACL para todos los grupos autorizados. | Manual (VM) |

### Instalación y actualización

| Fallo | Comportamiento esperado | Verificación |
| :--- | :--- | :--- |
| Sin conexión a Internet | `nas update` avisa que no se pudo contactar con GitHub y no rompe el asistente. | Revisión de código |
| `git fetch` falla | Se informa del fallo en lugar de decir "ya actualizado". | Revisión de código |
| Falta `ca-certificates` | El README indica instalarlo; el error de `curl` es explícito. | Manual |

### API web

| Fallo | Comportamiento esperado | Verificación |
| :--- | :--- | :--- |
| IP, recurso, ruta, usuario o cron inválidos | Se responde con un error JSON y **no se escribe ningún archivo**. | `test_api.py` |
| Ruta con `..` o raíz `/` | Se rechaza. | `test_api.py` |
| Sin permisos de escritura | Se responde con un error JSON en lugar de una traza. | Revisión de código |
| Carga de tareas fallida en el panel | Se muestra el error real, no una lista vacía. | Revisión de código |

## Ejecutar las pruebas

```bash
bats tests/          # pruebas de shell (incluye inyección de fallos)
pytest tests/ -q     # pruebas del backend web
```

## Principios de las pruebas

- Se prueba el **artefacto real** (runners generados y funciones del proyecto), no copias.
- Cada prueba verifica el **resultado del fallo** (mensaje, ausencia de artefactos parciales, estado consistente), no solo el código de salida.
- Las pruebas son **herméticas**: sin red, sin `root` y sin rutas del sistema.
- Cada modo de fallo apunta a una prueba concreta o a un paso manual documentado.
