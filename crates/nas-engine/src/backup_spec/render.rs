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
                "mkdir -p /root/.ssh\nchmod 0700 /root/.ssh\nRSYNC_RC=0\nif [ -n \"$KEY_FILE\" ] && [ -f \"$KEY_FILE\" ]; then\n    rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -e \"ssh -i $KEY_FILE -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}\nelif [ -f \"/etc/backup-credentials/$TASK.id_ed25519\" ]; then\n    rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -e \"ssh -i /etc/backup-credentials/$TASK.id_ed25519 -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}\nelse\n    SSHPASS=\"\"\n    if [ -f \"$CRED\" ]; then\n        IFS= read -r SSHPASS < \"$CRED\" || true\n    fi\n    export SSHPASS\n    sshpass -e rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -e \"ssh -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}\n    unset SSHPASS\nfi".to_string()
            }
            _ => {
                "RSYNC_RC=0\nrsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}".to_string()
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
LOG_DIR='/srv/nas/LOGS_BACKUP'
CRED_FILE={q_cred}
KEY_FILE={q_key}
MODE="$SOURCE_TYPE"
CRED="$CRED_FILE"
# Destino: {dest}
BKP_DIR="${{BKP_DIR:-$DEST_ROOT/$TASK}}"
PERM_DIR="${{DEST_ROOT/BACKUPS_HISTORICOS/BACKUPS_PERMANENTES}}/$TASK"
LOG="${{LOG_FILE:-$LOG_DIR/backup_${{TASK}}.log}}"
MOUNT="${{MOUNT_ROOT:-/mnt/backup_sources}}/$TASK"
PROG_FILE="/run/nas-backup-$TASK.progress"
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
START_TIME=$(date +%s)
STAGE="$BKP_DIR/.inprogress_$DATE_STR"
FINAL="$BKP_DIR/snapshot_$DATE_STR"
SNAPSHOT_OK=false

log() {{ printf '[%s] [%s] %s\n' "$(date '+%F %T')" "$TASK" "$*" >> "$LOG"; }}
cleanup() {{
    local status=$?
    rm -f "$PROG_FILE" "${{PROG_FILE}}.tmp."* 2>/dev/null || true
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

read -r AVAIL PCT < <(df -Pk "$BKP_DIR" | awk 'NR==2 {{gsub("%","",$5); print $4, $5}}')
if [ "${{AVAIL:-0}}" -lt 2097152 ] || [ "${{PCT:-0}}" -gt 95 ]; then
    log "=== ABORTADO: espacio libre insuficiente en $BKP_DIR ($AVAIL KB, $PCT%) ==="
    exit 1
fi
if [ "${{PCT:-0}}" -gt 85 ]; then
    log "alerta preventiva: ocupación del volumen en $PCT%"
fi

mapfile -t PREV_SNAPS < <(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' | LC_ALL=C sort)
LINK_ARGS=()
if [ "${{#PREV_SNAPS[@]}}" -gt 0 ]; then
    LAST="${{PREV_SNAPS[$((${{#PREV_SNAPS[@]}} - 1))]}}"
    LINK_ARGS=("--link-dest=$LAST")
fi

mkdir -p "$STAGE"
{source_setup}

log "inicio ($MODE): $SRC"
printf '{{"percent":0,"speed":"0.00kB/s","elapsed":"00:00:00","remaining":"--:--:--","bytes":"0"}}\n' > "$PROG_FILE" 2>/dev/null || true
chmod 0644 "$PROG_FILE" 2>/dev/null || true
{rsync}
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
chmod -R a-w "$FINAL" 2>/dev/null || true

if [ "$AUTO_PERMANENT" = "true" ]; then
    HAS_PERM=false
    if [ -d "$PERM_DIR" ] && find "$PERM_DIR" -mindepth 1 -maxdepth 1 -type d -name "FULL_*" 2>/dev/null | grep -q .; then
        HAS_PERM=true
    fi

    if [ "$HAS_PERM" != "true" ]; then
        PERM_DEST="$PERM_DIR/FULL_${{DATE_STR}}_INICIAL"
        if cp -al "$FINAL" "$PERM_DEST" 2>/dev/null; then
            printf '{{"task":"%s","name":"%s","path":"%s","date":"%s","tag":"INICIAL","size_bytes":0,"files_count":0}}\n' \
                "$TASK" "FULL_${{DATE_STR}}_INICIAL" "$PERM_DEST" "$DATE_STR" > "$PERM_DEST/.freeze_meta.json" 2>/dev/null || true
            chmod -R a-w "$PERM_DEST" 2>/dev/null || true
            chattr +i "$PERM_DEST" 2>/dev/null || true
            log "congelación inicial automática: FULL_${{DATE_STR}}_INICIAL"
        fi
    else
        DAY_OF_MONTH=$(date +%d)
        if [ "$DAY_OF_MONTH" = "01" ]; then
            MONTH_TAG=$(date +%Y-%m)
            if ! find "$PERM_DIR" -maxdepth 1 -type d -name "FULL_${{MONTH_TAG}}*" 2>/dev/null | grep -q .; then
                PERM_DEST="$PERM_DIR/FULL_${{DATE_STR}}_MENSUAL"
                if cp -al "$FINAL" "$PERM_DEST" 2>/dev/null; then
                    printf '{{"task":"%s","name":"%s","path":"%s","date":"%s","tag":"MENSUAL","size_bytes":0,"files_count":0}}\n' \
                        "$TASK" "FULL_${{DATE_STR}}_MENSUAL" "$PERM_DEST" "$DATE_STR" > "$PERM_DEST/.freeze_meta.json" 2>/dev/null || true
                    chmod -R a-w "$PERM_DEST" 2>/dev/null || true
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
        chmod -R u+w "$OLD" 2>/dev/null || true
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
