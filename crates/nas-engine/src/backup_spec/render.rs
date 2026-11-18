//! Generación de runners Bash, archivos cron, credenciales y claves SSH para tareas de respaldo.

use super::{BackupSpec, Proto};

impl BackupSpec {
    /// Runner bash listo para instalar, compatible con `BackupTask::parse`.
    pub fn render_runner(&self, proto: Proto) -> String {
        let id = self.id.trim();
        let dest_root = self.destination_root();
        let dest = self.destination();
        let cred = match proto {
            Proto::Cifs | Proto::Ssh => format!("/etc/backup-credentials/{id}.cred"),
            Proto::Local => String::new(),
        };
        let key_file = match proto {
            Proto::Ssh => {
                if let Some(k) = self.ssh_key.as_deref().filter(|s| !s.trim().is_empty()) {
                    let k = k.trim();
                    if k.starts_with("-----BEGIN") {
                        format!("/etc/backup-credentials/{id}.id_ed25519")
                    } else {
                        k.to_string()
                    }
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };
        let port = self.port.unwrap_or(22);

        let (src_type, src_ip, src_share, src_path, src_user) = match proto {
            Proto::Cifs => (
                "cifs",
                self.ip.trim(),
                self.share.trim(),
                "",
                self.user.trim(),
            ),
            Proto::Ssh => (
                "ssh",
                self.ip.trim(),
                "",
                self.path.trim().trim_end_matches('/'),
                self.user.trim(),
            ),
            Proto::Local => ("local", "", "", self.path.trim().trim_end_matches('/'), ""),
        };

        let source_setup = match proto {
            Proto::Cifs => {
                "mkdir -p \"$MOUNT\"\nif mountpoint -q \"$MOUNT\" 2>/dev/null; then umount -f \"$MOUNT\" 2>/dev/null || umount -l \"$MOUNT\" 2>/dev/null || true; fi\nmount -t cifs \"//$SRC_IP/$SRC_SHARE\" \"$MOUNT\" -o \"credentials=$CRED,ro,iocharset=utf8,vers=3.1.1,noserverino,cache=none,soft,nobrl,file_mode=0777,dir_mode=0777\" 2>> \"$LOG\"\nSRC=\"$MOUNT/\"".to_string()
            }
            Proto::Ssh => "SRC=\"$SRC_USER@$SRC_IP:${SRC_PATH:-/}/\"".to_string(),
            Proto::Local => "SRC=\"${SRC_PATH:-/}/\"".to_string(),
        };

        let rsync = match proto {
            Proto::Ssh => {
                "mkdir -p /root/.ssh\nchmod 0700 /root/.ssh\nRSYNC_RC=0\nif [ -n \"$KEY_FILE\" ] && [ -f \"$KEY_FILE\" ]; then\n    nice -n 10 ionice -c 2 -n 7 rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -e \"ssh -i $KEY_FILE -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}\nelif [ -f \"/etc/backup-credentials/$TASK.id_ed25519\" ]; then\n    nice -n 10 ionice -c 2 -n 7 rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -e \"ssh -i /etc/backup-credentials/$TASK.id_ed25519 -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}\nelse\n    SSHPASS=\"\"\n    if [ -f \"$CRED\" ]; then\n        IFS= read -r SSHPASS < \"$CRED\" || true\n    fi\n    export SSHPASS\n    nice -n 10 ionice -c 2 -n 7 sshpass -e rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -e \"ssh -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}\n    unset SSHPASS\nfi".to_string()
            }
            _ => {
                "RSYNC_RC=0\nnice -n 10 ionice -c 2 -n 7 rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}".to_string()
            }
        };

        format!(
            r#"#!/bin/bash
set -euo pipefail
TASK={q_id}
SOURCE_TYPE={q_type}
SRC_IP={q_ip}
SRC_SHARE={q_share}
SRC_PATH={q_path}
SRC_USER={q_user}
SRC_PORT={port}
RETENTION={retention}
AUTO_PERMANENT={auto_permanent}
DEST_ROOT={q_dest_root}
LOG_DIR="${{LOG_DIR:-/srv/nas/LOGS_BACKUP}}"
CRED_FILE={q_cred}
KEY_FILE={q_key}
MODE="$SOURCE_TYPE"
CRED="$CRED_FILE"
# Destino: {dest}
BKP_DIR="${{BKP_DIR:-$DEST_ROOT/$TASK}}"
PERM_DIR="${{PERM_DIR:-${{DEST_ROOT/BACKUPS_HISTORICOS/BACKUPS_PERMANENTES}}/$TASK}}"
LOG="${{LOG_FILE:-$LOG_DIR/backup_${{TASK}}.log}}"
MOUNT="${{MOUNT_ROOT:-/mnt/backup_sources}}/$TASK"
PROG_FILE="/run/nas-backup-$TASK.progress"
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
START_TIME=$(date +%s)
STAGE="$BKP_DIR/.inprogress_$DATE_STR"
FINAL="$BKP_DIR/snapshot_$DATE_STR"
SPACE_ABORT_FLAG="$BKP_DIR/.space_abort_$DATE_STR"
SPACE_WATCHDOG_PID=""
SNAPSHOT_OK=false

log() {{ printf '[%s] [%s] %s\n' "$(date '+%F %T')" "$TASK" "$*" >> "$LOG"; }}
cleanup() {{
    local status=$?
    if [ -n "${{SPACE_WATCHDOG_PID:-}}" ]; then
        kill "$SPACE_WATCHDOG_PID" 2>/dev/null || true
        wait "$SPACE_WATCHDOG_PID" 2>/dev/null || true
        SPACE_WATCHDOG_PID=""
    fi
    rm -f "$PROG_FILE" "${{PROG_FILE}}.tmp."* "$SPACE_ABORT_FLAG" 2>/dev/null || true
    if mountpoint -q "$MOUNT" 2>/dev/null; then umount "$MOUNT" || true; fi
    if [ "$status" -eq 143 ] || [ "$status" -eq 130 ]; then
        log "=== RESPALDO CANCELADO: la ejecucion fue detenida manualmente por el usuario (codigo de salida $status) ==="
        rm -rf -- "$STAGE" "$FINAL"
    elif [ "$SNAPSHOT_OK" != "true" ] && [ "$status" -ne 0 ]; then
        log "ERROR: se descarta el snapshot parcial (codigo de salida $status)"
        rm -rf -- "$STAGE" "$FINAL"
    elif [ -d "$STAGE" ]; then
        rm -rf -- "$STAGE"
    fi
    exit "$status"
}}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT

update_progress() {{
    local b pct spd rem rest num_pct diff elap tmp_file
    while read -r b pct spd rem rest; do
        [ -z "$pct" ] && continue
        num_pct="${{pct%%%}}"
        num_pct="${{num_pct//[^0-9]/}}"
        [ -z "$num_pct" ] && continue
        diff=$(($(date +%s) - START_TIME))
        elap=$(printf '%02d:%02d:%02d' $((diff/3600)) $(((diff%3600)/60)) $((diff%60)))
        tmp_file="${{PROG_FILE}}.tmp.$$"
        printf '{{"percent":%d,"speed":"%s","elapsed":"%s","remaining":"%s","bytes":"%s"}}\n' \
            "$num_pct" "${{spd:-0.00kB/s}}" "$elap" "${{rem:---:--:--}}" "${{b//,/}}" > "$tmp_file" 2>/dev/null || true
        chmod 0644 "$tmp_file" 2>/dev/null || true
        mv -f "$tmp_file" "$PROG_FILE" 2>/dev/null || true
    done
}}

mkdir -p "$BKP_DIR" "$LOG_DIR" "$PERM_DIR"
chown root:grp_samba "$PERM_DIR" 2>/dev/null || true
chmod 2770 "$PERM_DIR" 2>/dev/null || true
exec 9>"${{LOCK_DIR:-/var/lock}}/backup_$TASK.lock"
flock -n 9 || {{ log "=== BACKUP OMITIDO: ya hay una ejecucion en curso ($DATE_STR) ==="; exit 0; }}

POOL_SRC=$(df -Pk "$BKP_DIR" 2>/dev/null | awk 'NR==2 {{print $1}}' || true)
POOL_DEV="${{POOL_SRC%%\[*}}"
POOL_KEY=$(printf '%s' "${{POOL_DEV:-$DEST_ROOT}}" | tr -c 'A-Za-z0-9_-' '_')
POOL_SLOTS="${{NAS_BACKUP_POOL_SLOTS:-}}"
if [ -z "$POOL_SLOTS" ]; then
    POOL_SLOTS=1
    if [ -n "$POOL_DEV" ] && [ "${{POOL_DEV#/dev/}}" != "$POOL_DEV" ]; then
        ROTA_OUT=$(lsblk -nso ROTA "$POOL_DEV" 2>/dev/null | tr -d ' ' || true)
        if [ -n "$ROTA_OUT" ] && ! printf '%s\n' "$ROTA_OUT" | grep -qx '1'; then
            POOL_SLOTS=2
        fi
    fi
fi
if [ "$POOL_SLOTS" -lt 1 ] 2>/dev/null; then POOL_SLOTS=1; fi
POOL_LOCKED=false
for ((slot = 1; slot <= POOL_SLOTS; slot++)); do
    exec 8>"${{LOCK_DIR:-/var/lock}}/nas-pool-${{POOL_KEY}}_slot_${{slot}}.lock"
    if flock -n 8; then
        POOL_LOCKED=true
        break
    fi
done
if [ "$POOL_LOCKED" != "true" ]; then
    HASH_NUM=$(printf '%s' "$TASK" | cksum | awk '{{print $1}}')
    WAIT_SLOT=$(( (HASH_NUM % POOL_SLOTS) + 1 ))
    exec 8>"${{LOCK_DIR:-/var/lock}}/nas-pool-${{POOL_KEY}}_slot_${{WAIT_SLOT}}.lock"
    log "esperando turno de escritura en el volumen $POOL_KEY (maximo $POOL_SLOTS simultaneos)"
    if ! flock -w 21600 8; then
        log "=== ABORTADO: tiempo de espera agotado en cola de volumen $POOL_KEY ==="
        exit 1
    fi
fi

LAST_AVAIL_KB=0
LAST_USED_PCT=0
check_volume_space() {{
    local total_kb avail_kb pct min_kb abort_pct warn_pct
    read -r total_kb avail_kb pct < <(df -Pk "$BKP_DIR" 2>/dev/null | awk 'NR==2 {{gsub("%","",$5); print $2, $4, $5}}')
    if [ -z "${{avail_kb:-}}" ] || [ -z "${{pct:-}}" ]; then
        return 0
    fi
    total_kb="${{total_kb:-0}}"
    min_kb=$(( total_kb / 50 ))
    [ "$min_kb" -lt 2097152 ] && min_kb=2097152
    [ "$min_kb" -gt 33554432 ] && min_kb=33554432
    abort_pct=95
    [ "$total_kb" -ge 536870912 ] && abort_pct=97
    [ "$total_kb" -ge 2147483648 ] && abort_pct=98
    warn_pct=$(( abort_pct - 8 ))
    [ "$warn_pct" -lt 85 ] && warn_pct=85
    LAST_AVAIL_KB="$avail_kb"
    LAST_USED_PCT="$pct"
    if [ "$avail_kb" -lt "$min_kb" ] || [ "$pct" -gt "$abort_pct" ]; then
        return 1
    fi
    if [ "${{1:-}}" = "warn" ] && [ "$pct" -gt "$warn_pct" ]; then
        log "alerta preventiva: ocupación del volumen en $pct%"
    fi
    return 0
}}
if ! check_volume_space warn; then
    log "=== ABORTADO: espacio libre insuficiente en $BKP_DIR ($LAST_AVAIL_KB KB, $LAST_USED_PCT%) ==="
    exit 1
fi

mapfile -t PREV_SNAPS < <(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' | LC_ALL=C sort)
LINK_ARGS=()
if [ "${{#PREV_SNAPS[@]}}" -gt 0 ]; then
    LAST="${{PREV_SNAPS[$((${{#PREV_SNAPS[@]}} - 1))]}}"
    LINK_ARGS=("--link-dest=$LAST")
elif [ -d "$PERM_DIR" ]; then
    mapfile -t PREV_PERMS < <(find "$PERM_DIR" -maxdepth 1 -type d -name 'FULL_*' | LC_ALL=C sort)
    if [ "${{#PREV_PERMS[@]}}" -gt 0 ]; then
        LAST_PERM="${{PREV_PERMS[$((${{#PREV_PERMS[@]}} - 1))]}}"
        LINK_ARGS=("--link-dest=$LAST_PERM")
    fi
fi

mkdir -p "$STAGE"
{source_setup}

log "inicio ($MODE): $SRC"
printf '{{"percent":0,"speed":"0.00kB/s","elapsed":"00:00:00","remaining":"--:--:--","bytes":"0"}}\n' > "$PROG_FILE" 2>/dev/null || true
chmod 0644 "$PROG_FILE" 2>/dev/null || true
rm -f "$SPACE_ABORT_FLAG" 2>/dev/null || true
SPACE_CHECK_INTERVAL="${{NAS_BACKUP_SPACE_CHECK_SECS:-15}}"
if [ "$SPACE_CHECK_INTERVAL" -lt 1 ] 2>/dev/null; then SPACE_CHECK_INTERVAL=15; fi
(
    exec 8>&- 9>&-
    trap 'exit 0' TERM INT
    while [ -d "$STAGE" ]; do
        for ((_i = 0; _i < SPACE_CHECK_INTERVAL; _i++)); do
            sleep 1 || exit 0
            [ -d "$STAGE" ] || exit 0
        done
        if ! check_volume_space; then
            printf '1\n' > "$SPACE_ABORT_FLAG" 2>/dev/null || true
            log "=== ABORTADO EN CURSO: espacio libre insuficiente durante la copia ($LAST_AVAIL_KB KB, $LAST_USED_PCT%) ==="
            pkill -TERM -f "$STAGE" 2>/dev/null || true
            sleep 2 || true
            pkill -KILL -f "$STAGE" 2>/dev/null || true
            exit 0
        fi
    done
) &
SPACE_WATCHDOG_PID=$!
{rsync}
if [ -n "${{SPACE_WATCHDOG_PID:-}}" ]; then
    kill "$SPACE_WATCHDOG_PID" 2>/dev/null || true
    wait "$SPACE_WATCHDOG_PID" 2>/dev/null || true
    SPACE_WATCHDOG_PID=""
fi
if [ -f "$SPACE_ABORT_FLAG" ]; then
    rm -f "$SPACE_ABORT_FLAG" 2>/dev/null || true
    exit 1
fi
if [ "$RSYNC_RC" -eq 0 ]; then
    log "replicacion rsync completada con exito (codigo 0)"
elif [ "$RSYNC_RC" -eq 24 ]; then
    log "ADVERTENCIA: algunos archivos desaparecieron del origen durante la copia (codigo 24: ficheros temporales o eliminados mientras se ejecutaba)"
elif [ "$RSYNC_RC" -eq 23 ]; then
    log "ADVERTENCIA: algunos ficheros en uso o bloqueados por usuarios no se pudieron leer (codigo 23: el resto de ficheros se respaldo con exito)"
elif [ "$RSYNC_RC" -ne 0 ]; then
    log "ERROR: en replicación rsync (código $RSYNC_RC)"
    exit "$RSYNC_RC"
fi

if [ -d "$STAGE" ]; then
    mv "$STAGE" "$FINAL"
    SNAPSHOT_OK=true
fi
if btrfs subvolume show "$FINAL" >/dev/null 2>&1; then btrfs property set "$FINAL" ro true 2>/dev/null || true; fi

if [ "$AUTO_PERMANENT" = "true" ]; then
    HAS_PERM=false
    if [ -d "$PERM_DIR" ] && find "$PERM_DIR" -mindepth 1 -maxdepth 1 -type d -name "FULL_*" 2>/dev/null | grep -q .; then
        HAS_PERM=true
    fi

    if [ "$HAS_PERM" != "true" ]; then
        PERM_DEST="$PERM_DIR/FULL_${{DATE_STR}}_INICIAL"
        if cp -al "$FINAL" "$PERM_DEST" 2>/dev/null || cp -a --reflink=always "$FINAL" "$PERM_DEST" 2>/dev/null; then
            PERM_SIZE=$(du -sb "$PERM_DEST" 2>/dev/null | awk 'NR==1{{print $1}}' || true)
            PERM_FILES=$(find "$PERM_DEST" -type f 2>/dev/null | wc -l || true)
            printf '{{"task":"%s","name":"%s","path":"%s","date":"%s","tag":"INICIAL","size_bytes":%s,"files_count":%s}}\n' \
                "$TASK" "FULL_${{DATE_STR}}_INICIAL" "$PERM_DEST" "$DATE_STR" "${{PERM_SIZE:-0}}" "${{PERM_FILES:-0}}" > "$PERM_DEST/.freeze_meta.json" 2>/dev/null || true
            chattr +i "$PERM_DEST" 2>/dev/null || true
            log "congelación inicial automática: FULL_${{DATE_STR}}_INICIAL"
        fi
    else
        DAY_OF_MONTH=$(date +%d)
        if [ "$DAY_OF_MONTH" = "01" ]; then
            MONTH_TAG=$(date +%Y-%m)
            if ! find "$PERM_DIR" -maxdepth 1 -type d -name "FULL_${{MONTH_TAG}}*" 2>/dev/null | grep -q .; then
                PERM_DEST="$PERM_DIR/FULL_${{DATE_STR}}_MENSUAL"
                if cp -al "$FINAL" "$PERM_DEST" 2>/dev/null || cp -a --reflink=always "$FINAL" "$PERM_DEST" 2>/dev/null; then
                    PERM_SIZE=$(du -sb "$PERM_DEST" 2>/dev/null | awk 'NR==1{{print $1}}' || true)
                    PERM_FILES=$(find "$PERM_DEST" -type f 2>/dev/null | wc -l || true)
                    printf '{{"task":"%s","name":"%s","path":"%s","date":"%s","tag":"MENSUAL","size_bytes":%s,"files_count":%s}}\n' \
                        "$TASK" "FULL_${{DATE_STR}}_MENSUAL" "$PERM_DEST" "$DATE_STR" "${{PERM_SIZE:-0}}" "${{PERM_FILES:-0}}" > "$PERM_DEST/.freeze_meta.json" 2>/dev/null || true
                    chattr +i "$PERM_DEST" 2>/dev/null || true
                    log "congelación mensual automática: FULL_${{DATE_STR}}_MENSUAL"
                fi
            fi
        fi
    fi
fi

chattr +i "$FINAL" 2>/dev/null || true
log "snapshot completado: $FINAL"

mapfile -t ALL_SNAPS < <(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' | LC_ALL=C sort)
TOTAL_SNAPS="${{#ALL_SNAPS[@]}}"
if [ "$TOTAL_SNAPS" -gt "$RETENTION" ]; then
    TO_REMOVE=$((TOTAL_SNAPS - RETENTION))
    for ((i = 0; i < TO_REMOVE; i++)); do
        OLD="${{ALL_SNAPS[$i]}}"
        chattr -R -i "$OLD" 2>/dev/null || true
        find "$OLD" -type d -exec chmod u+w {{}} + 2>/dev/null || true
        if rm -rf -- "$OLD"; then
            log "retención: eliminado $(basename "$OLD")"
        fi
    done
    if [ -d "$PERM_DIR" ]; then
        find "$PERM_DIR" -mindepth 1 -maxdepth 1 -type d -name 'FULL_*' -exec chattr +i {{}} + 2>/dev/null || true
    fi
fi
"#,
            q_id = shell_single_quote(id),
            q_type = shell_single_quote(src_type),
            q_ip = shell_single_quote(src_ip),
            q_share = shell_single_quote(src_share),
            q_path = shell_single_quote(src_path),
            q_user = shell_single_quote(src_user),
            port = port,
            retention = self.retention,
            auto_permanent = self.auto_permanent,
            q_dest_root = shell_single_quote(&dest_root),
            q_cred = shell_single_quote(&cred),
            q_key = shell_single_quote(&key_file),
            dest = dest,
        )
    }

    /// Entrada de cron.d lista para instalar.
    pub fn render_cron(&self) -> String {
        format!(
            "SHELL=/bin/bash\nPATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\nMAILTO=\"\"\n{} root /usr/local/bin/backup_{}.sh\n",
            self.cron.trim(),
            self.id.trim()
        )
    }

    /// Archivo de credenciales 0600 (CIFS: clave=valor con soporte de dominio; SSH: clave literal sin evaluación shell).
    pub fn render_cred(&self, proto: Proto) -> String {
        match proto {
            Proto::Cifs => {
                let user = self.user.trim();
                let password = &self.password;
                if let Some((domain, account)) = split_domain_user(user) {
                    format!("domain={domain}\nusername={account}\npassword={password}\n")
                } else {
                    format!("username={user}\npassword={password}\n")
                }
            }
            Proto::Ssh => {
                if self.password.is_empty() {
                    String::new()
                } else {
                    format!("{}\n", self.password)
                }
            }
            Proto::Local => String::new(),
        }
    }

    /// Contenido de la clave privada SSH para guardar en `/etc/backup-credentials/<id>.id_ed25519` (0600 root:root).
    pub fn render_key(&self, proto: Proto) -> Option<String> {
        if proto != Proto::Ssh {
            return None;
        }
        let key = self.ssh_key.as_deref()?.trim();
        if key.starts_with("-----BEGIN") {
            let mut content = key.to_string();
            if !content.ends_with('\n') {
                content.push('\n');
            }
            Some(content)
        } else {
            None
        }
    }
}

fn split_domain_user(user: &str) -> Option<(&str, &str)> {
    user.split_once('\\')
        .or_else(|| user.split_once('/'))
        .filter(|(domain, account)| !domain.is_empty() && !account.is_empty())
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
