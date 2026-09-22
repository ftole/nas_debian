#!/usr/bin/env python3
import sys, os, glob, json, subprocess, re
from datetime import datetime

CRED_DIR = "/etc/backup-credentials"
BIN_DIR = "/usr/local/bin"
CRON_DIR = "/etc/cron.d"
BKP_ROOT = "/srv/nas/BACKUPS_HISTORICOS"
LOG_ROOT = "/srv/nas/LOGS_BACKUP"
# known_hosts dedicado y protegido para las tareas de backup por SSH.
KNOWN_HOSTS = "/root/.ssh/known_hosts_backup"

def ensure_dirs():
    try:
        os.makedirs(CRED_DIR, exist_ok=True)
        os.makedirs(BKP_ROOT, exist_ok=True)
        os.makedirs(LOG_ROOT, exist_ok=True)
        return True
    except OSError:
        return False

def ensure_known_hosts():
    """Inicializa el known_hosts dedicado con propietario root y permisos 0600."""
    try:
        ssh_dir = os.path.dirname(KNOWN_HOSTS)
        os.makedirs(ssh_dir, mode=0o700, exist_ok=True)
        os.chmod(ssh_dir, 0o700)
        if not os.path.exists(KNOWN_HOSTS):
            with open(KNOWN_HOSTS, "a"):
                pass
        try:
            os.chown(KNOWN_HOSTS, 0, 0)
        except OSError:
            pass
        os.chmod(KNOWN_HOSTS, 0o600)
        return True
    except OSError:
        return False

def list_tasks():
    ensure_dirs()
    tasks = []
    runners = sorted(glob.glob(f"{BIN_DIR}/backup_*.sh"))
    
    for r in runners:
        tname = os.path.basename(r).replace("backup_", "").replace(".sh", "")
        proto = "Local"
        src = "N/A"
        ret = 30
        
        try:
            with open(r, "r", encoding="utf-8", errors="ignore") as f:
                content = f.read()
                if "SRC_SHARE=" in content:
                    proto = "Windows (CIFS)"
                    m_ip = re.search(r'SRC_IP="([^"]+)"', content)
                    m_sh = re.search(r'SRC_SHARE="([^"]+)"', content)
                    if m_ip and m_sh:
                        src = f"//{m_ip.group(1)}/{m_sh.group(1)}"
                elif "SRC_PORT=" in content or "sshpass" in content:
                    proto = "Linux (SSH)"
                    m_ip = re.search(r'SRC_IP="([^"]+)"', content)
                    m_pt = re.search(r'SRC_PATH="([^"]+)"', content)
                    m_us = re.search(r'SRC_USER="([^"]+)"', content)
                    if m_ip and m_pt:
                        src = f"{m_us.group(1)}@{m_ip.group(1)}:{m_pt.group(1)}"
                else:
                    proto = "Local"
                    m_pt = re.search(r'SRC_PATH="([^"]+)"', content)
                    if m_pt: src = m_pt.group(1)
                
                m_ret = re.search(r'RETENTION=(\d+)', content)
                if m_ret: ret = int(m_ret.group(1))
        except Exception as e:
            continue

        cron_file = f"{CRON_DIR}/backup_{tname}"
        cron_sched = "Manual"
        if os.path.exists(cron_file):
            try:
                with open(cron_file, "r") as cf:
                    parts = cf.read().strip().split()
                    if len(parts) >= 5:
                        cron_sched = " ".join(parts[:5])
            except:
                pass

        log_file = f"{LOG_ROOT}/backup_{tname}.log"
        last_run = "Nunca"
        last_status = "Pendiente"
        if os.path.exists(log_file):
            try:
                mtime = os.path.getmtime(log_file)
                last_run = datetime.fromtimestamp(mtime).strftime("%Y-%m-%d %H:%M")
                with open(log_file, "r", encoding="utf-8", errors="ignore") as lf:
                    lines = lf.readlines()[-10:]
                    full_log = "".join(lines)
                    if "FINALIZADO CON ÉXITO" in full_log or "FINALIZADO:" in full_log:
                        last_status = "Éxito"
                    elif "error" in full_log.lower() or "failed" in full_log.lower():
                        last_status = "Fallo"
                    else:
                        last_status = "En progreso"
            except:
                pass

        # Conteo de snapshots
        snap_count = len(glob.glob(f"{BKP_ROOT}/{tname}/snapshot_*"))

        tasks.append({
            "id": tname,
            "proto": proto,
            "src": src,
            "cron": cron_sched,
            "retention": ret,
            "last_run": last_run,
            "last_status": last_status,
            "snaps": snap_count
        })
    
    print(json.dumps({"status": "ok", "tasks": tasks}))

def _redact(msg, secret):
    """Elimina un secreto del texto para no exponerlo en logs ni respuestas."""
    if secret and isinstance(msg, str):
        return msg.replace(secret, "***")
    return msg


def test_cifs(ip, share, user, password):
    if not _valid_host(ip) or not _valid_share(share) or not _valid_user(user):
        print(json.dumps({"status": "error", "message": "Datos de conexión con formato inválido."}))
        return
    try:
        env = os.environ.copy()
        env["USER"] = user
        env["PASSWD"] = password
        cmd = ["timeout", "7", "smbclient", f"//{ip}/{share}", "-c", "dir"]
        res = subprocess.run(cmd, capture_output=True, text=True, env=env)
        if res.returncode == 0:
            print(json.dumps({"status": "ok", "message": "Conexión CIFS/SMB exitosa."}))
        elif res.returncode == 124:
            print(json.dumps({"status": "error", "message": "Error: Tiempo de espera agotado (7s). Verifica la IP."}))
        else:
            err = res.stderr or res.stdout
            print(json.dumps({"status": "error", "message": f"Error de conexión: {_redact(err.strip(), password)}"}))
    except Exception as e:
        print(json.dumps({"status": "error", "message": f"Excepción: {str(e)}"}))

def test_ssh(ip, port, user, password):
    if not _valid_host(ip) or not _valid_user(user):
        print(json.dumps({"status": "error", "message": "Datos de conexión con formato inválido."}))
        return
    try:
        port = int(port)
    except (ValueError, TypeError):
        port = 22
    if port < 1 or port > 65535:
        print(json.dumps({"status": "error", "message": "Puerto SSH inválido."}))
        return
    if not ensure_known_hosts():
        print(json.dumps({"status": "error", "message": "No se pudo preparar el archivo de huellas SSH."}))
        return
    try:
        env = os.environ.copy()
        env["SSHPASS"] = password
        cmd = ["sshpass", "-e", "ssh", "-p", str(port),
               "-o", "StrictHostKeyChecking=accept-new",
               "-o", "UserKnownHostsFile=" + KNOWN_HOSTS,
               "-o", "ConnectTimeout=7", f"{user}@{ip}", "echo OK"]
        res = subprocess.run(cmd, capture_output=True, text=True, env=env)
        if res.returncode == 0 and "OK" in res.stdout:
            print(json.dumps({"status": "ok", "message": "Conexión SSH exitosa."}))
        else:
            err = res.stderr or res.stdout or "Tiempo de espera agotado"
            print(json.dumps({"status": "error", "message": f"Error SSH: {_redact(err.strip(), password)}"}))
    except Exception as e:
        print(json.dumps({"status": "error", "message": f"Excepción: {str(e)}"}))

def _valid_host(value):
    return bool(re.fullmatch(r'[A-Za-z0-9._-]+', value or ""))

def _valid_share(value):
    return bool(re.fullmatch(r'[A-Za-z0-9_$.-]+', value or ""))

def _valid_path(value):
    value = value or ""
    if value == "/" or not re.fullmatch(r'/[A-Za-z0-9._/-]*', value):
        return False
    return ".." not in value.split("/")

def _sanitize_name(name):
    return re.sub(r'[^A-Za-z0-9_-]', '_', name or "")

def _valid_user(value):
    return bool(re.fullmatch(r'[A-Za-z0-9._@-]+', value or ""))

def _valid_cron(value):
    parts = (value or "").split()
    if len(parts) != 5:
        return False
    return all(re.fullmatch(r'[0-9*,/-]+', p) for p in parts)

def create_task(data):
    if not ensure_dirs():
        print(json.dumps({"status": "error", "message": "Sin permisos para preparar los directorios de backup. Verifica la escalada de privilegios."}))
        return
    tname = _sanitize_name(data.get("id", ""))
    if not tname or tname.strip("_") == "":
        print(json.dumps({"status": "error", "message": "Nombre de tarea inválido."}))
        return

    proto = data.get("proto", "cifs")
    cron_expr = data.get("cron", "0 23 * * *")
    try:
        retention = int(data.get("retention", 30))
    except (ValueError, TypeError):
        retention = 30
    if retention < 1:
        retention = 30
    if not _valid_cron(cron_expr):
        print(json.dumps({"status": "error", "message": "Expresión cron inválida."}))
        return
    runner = f"{BIN_DIR}/backup_{tname}.sh"
    cron_file = f"{CRON_DIR}/backup_{tname}"
    cred_file = f"{CRED_DIR}/{tname}.cred"
    existed = os.path.exists(runner)

    if proto == "cifs":
        ip = (data.get("ip") or "").strip()
        share = (data.get("share") or "").replace("/", "").strip()
        user = data.get("user", "Administrador")
        pwd = data.get("password", "")

        if not _valid_host(ip) or not _valid_share(share):
            print(json.dumps({"status": "error", "message": "IP o recurso compartido con formato inválido."}))
            return
        if not _valid_user(user):
            print(json.dumps({"status": "error", "message": "Usuario con formato inválido."}))
            return
        if "\n" in pwd or "\r" in pwd:
            print(json.dumps({"status": "error", "message": "La contraseña contiene caracteres inválidos."}))
            return

        with open(cred_file, "w") as f:
            f.write(f"username={user}\npassword={pwd}\n")
        os.chmod(cred_file, 0o600)

        script = f"""#!/bin/bash
set -e
TASK="{tname}"
SRC_IP="{ip}"
SRC_SHARE="{share}"
CRED_FILE="{cred_file}"
MOUNT_POINT="${{MOUNT_ROOT:-/mnt/backup_sources}}/$TASK"
BKP_DIR="{BKP_ROOT}/$TASK"
LOG_FILE="{LOG_ROOT}/backup_${{TASK}}.log"
RETENTION={retention}
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
TARGET_SNAPSHOT="$BKP_DIR/snapshot_$DATE_STR"
trap 'umount "$MOUNT_POINT" 2>/dev/null || true' EXIT

exec 9>"${{LOCK_DIR:-/var/lock}}/backup_${{TASK}}.lock"
flock -n 9 || {{ echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso ($DATE_STR) ===" >> "$LOG_FILE"; exit 0; }}

echo "=== INICIANDO BACKUP CIFS: $TASK ($DATE_STR) ===" >> "$LOG_FILE"
mkdir -p "$MOUNT_POINT" "$BKP_DIR"
DISPONIBLE_KB=$(df -Pk "$BKP_DIR" 2>/dev/null | awk 'NR==2 {{print $4}}')
if [ -n "$DISPONIBLE_KB" ] && [ "$DISPONIBLE_KB" -lt 524288 ]; then
    echo "=== ABORTADO: espacio libre insuficiente en $BKP_DIR ($((DISPONIBLE_KB / 1024)) MB) ===" >> "$LOG_FILE"
    exit 1
fi
umount "$MOUNT_POINT" 2>/dev/null || true

CRED_OWNER=$(stat -c '%U:%G' "$CRED_FILE" 2>/dev/null)
CRED_MODE=$(stat -c '%a' "$CRED_FILE" 2>/dev/null)
if [ "$CRED_OWNER" != "root:root" ] || [ "$CRED_MODE" != "600" ]; then
    echo "=== ABORTADO: propietario o permisos inseguros en $CRED_FILE ===" >> "$LOG_FILE"
    exit 1
fi
mount -t cifs "//$SRC_IP/$SRC_SHARE" "$MOUNT_POINT" -o credentials="$CRED_FILE",ro,iocharset=utf8,vers=3.0,sec=ntlmssp 2>> "$LOG_FILE"

LAST_SNAPSHOT=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | sort | tail -n 1 || echo "")
LINK_DEST_OPT=""
if [ -n "$LAST_SNAPSHOT" ] && [ -d "$LAST_SNAPSHOT" ]; then
    LINK_DEST_OPT="--link-dest=$LAST_SNAPSHOT"
    echo " -> Deduplicando con hardlinks contra: $(basename "$LAST_SNAPSHOT")" >> "$LOG_FILE"
fi

if ! rsync -a --delete $LINK_DEST_OPT "$MOUNT_POINT/" "$TARGET_SNAPSHOT/" >> "$LOG_FILE" 2>&1; then
    echo "=== BACKUP FALLIDO: se descarta el snapshot parcial ===" >> "$LOG_FILE"
    if [ -n "$TARGET_SNAPSHOT" ] && [ "$TARGET_SNAPSHOT" != "/" ]; then
        rm -rf "$TARGET_SNAPSHOT"
    fi
    exit 1
fi
umount "$MOUNT_POINT" 2>/dev/null || true

SNAPSHOT_COUNT=$(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | wc -l)
if [ "$SNAPSHOT_COUNT" -gt "$RETENTION" ]; then
    while IFS= read -r old; do
        [ -n "$old" ] || continue
        if [ "${{old#"$BKP_DIR"/}}" = "$old" ]; then
            continue
        fi
        echo " -> Rotando snapshot antiguo: $(basename "$old")" >> "$LOG_FILE"
        rm -rf "$old"
    done < <(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | sort | head -n -"$RETENTION")
fi

echo "=== BACKUP FINALIZADO CON ÉXITO: $DATE_STR ===" >> "$LOG_FILE"
"""
    elif proto == "ssh":
        ip = (data.get("ip") or "").strip()
        rpath = (data.get("path") or "/var/www").strip()
        user = data.get("user", "root")
        pwd = data.get("password", "")

        try:
            port = int(data.get("port", 22))
        except (ValueError, TypeError):
            port = 22
        if port < 1 or port > 65535:
            print(json.dumps({"status": "error", "message": "Puerto SSH inválido."}))
            return
        if not _valid_host(ip) or not _valid_path(rpath):
            print(json.dumps({"status": "error", "message": "IP o ruta remota con formato inválido."}))
            return
        if not _valid_user(user):
            print(json.dumps({"status": "error", "message": "Usuario con formato inválido."}))
            return
        if "\n" in pwd or "\r" in pwd:
            print(json.dumps({"status": "error", "message": "La contraseña contiene caracteres inválidos."}))
            return
        if not ensure_known_hosts():
            print(json.dumps({"status": "error", "message": "No se pudo preparar el archivo de huellas SSH."}))
            return

        with open(cred_file, "w") as f:
            f.write(pwd)
        os.chmod(cred_file, 0o600)

        script = f"""#!/bin/bash
set -e
TASK="{tname}"
SRC_IP="{ip}"
SRC_PORT="{port}"
SRC_PATH="{rpath}"
SRC_USER="{user}"
CRED_FILE="{cred_file}"
BKP_DIR="{BKP_ROOT}/$TASK"
LOG_FILE="{LOG_ROOT}/backup_${{TASK}}.log"
RETENTION={retention}
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
TARGET_SNAPSHOT="$BKP_DIR/snapshot_$DATE_STR"

exec 9>"${{LOCK_DIR:-/var/lock}}/backup_${{TASK}}.lock"
flock -n 9 || {{ echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso ($DATE_STR) ===" >> "$LOG_FILE"; exit 0; }}

echo "=== INICIANDO BACKUP SSH: $TASK ($DATE_STR) ===" >> "$LOG_FILE"
mkdir -p "$BKP_DIR"
DISPONIBLE_KB=$(df -Pk "$BKP_DIR" 2>/dev/null | awk 'NR==2 {{print $4}}')
if [ -n "$DISPONIBLE_KB" ] && [ "$DISPONIBLE_KB" -lt 524288 ]; then
    echo "=== ABORTADO: espacio libre insuficiente en $BKP_DIR ($((DISPONIBLE_KB / 1024)) MB) ===" >> "$LOG_FILE"
    exit 1
fi

LAST_SNAPSHOT=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | sort | tail -n 1 || echo "")
LINK_DEST_OPT=""
if [ -n "$LAST_SNAPSHOT" ] && [ -d "$LAST_SNAPSHOT" ]; then
    LINK_DEST_OPT="--link-dest=$LAST_SNAPSHOT"
    echo " -> Deduplicando con hardlinks contra: $(basename "$LAST_SNAPSHOT")" >> "$LOG_FILE"
fi

CRED_OWNER=$(stat -c '%U:%G' "$CRED_FILE" 2>/dev/null)
CRED_MODE=$(stat -c '%a' "$CRED_FILE" 2>/dev/null)
if [ "$CRED_OWNER" != "root:root" ] || [ "$CRED_MODE" != "600" ]; then
    echo "=== ABORTADO: propietario o permisos inseguros en $CRED_FILE ===" >> "$LOG_FILE"
    exit 1
fi
if ! SSHPASS=$(cat "$CRED_FILE") sshpass -e rsync -avz -e "ssh -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile={KNOWN_HOSTS}" --delete $LINK_DEST_OPT "$SRC_USER@$SRC_IP:$SRC_PATH/" "$TARGET_SNAPSHOT/" >> "$LOG_FILE" 2>&1; then
    echo "=== BACKUP FALLIDO: se descarta el snapshot parcial ===" >> "$LOG_FILE"
    if [ -n "$TARGET_SNAPSHOT" ] && [ "$TARGET_SNAPSHOT" != "/" ]; then
        rm -rf "$TARGET_SNAPSHOT"
    fi
    exit 1
fi

SNAPSHOT_COUNT=$(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | wc -l)
if [ "$SNAPSHOT_COUNT" -gt "$RETENTION" ]; then
    while IFS= read -r old; do
        [ -n "$old" ] || continue
        if [ "${{old#"$BKP_DIR"/}}" = "$old" ]; then
            continue
        fi
        echo " -> Rotando snapshot antiguo: $(basename "$old")" >> "$LOG_FILE"
        rm -rf "$old"
    done < <(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | sort | head -n -"$RETENTION")
fi

echo "=== BACKUP FINALIZADO CON ÉXITO: $DATE_STR ===" >> "$LOG_FILE"
"""
    else:
        lpath = (data.get("path") or "/srv/nas/SISTEMAS").strip()
        if not _valid_path(lpath):
            print(json.dumps({"status": "error", "message": "La ruta local tiene un formato inválido."}))
            return
        script = f"""#!/bin/bash
set -e
TASK="{tname}"
SRC_PATH="{lpath}"
BKP_DIR="{BKP_ROOT}/$TASK"
LOG_FILE="{LOG_ROOT}/backup_${{TASK}}.log"
RETENTION={retention}
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
TARGET_SNAPSHOT="$BKP_DIR/snapshot_$DATE_STR"

exec 9>"${{LOCK_DIR:-/var/lock}}/backup_${{TASK}}.lock"
flock -n 9 || {{ echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso ($DATE_STR) ===" >> "$LOG_FILE"; exit 0; }}

echo "=== INICIANDO BACKUP LOCAL: $TASK ($DATE_STR) ===" >> "$LOG_FILE"
mkdir -p "$BKP_DIR"
DISPONIBLE_KB=$(df -Pk "$BKP_DIR" 2>/dev/null | awk 'NR==2 {{print $4}}')
if [ -n "$DISPONIBLE_KB" ] && [ "$DISPONIBLE_KB" -lt 524288 ]; then
    echo "=== ABORTADO: espacio libre insuficiente en $BKP_DIR ($((DISPONIBLE_KB / 1024)) MB) ===" >> "$LOG_FILE"
    exit 1
fi

LAST_SNAPSHOT=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | sort | tail -n 1 || echo "")
LINK_DEST_OPT=""
if [ -n "$LAST_SNAPSHOT" ] && [ -d "$LAST_SNAPSHOT" ]; then
    LINK_DEST_OPT="--link-dest=$LAST_SNAPSHOT"
    echo " -> Deduplicando con hardlinks contra: $(basename "$LAST_SNAPSHOT")" >> "$LOG_FILE"
fi

if ! rsync -a --delete $LINK_DEST_OPT "$SRC_PATH/" "$TARGET_SNAPSHOT/" >> "$LOG_FILE" 2>&1; then
    echo "=== BACKUP FALLIDO: se descarta el snapshot parcial ===" >> "$LOG_FILE"
    if [ -n "$TARGET_SNAPSHOT" ] && [ "$TARGET_SNAPSHOT" != "/" ]; then
        rm -rf "$TARGET_SNAPSHOT"
    fi
    exit 1
fi

SNAPSHOT_COUNT=$(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | wc -l)
if [ "$SNAPSHOT_COUNT" -gt "$RETENTION" ]; then
    while IFS= read -r old; do
        [ -n "$old" ] || continue
        if [ "${{old#"$BKP_DIR"/}}" = "$old" ]; then
            continue
        fi
        echo " -> Rotando snapshot antiguo: $(basename "$old")" >> "$LOG_FILE"
        rm -rf "$old"
    done < <(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | sort | head -n -"$RETENTION")
fi

echo "=== BACKUP FINALIZADO CON ÉXITO: $DATE_STR ===" >> "$LOG_FILE"
"""

    with open(runner, "w") as f:
        f.write(script)
    os.chmod(runner, 0o750)

    cron_line = (
        f"{cron_expr} root systemd-run --collect --unit=backup-{tname} "
        f"--slice=backups.slice -p CPUSchedulingPolicy=batch -p IOSchedulingClass=idle "
        f"bash {runner} >/dev/null 2>&1\n"
    )
    with open(cron_file, "w") as f:
        f.write(cron_line)
    os.chmod(cron_file, 0o644)

    print(json.dumps({"status": "ok", "message": f"Tarea '{tname}' {'actualizada' if existed else 'programada'} exitosamente."}))

def delete_task(tname):
    tname = _sanitize_name(tname)
    runner = f"{BIN_DIR}/backup_{tname}.sh"
    cron_file = f"{CRON_DIR}/backup_{tname}"
    cred_file = f"{CRED_DIR}/{tname}.cred"

    if os.path.exists(runner): os.remove(runner)
    if os.path.exists(cron_file): os.remove(cron_file)
    if os.path.exists(cred_file): os.remove(cred_file)

    print(json.dumps({"status": "ok", "message": f"Tarea '{tname}' eliminada."}))

def read_logs(tname):
    tname = _sanitize_name(tname)
    log_file = f"{LOG_ROOT}/backup_{tname}.log"
    if os.path.exists(log_file):
        try:
            with open(log_file, "r", encoding="utf-8", errors="ignore") as f:
                content = "".join(f.readlines()[-200:])
            print(json.dumps({"status": "ok", "logs": content}))
        except Exception as e:
            print(json.dumps({"status": "error", "logs": str(e)}))
    else:
        print(json.dumps({"status": "ok", "logs": "(No se han generado registros todavía)"}))

def _read_payload():
    try:
        if sys.stdin.isatty():
            return {}
        raw = sys.stdin.read()
    except Exception:
        raw = ""
    if not raw.strip():
        return {}
    try:
        return json.loads(raw)
    except ValueError:
        return {}

if __name__ == "__main__":
    if len(sys.argv) < 2:
        list_tasks()
        sys.exit(0)

    try:
        action = sys.argv[1]
        if action == "list":
            list_tasks()
        elif action == "delete":
            delete_task(sys.argv[2])
        elif action == "logs":
            read_logs(sys.argv[2])
        elif action == "create":
            create_task(_read_payload())
        elif action == "test_cifs":
            data = _read_payload()
            test_cifs(data.get("ip"), data.get("share"), data.get("user"), data.get("password"))
        elif action == "test_ssh":
            data = _read_payload()
            test_ssh(data.get("ip"), data.get("port", 22), data.get("user"), data.get("password"))
        else:
            print(json.dumps({"status": "error", "message": f"Acción desconocida: {action}"}))
    except (IndexError, KeyError) as e:
        print(json.dumps({"status": "error", "message": f"Parámetros incompletos: {str(e)}"}))
    except OSError as e:
        print(json.dumps({"status": "error", "message": f"Error de sistema de archivos: {str(e)}"}))
    except ValueError as e:
        print(json.dumps({"status": "error", "message": f"Entrada inválida: {str(e)}"}))
