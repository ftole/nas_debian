# Auditoría de Preparación para Producción — Servidor NAS Debian 13

**Fecha:** 2026-10-09
**Objetivo:** Evaluar si el proyecto/servidor (`10.10.1.222`, rol `ARCHIVOS`) está listo para producción en una **LAN corporativa**.
**Alcance:** Auditoría completa (7 ejes) ejecutada en el servidor con herramientas read-only/lastre: `lynis 3.1.4`, `smartctl 7.4`, `nmap 7.95`, `debsecan`, CPU/probing de red.
**Método:** OWASP ASVS 5.0 Nivel 2 + WSTG, CIS Debian 13 (Nivel 1/2 vía Lynis), guías oficiales de Samba, regla de respaldo 3-2-1-1-0.

---

## 1. Veredicto

> **GO CONDICIONADO** — aprobado para producción LAN **si se corrigen 2 hallazgos Altos** (H1 logrotate, H2 off-site) **y se cierran/planifican los Medios** priorizados (sección 4).

No se detectaron vulnerabilidades **Críticas**. Los controles de seguridad críticos de la aplicación están correctamente implementados y fueron verificados de forma dinámica.

| Criterio de salida | Estado |
|---|---|
| Críticos abiertos | **0** |
| Altos (Ejes A–C) | 0 (los Altos son Eje D/E: logrotate, off-site) |
| Restauración de respaldo | **Verificada** (hashes idénticos) |
| Monitoreo de discos (SMART) | **Activo** (smartd/timers) |
| RTO/RPO documentados | **Pendiente** (recomendado) |
| Medios | Plan fechado (abajo) |

---

## 2. Controles positivos verificados (con evidencia)

| Control | Resultado |
|---|---|
| Ejecución de comandos sin shell (`proc_open` con arrays en `SystemService::runCommand/sudo`) | ✔ Sin superficie de inyección de comandos |
| Path traversal en `/api/files/*` (`resolveSubpath` elimina `..` y `\`) | ✔ Probado: `../../etc/passwd` y `%2e%2e%2f` → 400/404, **sin fuga** |
| CSRF en operaciones mutantes | ✔ POST sin token → **403** |
| Validación de nombres de recurso (regex alfanumérica) | ✔ `prueba;id`, `x<script>`, `a b` rechazados |
| Aislamiento de rol **operator** (`grp_web`) | ✔ 403 en `/api/storage`, `/api/terminal/exec`, `/api/domain`, `/api/system/reboot`, `/api/system/updates`, `/api/backups` |
| Cabeceras de seguridad HTTP | ✔ CSP, X-Frame-Options SAMEORIGIN, nosniff, X-XSS, Referrer-Policy, Permissions-Policy — **✘ HSTS ausente** (ver M2) |
| TLS | ✔ Solo TLSv1.2/1.3, cifrados `HIGH:!aNULL:!MD5` |
| Integridad/cadena | ✔ CI (ShellCheck+BATS+Flake8+Pytest+PHP), tags GPG firmadas en `install/updater`, `.env` en `.gitignore` |
| Auditoría del kernel | ✔ Reglas auditd (sudoers, smb.conf, credenciales, nas-terminal) y AIDE con revisión diaria (`dailyaidecheck.timer`) |
| Respaldo | ✔ Snapshot inmutable (`chattr +i`), deduplicación por hardlinks (inodo compartido, `links=2`), **restauración verificada con hashes IGUALES** |
| Host base | ✔ `PermitRootLogin no`, AppArmor activo, fail2ban activo, unattended-upgrades, journald persistente |

---

## 3. Hallazgos clasificados

### Críticos
Ninguno.

### Altos

**H1 — `logrotate` de logs de backup falla cada noche (no rotan).**
- Evidencia: `systemctl status logrotate` → `error: skipping "/srv/nas/LOGS_BACKUP/*.log" because parent directory has insecure permissions ... Set "su" directive`.
- Causa: `/srv/nas/LOGS_BACKUP` es `2770 root:grp_samba` (gravable por grupo) y el perfil no declara `su`.
- Impacto: los logs del backup **nunca se rotan**; riesgo de llenar el disco a largo plazo. Contradice la documentación del proyecto.
- Mitigación: añadir `su root grp_samba` (o `su www-data grp_samba`) a `/etc/logrotate.d/nas-backups` (revisar `nas-admin`/`nas-deploy`). **Corregir en `deploy.sh` + aplicar en producción.**

**H2 — Sin copia off-site / regla 3-2-1 (todo reside en un mismo host/disco).**
- Evidencia: `/srv/nas` es el único destino; no hay réplica remota ni medio alterno.
- Impacto: fallo del disco o ransomware destruye datos **y** respaldos a la vez (los snapshots, aunque inmutables, viven en el mismo dispositivo).
- Mitigación: programar réplica incremental a otro host/medio (rsync/SSH) con retención, o copia a nube in‑mutable; verificar restauración mensualmente.

### Medios

**M1 — SSH por contraseña habilitado + X11Forwarding + sin timeout idle.**
- `passwordauthentication yes`, `X11Forwarding yes`, `ClientAliveInterval 0`, sin `AllowUsers`.
- LAN mitigado, pero conviene: solo clave pública, `AllowUsers`/`AllowGroups` restringido, `ClientAliveInterval 300`, `X11Forwarding no`.

**M2 — HSTS ausente pese a estar documentado; certificado autofirmado (10 años).**
- No existe `add_header Strict-Transport-Security` en el bloque SSL de nginx (AGENTS.md lo afirma).
- Corregir en `deploy.sh`; el autofirmado es aceptable en LAN, pero rompe HSTS real y genera avisos en clientes.

**M3 — Servicios/NFS y exim4 activos sin necesidad + regla UFW fantasma 9090.**
- `nfs-server/nfs-mountd/rpcbind` (111, 2049) y `exim4` (25) escuchando en `0.0.0.0`; **NFS y 9090 no alcanzables desde LAN** por UFW (verificado), pero amplían superficie.
- UFW permite `9090/tcp` (Cockpit) a *Anywhere* sin que haya servicio → regla muerta.
- Mitigación: `systemctl disable --now nfs-server rpcbind rpcbind.socket exim4` y eliminar la regla 9090.

**M4 — Política de contraseñas y GRUB.**
- `login.defs` sin `PASS_MAX_DAYS/PASS_MIN_DAYS` (cuentas sin caducidad) y sin `UMASK` explícito (022). Sin contraseña de GRUB (acceso físico).
- Mitigación: aplicar caducidad a cuentas de red administradas y `umask 027`; proteger GRUB si hay riesgo físico.

**M5 — Backups deshabilitados en rol `ARCHIVOS` (por diseño).**
- El middleware bloquea `/api/backups` en rol ARCHIVOS. Para usar la central de respaldo en producción, desplegar con rol `ARCHIVOS_BACKUP` o `BACKUP`.
- Nota de **configuración**, no falla; confirmar el rol objetivo antes de producción.

**M6 — PHP `allow_url_fopen = On` y sin `disable_functions`.**
- Recomendado `allow_url_fopen = Off` (la app no consume URLs remotas) y endurecer `disable_functions` para el pool fpm.

### Bajos

- **B1** `version.json` desplegado con `"commit": "dev"` (stale). Generar versión real en deploy.
- **B2** Dependencias Python con rango `>=` (paramiko, keyring); fijar versiones (`==`) y correr `pip-audit`.
- **B3** `vm.swappiness=60`; sin banner legal (`/etc/issue`); sin syslog externo; sin escáner de malware (recomendación Lynis). Instalar `debsums`/`apt-listbugs`.
- **B4** Residuos de pruebas en el servidor: recurso `[asesor]`, usuarios `juan/asesor/web`, **debsecan reporta CVEs en librerías trixie sin fix aún** (busybox/acl/…; `apt` tiene **0** actualizaciones pendientes — monitorear point releases).

---

## 4. Backlog de remediación priorizado

| # | Acción | Severidad | Archivo / recurso |
|---|---|---|---|
| 1 | Añadir `su root grp_samba` a logrotate nas-backups | Alto | `deploy.sh`, `/etc/logrotate.d/nas-backups` |
| 2 | Réplica off-site + prueba de restauración mensual | Alto | plan operativo |
| 3 | HSTS en nginx SSL + firmado SMB (`server signing = required`, subir `server min protocol`) | Medio | `deploy.sh`, `smb.conf` |
| 4 | SSH solo claves + `AllowUsers` + timeout + X11 off | Medio | `deploy.sh` (sshd_config.d) |
| 5 | Desactivar nfs-server/rpcbind/exim4; quitar regla 9090 | Medio | `deploy.sh` (apt/systemd/ufw) |
| 6 | Caducidad de contraseñas + umask 027 + GRUB | Medio | `login.defs` / `deploy.sh` |
| 7 | `allow_url_fopen = Off` para fpm | Medio | `deploy.sh` (php pool) |
| 8 | `version.json` real en deploy; fijar deps Python; `pip-audit` en CI | Bajo | `deploy.sh`, `requirements-*`, CI |
| 9 | Documentar RTO/RPO y runbook de restauración; limpiar residuos de prueba | Bajo | `docs/` |

---

## 5. Comandos/evidencia clave

```bash
# Herramientas instaladas (auditoría)
apt-get install -y lynis smartmontools nmap debsecan
lynis audit system --quick --no-colors          # Hardening index: 73/100 (284 tests)

# Web dinámico (verificaciones, todos superados salvo HSTS)
curl -skI https://$IP | grep -i strict        # HSTS ausente
# traversal, CSRF, rol operator, inyección → 400/403 (ver sección 2)

# SMB
nmap -p139,445 127.0.0.1 --script smb-protocols,smb2-security-mode
# -> dialectos 2.0.2/3.1.1; "Message signing enabled but not required" (M)

# Respaldo (DR drill superado)
# snapshot inmutable (chattr +i), dedup por hardlinks, restore con hashes idénticos.

# Red desde LAN (solo estos abiertos): 22, 80, 443, 139, 445, 5355
#   NFS(111/2049), 9090, 5357 → closed/filtered (contenidos por UFW)
```

---

## 6. Condiciones de salida de producción

1. H1 logrotate corregido y validado (`logrotate -d` sin errores; `logrotate.service` OK).
2. Plan fechado de off-site (H2) + restauración de prueba mensual documentada.
3. M1–M6 aplicados o con fecha acordada (rodar en entorno de pruebas 2 semanas).
4. RTO/RPO definidos y runbook de restauración publicado.
5. Despliegue final con rol adecuado (`ARCHIVOS_BACKUP` si se usará la central de respaldos).

Con estos puntos cerrados, el proyecto puede certificarse **producción apta** en su contexto LAN.