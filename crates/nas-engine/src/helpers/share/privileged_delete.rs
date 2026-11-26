//! Eliminación privilegiada de archivos o carpetas dentro de recursos compartidos.
//!
//! Autentica credenciales reales del sistema operativo (`root` o usuario con `sudo`)
//! vía `stdin` sin almacenar contraseñas y sin modificar permisos, ACLs ni atributos
//! extendidos (`chmod`/`chown`/`setfacl`/`chattr`).

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner, SecretInput};
use serde::{Deserialize, Serialize};

use super::is_valid_unit_name;

/// Acción solicitada al helper de eliminación/papelera privilegiada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegedFileAction {
    /// Mueve el elemento validado a `.trash` mediante `rename` O(1).
    Trash,
    /// Elimina definitivamente el elemento del disco.
    #[default]
    Delete,
    /// Elimina definitivamente una entrada específica de `.trash`.
    PurgeTrash,
    /// Restaura una entrada de `.trash` a su `original_path`.
    RestoreTrash,
    /// Vacía definitivamente todas las entradas de `.trash`.
    EmptyTrash,
}

/// Solicitud de eliminación o gestión privilegiada enviada por `stdin` a `nas helper files-delete`.
#[derive(Deserialize)]
pub struct PrivilegedDeleteRequest {
    /// Raíz lógica (`nas`, `backups` o `unidades/<id>`).
    #[serde(default)]
    pub root: Option<String>,
    /// Acción a ejecutar (`trash`, `delete`, `purge_trash`, `restore_trash`, `empty_trash`).
    #[serde(default)]
    pub action: PrivilegedFileAction,
    /// Ruta relativa dentro de la raíz seleccionada (para `trash` y `delete`).
    #[serde(default)]
    pub path: String,
    /// Identificador de entrada dentro de `.trash` (para `purge_trash` y `restore_trash`).
    #[serde(default)]
    pub trash_id: String,
    /// Usuario del sistema operativo (`root` o usuario con `sudo`).
    pub auth_user: String,
    /// Contraseña del usuario del sistema operativo.
    pub auth_password: String,
    /// Usuario autenticado del panel web (para auditoría).
    #[serde(default)]
    pub web_user: String,
    /// IP de origen de la petición web (para auditoría).
    #[serde(default)]
    pub origin_ip: String,
}

impl fmt::Debug for PrivilegedDeleteRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivilegedDeleteRequest")
            .field("root", &self.root)
            .field("action", &self.action)
            .field("path", &self.path)
            .field("trash_id", &self.trash_id)
            .field("auth_user", &self.auth_user)
            .field("auth_password", &"***")
            .field("web_user", &self.web_user)
            .field("origin_ip", &self.origin_ip)
            .finish()
    }
}

/// Valida un nombre de usuario POSIX (`^[a-z_][a-z0-9_-]{0,31}$`).
pub fn is_valid_os_username(user: &str) -> bool {
    let bytes = user.as_bytes();
    if bytes.is_empty() || bytes.len() > 32 {
        return false;
    }
    let first = bytes[0];
    if !(first.is_ascii_lowercase() || first == b'_') {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Extrae `(hash, expire_days)` de `/etc/shadow` para `username`.
fn parse_shadow_entry(shadow_text: &str, username: &str) -> Option<(String, Option<i64>)> {
    for line in shadow_text.lines() {
        let mut fields = line.split(':');
        let Some(name) = fields.next() else {
            continue;
        };
        if name != username {
            continue;
        }
        let hash = fields.next()?.trim().to_string();
        let _lastchg = fields.next();
        let _min = fields.next();
        let _max = fields.next();
        let _warn = fields.next();
        let _inactive = fields.next();
        let expire = fields
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse::<i64>().ok());
        return Some((hash, expire));
    }
    None
}

fn verify_shadow_password(
    runner: &Runner,
    shadow_hash: &str,
    expire_days: Option<i64>,
    password: &str,
) -> NasResult<()> {
    if shadow_hash.is_empty() || shadow_hash.starts_with('!') || shadow_hash.starts_with('*') {
        return Err(NasError::Denied(
            "la cuenta del sistema está bloqueada o sin contraseña".into(),
        ));
    }
    if let Some(exp) = expire_days {
        let now_days = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| (d.as_secs() / 86_400) as i64)
            .unwrap_or(0);
        if exp <= now_days {
            return Err(NasError::Denied("la cuenta del sistema ha expirado".into()));
        }
    }

    let perl_script = concat!(
        "chomp(my $h = <STDIN>); chomp(my $p = <STDIN>); ",
        "exit(1) unless defined($h) && defined($p) && length($h) > 2; ",
        "my $c = crypt($p, $h); ",
        "exit(1) unless defined($c) && length($c) == length($h); ",
        "my $d = 0; ",
        "for (my $i = 0; $i < length($h); $i++) { $d |= ord(substr($c,$i,1)) ^ ord(substr($h,$i,1)); } ",
        "exit($d == 0 ? 0 : 1);"
    );
    let safe_runner = runner
        .clone()
        .with_secret(password)
        .with_secret(shadow_hash);
    let out = safe_runner.run(
        &CommandSpec::new("/usr/bin/perl")
            .args(["-e", perl_script])
            .stdin_secret(SecretInput::new(format!("{shadow_hash}\n{password}")))
            .timeout(Duration::from_secs(10)),
    )?;
    if !out.is_success() {
        return Err(NasError::Denied(
            "credenciales del sistema operativo inválidas".into(),
        ));
    }
    Ok(())
}

/// Comprueba si la salida de `sudo -l -U <user>` otorga ejecución `ALL` como `root`/`ALL`.
pub fn sudo_list_grants_root_all(stdout: &str) -> bool {
    let mut in_commands = false;
    for raw in stdout.lines() {
        let line = raw.trim();
        if line.contains("may run the following commands")
            || line.contains("puede ejecutar los siguientes comandos")
        {
            in_commands = true;
            continue;
        }
        if !in_commands || line.is_empty() {
            continue;
        }
        if let Some(open) = line.find('(') {
            if let Some(close_rel) = line[open + 1..].find(')') {
                let target = &line[open + 1..open + 1 + close_rel];
                let after = line[open + 1 + close_rel + 1..].trim();
                let runas_user = target.split(':').next().unwrap_or("").trim();
                let target_ok = runas_user
                    .split(',')
                    .any(|u| matches!(u.trim(), "ALL" | "root"));
                let mut cmd_clean = after;
                loop {
                    let trimmed = cmd_clean.trim_start();
                    if let Some(rest) = trimmed
                        .strip_prefix("NOPASSWD:")
                        .or_else(|| trimmed.strip_prefix("PASSWD:"))
                        .or_else(|| trimmed.strip_prefix("SETENV:"))
                        .or_else(|| trimmed.strip_prefix("NOSETENV:"))
                        .or_else(|| trimmed.strip_prefix("EXEC:"))
                        .or_else(|| trimmed.strip_prefix("NOEXEC:"))
                    {
                        cmd_clean = rest;
                    } else {
                        cmd_clean = trimmed;
                        break;
                    }
                }
                if target_ok && cmd_clean.trim() == "ALL" {
                    return true;
                }
            }
        }
    }
    false
}

fn verify_sudo_privilege(runner: &Runner, username: &str) -> NasResult<()> {
    let out = runner.run(
        &CommandSpec::new("/usr/bin/sudo")
            .args(["-l", "-U", username])
            .timeout(Duration::from_secs(10)),
    )?;
    if !out.is_success() || !sudo_list_grants_root_all(&out.stdout) {
        return Err(NasError::Denied(format!(
            "el usuario '{username}' no tiene privilegios completos de sudo"
        )));
    }
    Ok(())
}

/// Verifica contraseña y privilegios (`root` o usuario con `sudo` completo).
pub fn verify_privileged_os_credentials(
    runner: &Runner,
    user: &str,
    password: &str,
) -> NasResult<()> {
    let user = user.trim();
    if !is_valid_os_username(user) {
        return Err(NasError::Denied("usuario del sistema inválido".into()));
    }
    if password.is_empty()
        || password.contains('\0')
        || password.contains('\n')
        || password.contains('\r')
    {
        return Err(NasError::Denied("contraseña inválida".into()));
    }

    let shadow_entry = std::fs::read_to_string("/etc/shadow")
        .ok()
        .and_then(|txt| parse_shadow_entry(&txt, user));

    if let Some((hash, expire)) = shadow_entry {
        verify_shadow_password(runner, &hash, expire, password)?;
    } else if user == "root" {
        return Err(NasError::Denied(
            "credenciales del sistema operativo inválidas".into(),
        ));
    } else {
        // Cuentas externas (AD/SSSD) que no residen en /etc/shadow
        let safe_runner = runner.clone().with_secret(password);
        let out = safe_runner.run(
            &CommandSpec::new("/usr/sbin/runuser")
                .args([
                    "-u",
                    user,
                    "--",
                    "/usr/bin/sudo",
                    "-k",
                    "-S",
                    "-p",
                    "",
                    "-u",
                    "root",
                    "/usr/bin/true",
                ])
                .stdin_secret(SecretInput::new(password.to_string()))
                .timeout(Duration::from_secs(10)),
        )?;
        if !out.is_success() {
            return Err(NasError::Denied(
                "credenciales del sistema operativo inválidas".into(),
            ));
        }
    }

    if user != "root" {
        verify_sudo_privilege(runner, user)?;
    }
    Ok(())
}

fn resolve_root_base_and_rel(
    data_root: &Path,
    root_name: Option<&str>,
    rel_path: &str,
) -> NasResult<(PathBuf, String)> {
    let raw = root_name.unwrap_or("").trim();
    let trimmed_rel = rel_path.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("nas") || raw.eq_ignore_ascii_case("principal") {
        return Ok((data_root.to_path_buf(), trimmed_rel.to_string()));
    }
    if raw.eq_ignore_ascii_case("backups") {
        if trimmed_rel.is_empty() {
            return Err(NasError::Denied(
                "no se puede eliminar la carpeta raíz BACKUPS_HISTORICOS".into(),
            ));
        }
        return Ok((
            data_root.to_path_buf(),
            format!("BACKUPS_HISTORICOS/{trimmed_rel}"),
        ));
    }
    let unit_id = raw.strip_prefix("unidades/").unwrap_or(raw);
    if !is_valid_unit_name(unit_id) {
        return Err(NasError::Denied("unidad de almacenamiento inválida".into()));
    }
    let unit_path = data_root.join("unidades").join(unit_id);
    let meta = std::fs::symlink_metadata(&unit_path).map_err(|_| {
        NasError::NotFound(format!("unidad no encontrada: {}", unit_path.display()))
    })?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(NasError::Denied(
            "la unidad no es un directorio válido".into(),
        ));
    }
    Ok((unit_path, trimmed_rel.to_string()))
}

fn is_privileged_target_blocked(rel_from_data_root: &Path) -> bool {
    let mut comps: Vec<String> = Vec::new();
    for comp in rel_from_data_root.components() {
        match comp {
            Component::Normal(s) => comps.push(s.to_string_lossy().to_string()),
            _ => return true,
        }
    }
    if comps.is_empty() {
        return true;
    }
    let sub: &[String] = if comps[0] == "unidades" {
        if comps.len() < 3 {
            return true;
        }
        &comps[2..]
    } else {
        &comps[..]
    };
    let Some(first) = sub.first() else {
        return true;
    };
    if matches!(
        first.as_str(),
        "LOGS_BACKUP" | ".trash" | ".nas_staging" | ".tmp_zip" | "unidades"
    ) {
        return true;
    }
    if matches!(first.as_str(), "BACKUPS_HISTORICOS" | "BACKUPS_PERMANENTES") {
        return sub.len() < 2;
    }
    false
}

fn validate_relative_input(rel_trimmed: &str) -> NasResult<()> {
    if rel_trimmed.is_empty()
        || rel_trimmed.contains('\0')
        || rel_trimmed.chars().any(char::is_control)
        || rel_trimmed.contains('\\')
        || rel_trimmed.contains("//")
        || rel_trimmed.starts_with('/')
        || rel_trimmed.split('/').any(|seg| seg == ".")
    {
        return Err(NasError::Denied("ruta de eliminación inválida".into()));
    }
    for comp in Path::new(rel_trimmed).components() {
        if !matches!(comp, Component::Normal(_)) {
            return Err(NasError::Denied(
                "la ruta contiene segmentos relativos no permitidos".into(),
            ));
        }
    }
    Ok(())
}

fn resolve_privileged_target_with_base(
    data_root: &Path,
    root_name: Option<&str>,
    rel_path: &str,
) -> NasResult<(PathBuf, PathBuf, String)> {
    validate_relative_input(rel_path.trim())?;
    let (root_base, effective_rel) = resolve_root_base_and_rel(data_root, root_name, rel_path)?;
    validate_relative_input(&effective_rel)?;

    let candidate = root_base.join(&effective_rel);
    if candidate == data_root || candidate == root_base {
        return Err(NasError::Denied(
            "no se puede eliminar el directorio raíz".into(),
        ));
    }

    let rel_from_data_root = candidate
        .strip_prefix(data_root)
        .map_err(|_| NasError::Denied("ruta fuera de la raíz NAS".into()))?;
    if is_privileged_target_blocked(rel_from_data_root) {
        return Err(NasError::Denied(
            "operación prohibida: las carpetas raíz de respaldo, LOGS_BACKUP, .trash y áreas temporales están protegidas".into(),
        ));
    }

    let parent = candidate
        .parent()
        .ok_or_else(|| NasError::Denied("directorio padre inválido".into()))?;
    let file_name = candidate
        .file_name()
        .ok_or_else(|| NasError::Denied("nombre de elemento inválido".into()))?;

    let rel_parent = parent
        .strip_prefix(data_root)
        .map_err(|_| NasError::Denied("ruta padre fuera de la raíz NAS".into()))?;
    let mut cur = data_root.to_path_buf();
    for comp in rel_parent.components() {
        let Component::Normal(part) = comp else {
            return Err(NasError::Denied("ruta padre inválida".into()));
        };
        cur.push(part);
        let meta = std::fs::symlink_metadata(&cur)
            .map_err(|_| NasError::NotFound(format!("ruta no encontrada: {}", cur.display())))?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(NasError::Denied(format!(
                "enlace simbólico o tipo no válido en ruta padre: {}",
                cur.display()
            )));
        }
    }

    let canon_data = std::fs::canonicalize(data_root).map_err(|e| NasError::io(data_root, e))?;
    let canon_base = std::fs::canonicalize(&root_base).map_err(|e| NasError::io(&root_base, e))?;
    let canon_parent = std::fs::canonicalize(parent).map_err(|e| NasError::io(parent, e))?;
    if !canon_parent.starts_with(&canon_base) || !canon_parent.starts_with(&canon_data) {
        return Err(NasError::Denied(
            "la ruta escapa de la raíz permitida".into(),
        ));
    }
    let final_target = canon_parent.join(file_name);
    let canon_rel_data = final_target
        .strip_prefix(&canon_data)
        .map_err(|_| NasError::Denied("la ruta escapa de la raíz NAS".into()))?;
    if is_privileged_target_blocked(canon_rel_data) {
        return Err(NasError::Denied(
            "operación prohibida: ruta canónica protegida".into(),
        ));
    }
    let canon_rel_base = final_target
        .strip_prefix(&canon_base)
        .map_err(|_| NasError::Denied("la ruta escapa de la raíz permitida".into()))?;
    let rel_base_str = canon_rel_base.to_string_lossy().replace('\\', "/");
    Ok((canon_base, final_target, rel_base_str))
}

#[cfg(test)]
fn resolve_privileged_delete_target(
    data_root: &Path,
    root_name: Option<&str>,
    rel_path: &str,
) -> NasResult<PathBuf> {
    let (_, target, _) = resolve_privileged_target_with_base(data_root, root_name, rel_path)?;
    Ok(target)
}

fn has_immutable_flag(runner: &Runner, path: &Path) -> bool {
    let Ok(out) = runner.run(
        &CommandSpec::new("lsattr")
            .args(["-d", "--", &path.to_string_lossy()])
            .timeout(Duration::from_secs(5)),
    ) else {
        return false;
    };
    if !out.is_success() {
        return false;
    }
    out.stdout
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .is_some_and(|attrs| attrs.contains('i'))
}

fn set_immutable_flag(runner: &Runner, path: &Path, immutable: bool) -> bool {
    let flag = if immutable { "+i" } else { "-i" };
    runner
        .run(
            &CommandSpec::new("chattr")
                .args([flag, "--", &path.to_string_lossy()])
                .timeout(Duration::from_secs(5)),
        )
        .is_ok_and(|o| o.is_success())
}

struct ImmutableRestoreGuard<'a> {
    runner: &'a Runner,
    paths: Vec<PathBuf>,
}

impl<'a> ImmutableRestoreGuard<'a> {
    fn unlock(runner: &'a Runner, candidates: &[&Path]) -> Self {
        let mut paths = Vec::new();
        for &p in candidates {
            if p.exists() && has_immutable_flag(runner, p) && set_immutable_flag(runner, p, false) {
                paths.push(p.to_path_buf());
            }
        }
        Self { runner, paths }
    }
}

impl Drop for ImmutableRestoreGuard<'_> {
    fn drop(&mut self) {
        for path in self.paths.iter().rev() {
            if path.exists() {
                let _ = set_immutable_flag(self.runner, path, true);
            }
        }
    }
}

fn is_btrfs_subvolume(runner: &Runner, path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    runner
        .run(
            &CommandSpec::new("btrfs")
                .args(["subvolume", "show", &path.to_string_lossy()])
                .timeout(Duration::from_secs(5)),
        )
        .is_ok_and(|o| o.is_success())
}

fn is_btrfs_ro(runner: &Runner, path: &Path) -> bool {
    let Ok(out) = runner.run(
        &CommandSpec::new("btrfs")
            .args(["property", "get", "-ts", &path.to_string_lossy(), "ro"])
            .timeout(Duration::from_secs(5)),
    ) else {
        return false;
    };
    out.is_success() && out.stdout.contains("ro=true")
}

fn set_btrfs_ro(runner: &Runner, path: &Path, ro: bool) -> bool {
    let val = if ro { "true" } else { "false" };
    runner
        .run(
            &CommandSpec::new("btrfs")
                .args(["property", "set", "-ts", &path.to_string_lossy(), "ro", val])
                .timeout(Duration::from_secs(5)),
        )
        .is_ok_and(|o| o.is_success())
}

struct BtrfsRoRestoreGuard<'a> {
    runner: &'a Runner,
    subvolumes: Vec<PathBuf>,
}

impl<'a> BtrfsRoRestoreGuard<'a> {
    fn unlock_ancestors(runner: &'a Runner, stop_at: &Path, start: &Path) -> Self {
        let mut subvolumes = Vec::new();
        let mut cur = Some(start);
        let mut depth = 0usize;
        while let Some(dir) = cur {
            if depth >= 16 || dir == stop_at || !dir.starts_with(stop_at) {
                break;
            }
            if is_btrfs_subvolume(runner, dir)
                && is_btrfs_ro(runner, dir)
                && set_btrfs_ro(runner, dir, false)
            {
                subvolumes.push(dir.to_path_buf());
            }
            cur = dir.parent();
            depth += 1;
        }
        Self { runner, subvolumes }
    }
}

impl Drop for BtrfsRoRestoreGuard<'_> {
    fn drop(&mut self) {
        for subvol in self.subvolumes.iter().rev() {
            if subvol.exists() {
                let _ = set_btrfs_ro(self.runner, subvol, true);
            }
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PrivTrashMetaRecord {
    #[serde(default)]
    original_path: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    deleted_by: String,
    #[serde(default)]
    deleted_at: String,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    privileged: bool,
    #[serde(default)]
    was_immutable: bool,
}

fn next_unique_trash_ms(trash: &Path) -> i64 {
    let mut candidate = chrono::Utc::now().timestamp_millis().max(1);
    let existing: Vec<String> = std::fs::read_dir(trash)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    while existing
        .iter()
        .any(|name| name.starts_with(&format!("{candidate}_")))
    {
        candidate += 1;
    }
    candidate
}

fn split_trash_name(id: &str) -> (&str, &str) {
    match id.split_once('_') {
        Some((stamp, name)) if !name.is_empty() && stamp.bytes().all(|b| b.is_ascii_digit()) => {
            (stamp, name)
        }
        _ => ("", id),
    }
}

fn ensure_real_trash_dir(trash_base: &Path, create_if_missing: bool) -> NasResult<PathBuf> {
    let canon_base = std::fs::canonicalize(trash_base).map_err(|e| NasError::io(trash_base, e))?;
    let trash = canon_base.join(".trash");
    match std::fs::symlink_metadata(&trash) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(NasError::Denied(
                    "la papelera (.trash) es un enlace simbólico o tipo inválido".into(),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if create_if_missing {
                std::fs::create_dir(&trash).map_err(|err| NasError::io(&trash, err))?;
            } else {
                return Err(NasError::NotFound(format!(
                    "no existe la papelera en {}",
                    canon_base.display()
                )));
            }
        }
        Err(e) => return Err(NasError::io(&trash, e)),
    }
    let canon_trash = std::fs::canonicalize(&trash).map_err(|e| NasError::io(&trash, e))?;
    if canon_trash != canon_base.join(".trash") {
        return Err(NasError::Denied(
            "la papelera (.trash) escapa de su raíz permitida".into(),
        ));
    }
    Ok(canon_trash)
}

fn ensure_real_trash_meta_dir(trash_base: &Path, create_if_missing: bool) -> NasResult<PathBuf> {
    let canon_trash = ensure_real_trash_dir(trash_base, create_if_missing)?;
    let meta_dir = canon_trash.join(".meta");
    match std::fs::symlink_metadata(&meta_dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(NasError::Denied(
                    "el directorio de metadatos (.trash/.meta) es un enlace simbólico o tipo inválido"
                        .into(),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if create_if_missing {
                std::fs::create_dir(&meta_dir).map_err(|err| NasError::io(&meta_dir, err))?;
            } else {
                return Err(NasError::NotFound(format!(
                    "no existe .trash/.meta en {}",
                    canon_trash.display()
                )));
            }
        }
        Err(e) => return Err(NasError::io(&meta_dir, e)),
    }
    let canon_meta = std::fs::canonicalize(&meta_dir).map_err(|e| NasError::io(&meta_dir, e))?;
    if canon_meta != canon_trash.join(".meta") {
        return Err(NasError::Denied(
            "el directorio .trash/.meta escapa de la papelera confinada".into(),
        ));
    }
    Ok(canon_meta)
}

fn write_privileged_trash_meta(
    trash_base: &Path,
    id: &str,
    record: &PrivTrashMetaRecord,
) -> NasResult<()> {
    use std::io::Write;
    let meta_dir = ensure_real_trash_meta_dir(trash_base, true)?;
    let json = serde_json::to_string(record)
        .map_err(|e| NasError::Invalid(format!("metadatos de papelera: {e}")))?;
    let meta_path = meta_dir.join(id);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&meta_path)
        .map_err(|e| NasError::io(&meta_path, e))?;
    file.write_all(json.as_bytes())
        .map_err(|e| NasError::io(&meta_path, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&meta_path, std::fs::Permissions::from_mode(0o644));
    }
    Ok(())
}

fn trash_validated_target(
    runner: &Runner,
    trash_base: &Path,
    target: &Path,
    rel_from_base: &str,
    actor: &str,
) -> NasResult<String> {
    let meta = std::fs::symlink_metadata(target)
        .map_err(|_| NasError::NotFound(format!("elemento no encontrado: {}", target.display())))?;
    let is_symlink = meta.file_type().is_symlink();

    let trash_dir = ensure_real_trash_dir(trash_base, true)?;

    let stamp_ms = next_unique_trash_ms(&trash_dir);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "elemento".to_string());
    let id = format!("{stamp_ms}_{name}");
    let trash_dest = trash_dir.join(&id);
    let deleted_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let parent = target
        .parent()
        .ok_or_else(|| NasError::Denied("directorio padre inválido".into()))?;
    let _ro_guard = BtrfsRoRestoreGuard::unlock_ancestors(runner, trash_base, parent);
    let _imm_parent_guard = ImmutableRestoreGuard::unlock(runner, &[parent]);

    let was_immutable = !is_symlink
        && target.exists()
        && has_immutable_flag(runner, target)
        && set_immutable_flag(runner, target, false);

    if let Err(e) = std::fs::rename(target, &trash_dest) {
        if was_immutable {
            let _ = set_immutable_flag(runner, target, true);
        }
        return Err(if e.raw_os_error() == Some(18) {
            NasError::Invalid(format!(
                "no se puede mover '{rel_from_base}' a la papelera entre diferentes sistemas de archivos"
            ))
        } else if e.raw_os_error() == Some(1) {
            NasError::Denied(
                "operación denegada (EPERM): el elemento o su directorio contenedor es inmutable o de solo lectura".into(),
            )
        } else {
            NasError::io(target, e)
        });
    }

    let record = PrivTrashMetaRecord {
        original_path: rel_from_base.to_string(),
        filename: name,
        deleted_by: if actor.trim().is_empty() {
            "admin".to_string()
        } else {
            actor.trim().to_string()
        },
        deleted_at,
        size_bytes: None,
        privileged: true,
        was_immutable,
    };
    if let Err(err) = write_privileged_trash_meta(trash_base, &id, &record) {
        let _ = std::fs::rename(&trash_dest, target);
        if was_immutable {
            let _ = set_immutable_flag(runner, target, true);
        }
        return Err(err);
    }

    Ok(format!(
        "Elemento movido a la papelera con privilegios: {rel_from_base}"
    ))
}

fn delete_validated_target_with_runner(
    runner: &Runner,
    trash_base: &Path,
    target: &Path,
) -> NasResult<String> {
    let meta = std::fs::symlink_metadata(target)
        .map_err(|_| NasError::NotFound(format!("elemento no encontrado: {}", target.display())))?;

    let parent = target.parent().unwrap_or(trash_base);
    let _ro_guard = BtrfsRoRestoreGuard::unlock_ancestors(runner, trash_base, parent);
    let _imm_parent_guard = ImmutableRestoreGuard::unlock(runner, &[parent]);

    let map_io = |e: std::io::Error| {
        if e.raw_os_error() == Some(1) {
            NasError::Denied(
                "operación denegada (EPERM): el elemento es inmutable (+i) o está en un volumen de solo lectura".into(),
            )
        } else {
            NasError::io(target, e)
        }
    };

    if meta.file_type().is_symlink() {
        std::fs::remove_file(target).map_err(map_io)?;
    } else if !meta.is_dir() {
        if has_immutable_flag(runner, target) {
            let _ = set_immutable_flag(runner, target, false);
        }
        std::fs::remove_file(target).map_err(map_io)?;
    } else if is_btrfs_subvolume(runner, target) {
        if is_btrfs_ro(runner, target) {
            let _ = set_btrfs_ro(runner, target, false);
        }
        let out = runner.run(
            &CommandSpec::new("btrfs")
                .args(["subvolume", "delete", &target.to_string_lossy()])
                .timeout(Duration::from_secs(30)),
        )?;
        if !out.is_success() {
            return Err(NasError::Denied(format!(
                "no se pudo eliminar el subvolumen Btrfs {}: {}",
                target.display(),
                out.stderr.trim()
            )));
        }
    } else {
        if has_immutable_flag(runner, target) {
            let _ = set_immutable_flag(runner, target, false);
        }
        std::fs::remove_dir_all(target).map_err(map_io)?;
    }

    Ok(format!(
        "Elemento eliminado con privilegios: {}",
        target.display()
    ))
}

#[cfg(test)]
fn delete_validated_target(target: &Path) -> NasResult<String> {
    let runner = Runner::new();
    let stop_at = target.parent().unwrap_or(target);
    delete_validated_target_with_runner(&runner, stop_at, target)
}

fn resolve_privileged_trash_entry(
    trash_base: &Path,
    trash_id: &str,
) -> NasResult<(String, PathBuf)> {
    let id = trash_id.trim();
    if id.is_empty()
        || id.starts_with('.')
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains('\0')
        || id.chars().any(char::is_control)
    {
        return Err(NasError::Denied(
            "identificador de papelera inválido".into(),
        ));
    }
    let canon_trash = match ensure_real_trash_dir(trash_base, false) {
        Ok(dir) => dir,
        Err(NasError::NotFound(_)) => {
            return Err(NasError::NotFound(format!("no existe en papelera: {id}")));
        }
        Err(e) => return Err(e),
    };
    let trash = canon_trash.as_path();

    let verify_under_trash = |name: String, candidate: PathBuf| -> NasResult<(String, PathBuf)> {
        let Some(parent) = candidate.parent() else {
            return Err(NasError::Denied("entrada de papelera inválida".into()));
        };
        let canon_parent = std::fs::canonicalize(parent).map_err(|e| NasError::io(parent, e))?;
        if canon_parent != canon_trash {
            return Err(NasError::Denied(
                "la entrada escapa de la papelera confinada".into(),
            ));
        }
        Ok((
            name,
            canon_parent.join(candidate.file_name().unwrap_or_default()),
        ))
    };

    let exact = trash.join(id);
    if std::fs::symlink_metadata(&exact).is_ok() {
        return verify_under_trash(id.to_string(), exact);
    }
    if id.bytes().all(|b| b.is_ascii_digit()) {
        let prefix = format!("{id}_");
        if let Ok(entries) = std::fs::read_dir(trash) {
            let mut matches: Vec<(String, PathBuf)> = entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    name.starts_with(&prefix).then(|| (name, entry.path()))
                })
                .collect();
            if matches.len() == 1 {
                let (name, path) = matches.remove(0);
                return verify_under_trash(name, path);
            }
        }
    }
    Err(NasError::NotFound(format!("no existe en papelera: {id}")))
}

fn read_priv_trash_meta(trash_base: &Path, id: &str) -> Option<PrivTrashMetaRecord> {
    let meta_dir = ensure_real_trash_meta_dir(trash_base, false).ok()?;
    let path = meta_dir.join(id);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('{') {
        if let Ok(rec) = serde_json::from_str::<PrivTrashMetaRecord>(trimmed) {
            return Some(rec);
        }
    }
    Some(PrivTrashMetaRecord {
        original_path: trimmed.to_string(),
        ..PrivTrashMetaRecord::default()
    })
}

fn remove_priv_trash_meta(trash_base: &Path, id: &str) {
    let Ok(meta_dir) = ensure_real_trash_meta_dir(trash_base, false) else {
        return;
    };
    let path = meta_dir.join(id);
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if !meta.file_type().is_symlink() && meta.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn purge_privileged_trash_entry(
    runner: &Runner,
    trash_base: &Path,
    trash_id: &str,
) -> NasResult<String> {
    let (real_id, path) = resolve_privileged_trash_entry(trash_base, trash_id)?;
    let _ = delete_validated_target_with_runner(runner, trash_base, &path)?;
    remove_priv_trash_meta(trash_base, &real_id);
    Ok(format!(
        "Elemento eliminado definitivamente de la papelera: {real_id}"
    ))
}

fn restore_privileged_trash_entry(
    runner: &Runner,
    data_root: &Path,
    trash_base: &Path,
    trash_id: &str,
) -> NasResult<String> {
    let (real_id, source) = resolve_privileged_trash_entry(trash_base, trash_id)?;
    let (_, fallback_name) = split_trash_name(&real_id);
    let meta = read_priv_trash_meta(trash_base, &real_id);

    let raw_name = meta
        .as_ref()
        .map(|m| m.filename.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_name);
    let mut name_comps = Path::new(raw_name).components();
    let name = match (name_comps.next(), name_comps.next()) {
        (Some(Component::Normal(c)), None)
            if !raw_name.trim().is_empty()
                && !raw_name.contains('/')
                && !raw_name.contains('\\') =>
        {
            c.to_string_lossy().to_string()
        }
        _ => {
            return Err(NasError::Denied(
                "nombre de archivo en metadatos de papelera inválido".into(),
            ));
        }
    };

    let relative = meta
        .as_ref()
        .map(|m| m.original_path.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| name.clone());
    validate_relative_input(&relative)?;

    let canon_data = std::fs::canonicalize(data_root).map_err(|e| NasError::io(data_root, e))?;
    let canon_base = std::fs::canonicalize(trash_base).map_err(|e| NasError::io(trash_base, e))?;
    let candidate = canon_base.join(&relative);
    let rel_from_data = candidate
        .strip_prefix(&canon_data)
        .map_err(|_| NasError::Denied("destino fuera de la raíz NAS".into()))?;
    if candidate == canon_base
        || candidate == canon_data
        || is_privileged_target_blocked(rel_from_data)
    {
        return Err(NasError::Denied(
            "operación prohibida: destino de restauración protegido".into(),
        ));
    }

    let parent = candidate
        .parent()
        .ok_or_else(|| NasError::Denied("directorio padre de destino inválido".into()))?;
    // Verificar sin symlinks en los ancestros existentes del destino.
    let mut cur = canon_base.clone();
    if let Ok(rel_parent) = parent.strip_prefix(&canon_base) {
        for comp in rel_parent.components() {
            let Component::Normal(part) = comp else {
                return Err(NasError::Denied("ruta padre de destino inválida".into()));
            };
            cur.push(part);
            if let Ok(m) = std::fs::symlink_metadata(&cur) {
                if m.file_type().is_symlink() || !m.is_dir() {
                    return Err(NasError::Denied(format!(
                        "enlace simbólico o tipo no válido en destino: {}",
                        cur.display()
                    )));
                }
            }
        }
    }
    if !parent.exists() {
        let mut existing = parent.to_path_buf();
        while !existing.exists() {
            let Some(p) = existing.parent() else {
                break;
            };
            existing = p.to_path_buf();
        }
        let _ro_guard = BtrfsRoRestoreGuard::unlock_ancestors(runner, &canon_base, &existing);
        let _imm_guard = ImmutableRestoreGuard::unlock(runner, &[&existing]);
        std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
    }

    let canon_parent = std::fs::canonicalize(parent).map_err(|e| NasError::io(parent, e))?;
    if !canon_parent.starts_with(&canon_base) || !canon_parent.starts_with(&canon_data) {
        return Err(NasError::Denied(
            "el destino de restauración escapa de la raíz permitida".into(),
        ));
    }

    let mut target = canon_parent.join(&name);
    let mut counter = 1u32;
    while std::fs::symlink_metadata(&target).is_ok() {
        if counter > 10_000 {
            return Err(NasError::Invalid(
                "demasiadas colisiones de nombre al restaurar desde la papelera".into(),
            ));
        }
        target = canon_parent.join(format!("{name}.{counter}"));
        counter += 1;
    }

    let final_rel_data = target
        .strip_prefix(&canon_data)
        .map_err(|_| NasError::Denied("destino fuera de la raíz NAS".into()))?;
    if is_privileged_target_blocked(final_rel_data) {
        return Err(NasError::Denied(
            "operación prohibida: destino final protegido".into(),
        ));
    }

    let _ro_guard = BtrfsRoRestoreGuard::unlock_ancestors(runner, &canon_base, &canon_parent);
    let _imm_guard = ImmutableRestoreGuard::unlock(runner, &[&canon_parent]);

    let source_is_symlink =
        std::fs::symlink_metadata(&source).is_ok_and(|m| m.file_type().is_symlink());

    std::fs::rename(&source, &target).map_err(|e| {
        if e.raw_os_error() == Some(1) {
            NasError::Denied(
                "operación denegada (EPERM): el destino o su directorio contenedor es inmutable o de solo lectura".into(),
            )
        } else {
            NasError::io(&target, e)
        }
    })?;

    if !source_is_symlink && meta.as_ref().is_some_and(|m| m.was_immutable) {
        let _ = set_immutable_flag(runner, &target, true);
    }
    remove_priv_trash_meta(trash_base, &real_id);

    let restored_rel = target
        .strip_prefix(&canon_base)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or(relative);
    Ok(format!(
        "Elemento restaurado con privilegios: {restored_rel}"
    ))
}

fn empty_privileged_trash(runner: &Runner, trash_base: &Path) -> NasResult<String> {
    let trash = match ensure_real_trash_dir(trash_base, false) {
        Ok(dir) => dir,
        Err(NasError::NotFound(_)) => {
            return Ok("La papelera ya está vacía (0 elementos eliminados)".into());
        }
        Err(e) => return Err(e),
    };
    let mut removed = 0u32;
    let mut errors = 0u32;
    for entry in std::fs::read_dir(&trash).map_err(|e| NasError::io(&trash, e))? {
        let entry = entry.map_err(|e| NasError::io(&trash, e))?;
        let id = entry.file_name().to_string_lossy().to_string();
        if id.starts_with('.') {
            continue;
        }
        if purge_privileged_trash_entry(runner, trash_base, &id).is_ok() {
            removed += 1;
        } else {
            errors += 1;
        }
    }
    if errors > 0 && removed == 0 {
        return Err(NasError::Denied(
            "no se pudieron eliminar los elementos restantes de la papelera".into(),
        ));
    }
    Ok(format!(
        "Papelera vaciada con privilegios ({removed} elementos eliminados)"
    ))
}

/// Autentica al usuario OS (`root` o `sudo`) y ejecuta la acción solicitada sobre archivos o papelera.
pub fn privileged_delete_file(
    runner: &Runner,
    data_root: &Path,
    req: &PrivilegedDeleteRequest,
) -> NasResult<String> {
    verify_privileged_os_credentials(runner, &req.auth_user, &req.auth_password)?;

    match req.action {
        PrivilegedFileAction::Trash => {
            let (trash_base, target, rel_from_base) =
                resolve_privileged_target_with_base(data_root, req.root.as_deref(), &req.path)?;
            let actor = if req.web_user.trim().is_empty() {
                req.auth_user.trim()
            } else {
                req.web_user.trim()
            };
            trash_validated_target(runner, &trash_base, &target, &rel_from_base, actor)
        }
        PrivilegedFileAction::Delete => {
            let (trash_base, target, _) =
                resolve_privileged_target_with_base(data_root, req.root.as_deref(), &req.path)?;
            delete_validated_target_with_runner(runner, &trash_base, &target)
        }
        PrivilegedFileAction::PurgeTrash => {
            let (trash_base, _) = resolve_root_base_and_rel(data_root, req.root.as_deref(), "x")?;
            let canon_base =
                std::fs::canonicalize(&trash_base).map_err(|e| NasError::io(&trash_base, e))?;
            purge_privileged_trash_entry(runner, &canon_base, &req.trash_id)
        }
        PrivilegedFileAction::RestoreTrash => {
            let (trash_base, _) = resolve_root_base_and_rel(data_root, req.root.as_deref(), "x")?;
            let canon_base =
                std::fs::canonicalize(&trash_base).map_err(|e| NasError::io(&trash_base, e))?;
            restore_privileged_trash_entry(runner, data_root, &canon_base, &req.trash_id)
        }
        PrivilegedFileAction::EmptyTrash => {
            let (trash_base, _) = resolve_root_base_and_rel(data_root, req.root.as_deref(), "x")?;
            let canon_base =
                std::fs::canonicalize(&trash_base).map_err(|e| NasError::io(&trash_base, e))?;
            empty_privileged_trash(runner, &canon_base)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn parse_shadow_entry_y_validacion_de_usuario() {
        assert!(is_valid_os_username("root"));
        assert!(is_valid_os_username("admin_01"));
        assert!(!is_valid_os_username(""));
        assert!(!is_valid_os_username("Root"));
        assert!(!is_valid_os_username("../root"));

        let shadow = "root:$y$j9T$abc$def:19000:0:99999:7:::\nnobody:*:19000:0:99999:7:::\nexp:$y$x:19000:0:99999:7::10:\n";
        assert_eq!(
            parse_shadow_entry(shadow, "root"),
            Some(("$y$j9T$abc$def".to_string(), None))
        );
        assert_eq!(
            parse_shadow_entry(shadow, "nobody"),
            Some(("*".to_string(), None))
        );
        assert_eq!(
            parse_shadow_entry(shadow, "exp"),
            Some(("$y$x".to_string(), Some(10)))
        );
    }

    #[test]
    fn sudo_list_grants_root_all_distingue_privilegios_completos() {
        let full = "User sadmin may run the following commands on host:\n    (ALL : ALL) ALL\n";
        assert!(sudo_list_grants_root_all(full));

        let nopass = "User op may run the following commands:\n    (ALL) NOPASSWD: ALL\n";
        assert!(sudo_list_grants_root_all(nopass));

        let restricted = "User nas-web may run the following commands:\n    (root) NOPASSWD: /usr/bin/smbstatus -b\n";
        assert!(!sudo_list_grants_root_all(restricted));
        assert!(!sudo_list_grants_root_all(
            "User foo is not allowed to run sudo"
        ));
        assert!(!sudo_list_grants_root_all(
            "User foo may run the following commands:\n    (nobody : ALL) ALL\n"
        ));
        assert!(!sudo_list_grants_root_all(
            "User foo may run the following commands:\n    (root) ALL, !/usr/bin/su\n"
        ));
        assert!(sudo_list_grants_root_all(
            "User foo may run the following commands:\n    (root : grp) SETENV: NOPASSWD: ALL\n"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_y_delete_preserva_permisos_padre_y_bloquea_protegidos() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("nas-priv-del-{}-{nanos}", std::process::id()));
        let share = root.join("SISTEMAS");
        let nested = share.join("subcarpeta");
        let outside =
            std::env::temp_dir().join(format!("nas-priv-out-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join("BACKUPS_HISTORICOS/t1")).unwrap();
        std::fs::create_dir_all(root.join("BACKUPS_PERMANENTES/t1")).unwrap();
        std::fs::create_dir_all(root.join("unidades/u1/BACKUPS_HISTORICOS/t1")).unwrap();
        std::fs::create_dir_all(root.join("unidades/u1/COMPARTIDA")).unwrap();
        std::fs::write(share.join("copiado_error.txt"), b"dato").unwrap();
        std::fs::write(nested.join("archivo.bin"), b"123").unwrap();
        std::fs::write(root.join("BACKUPS_HISTORICOS/t1/err.txt"), b"err").unwrap();
        std::fs::write(root.join("BACKUPS_PERMANENTES/t1/err.txt"), b"err").unwrap();
        std::fs::write(
            root.join("unidades/u1/BACKUPS_HISTORICOS/t1/err.txt"),
            b"err",
        )
        .unwrap();
        std::fs::write(&outside, b"externo").unwrap();
        std::os::unix::fs::symlink(&outside, share.join("enlace_externo")).unwrap();
        std::fs::set_permissions(&share, std::fs::Permissions::from_mode(0o750)).unwrap();

        // Bloqueos obligatorios (raíces de respaldo, logs, papelera y áreas temporales)
        for bad in [
            "BACKUPS_HISTORICOS",
            "BACKUPS_PERMANENTES",
            "LOGS_BACKUP",
            "LOGS_BACKUP/x",
            ".trash",
            ".trash/x",
            ".nas_staging",
            ".nas_staging/x",
            ".tmp_zip",
            ".tmp_zip/x",
            "unidades",
            "unidades/u1",
            "unidades/u1/BACKUPS_HISTORICOS",
            "unidades/u1/BACKUPS_PERMANENTES",
            "unidades/u1/LOGS_BACKUP/x",
            "../etc/passwd",
            "/etc/passwd",
            "SISTEMAS//copiado_error.txt",
            "SISTEMAS/./copiado_error.txt",
        ] {
            assert!(
                resolve_privileged_delete_target(&root, None, bad).is_err(),
                "debe rechazar ruta protegida/inválida: {bad}"
            );
        }
        assert!(resolve_privileged_delete_target(&root, Some("backups"), "").is_err());

        // Permitidos dentro de BACKUPS_HISTORICOS y BACKUPS_PERMANENTES
        assert!(
            resolve_privileged_delete_target(&root, None, "BACKUPS_HISTORICOS/t1/err.txt").is_ok()
        );
        assert!(
            resolve_privileged_delete_target(&root, None, "BACKUPS_PERMANENTES/t1/err.txt").is_ok()
        );
        assert!(resolve_privileged_delete_target(&root, Some("backups"), "t1/err.txt").is_ok());
        assert!(resolve_privileged_delete_target(
            &root,
            Some("unidades/u1"),
            "BACKUPS_HISTORICOS/t1/err.txt"
        )
        .is_ok());

        // Borrar archivo regular, carpeta y symlink (sin tocar destino externo ni permisos del padre)
        let f_target =
            resolve_privileged_delete_target(&root, None, "SISTEMAS/copiado_error.txt").unwrap();
        delete_validated_target(&f_target).unwrap();
        assert!(!share.join("copiado_error.txt").exists());

        let s_target =
            resolve_privileged_delete_target(&root, None, "SISTEMAS/enlace_externo").unwrap();
        delete_validated_target(&s_target).unwrap();
        assert!(
            outside.exists(),
            "el destino externo del symlink no debe borrarse"
        );

        let d_target =
            resolve_privileged_delete_target(&root, None, "SISTEMAS/subcarpeta").unwrap();
        delete_validated_target(&d_target).unwrap();
        assert!(!nested.exists());

        let mode_after = std::fs::metadata(&share).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode_after, 0o750,
            "no debe alterar permisos del directorio padre"
        );

        // Unidad adicional válida
        std::fs::write(root.join("unidades/u1/COMPARTIDA/err.txt"), b"x").unwrap();
        let u_target =
            resolve_privileged_delete_target(&root, Some("unidades/u1"), "COMPARTIDA/err.txt")
                .unwrap();
        delete_validated_target(&u_target).unwrap();
        assert!(!root.join("unidades/u1/COMPARTIDA/err.txt").exists());

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn ciclo_papelera_privilegiada_en_backups_y_shares() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("nas-priv-trash-{}-{nanos}", std::process::id()));
        let backup_dir = root.join("BACKUPS_HISTORICOS/t1");
        std::fs::create_dir_all(&backup_dir).unwrap();
        std::fs::set_permissions(&backup_dir, std::fs::Permissions::from_mode(0o750)).unwrap();
        std::fs::write(backup_dir.join("err.txt"), b"copia_erronea").unwrap();

        let runner = Runner::new();
        let (base, target, rel) =
            resolve_privileged_target_with_base(&root, Some("backups"), "t1/err.txt").unwrap();
        assert_eq!(rel, "BACKUPS_HISTORICOS/t1/err.txt");

        trash_validated_target(&runner, &base, &target, &rel, "admin").unwrap();
        assert!(!backup_dir.join("err.txt").exists());

        // Debe existir entrada en .trash y metadato con privileged=true
        let meta_dir = base.join(".trash/.meta");
        let meta_entries: Vec<_> = std::fs::read_dir(&meta_dir).unwrap().flatten().collect();
        assert_eq!(meta_entries.len(), 1);
        let id = meta_entries[0].file_name().to_string_lossy().to_string();
        let meta = read_priv_trash_meta(&base, &id).unwrap();
        assert!(meta.privileged);
        assert_eq!(meta.original_path, "BACKUPS_HISTORICOS/t1/err.txt");

        // Restaurar a su ubicación dentro de BACKUPS_HISTORICOS
        restore_privileged_trash_entry(&runner, &root, &base, &id).unwrap();
        assert!(backup_dir.join("err.txt").exists());
        assert_eq!(
            std::fs::metadata(&backup_dir).unwrap().permissions().mode() & 0o777,
            0o750
        );

        // Enviar de nuevo a papelera y purgar
        let (base2, target2, rel2) =
            resolve_privileged_target_with_base(&root, None, "BACKUPS_HISTORICOS/t1/err.txt")
                .unwrap();
        trash_validated_target(&runner, &base2, &target2, &rel2, "admin").unwrap();
        let id2 = std::fs::read_dir(base2.join(".trash/.meta"))
            .unwrap()
            .flatten()
            .next()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .to_string();
        purge_privileged_trash_entry(&runner, &base2, &id2).unwrap();
        assert!(!base2.join(".trash").join(&id2).exists());
        assert!(!base2.join(".trash/.meta").join(&id2).exists());

        // Probar empty_privileged_trash con 2 elementos
        std::fs::write(backup_dir.join("e1.txt"), b"1").unwrap();
        std::fs::write(backup_dir.join("e2.txt"), b"2").unwrap();
        for f in [
            "BACKUPS_HISTORICOS/t1/e1.txt",
            "BACKUPS_HISTORICOS/t1/e2.txt",
        ] {
            let (b, t, r) = resolve_privileged_target_with_base(&root, None, f).unwrap();
            trash_validated_target(&runner, &b, &t, &r, "admin").unwrap();
        }
        let msg = empty_privileged_trash(&runner, &base).unwrap();
        assert!(msg.contains("2 elementos eliminados"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn rechaza_symlinks_en_trash_y_meta() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("nas-priv-sym-{}-{nanos}", std::process::id()));
        let outside =
            std::env::temp_dir().join(format!("nas-priv-sym-out-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(root.join("SISTEMAS")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("SISTEMAS/a.txt"), b"hola").unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".trash")).unwrap();

        let runner = Runner::new();
        let (base, target, rel) =
            resolve_privileged_target_with_base(&root, None, "SISTEMAS/a.txt").unwrap();
        assert!(trash_validated_target(&runner, &base, &target, &rel, "admin").is_err());
        assert!(resolve_privileged_trash_entry(&base, "123_a.txt").is_err());
        assert!(empty_privileged_trash(&runner, &base).is_err());

        // Ahora .trash real, pero .trash/.meta como symlink externo
        std::fs::remove_file(root.join(".trash")).unwrap();
        std::fs::create_dir_all(root.join(".trash")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".trash/.meta")).unwrap();
        let rec = PrivTrashMetaRecord {
            original_path: "SISTEMAS/a.txt".into(),
            filename: "a.txt".into(),
            deleted_by: "admin".into(),
            deleted_at: "2026-01-01 00:00:00".into(),
            size_bytes: None,
            privileged: true,
            was_immutable: false,
        };
        assert!(write_privileged_trash_meta(&base, "123_a.txt", &rec).is_err());
        assert!(!outside.join("123_a.txt").exists());

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
