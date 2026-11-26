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
use serde::Deserialize;

use super::{is_protected_share_path, is_valid_unit_name};

/// Solicitud de eliminación privilegiada enviada por `stdin` a `nas helper files-delete`.
#[derive(Deserialize)]
pub struct PrivilegedDeleteRequest {
    /// Raíz lógica (`nas` o `unidades/<id>`).
    #[serde(default)]
    pub root: Option<String>,
    /// Ruta relativa dentro de la raíz seleccionada.
    pub path: String,
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
            .field("path", &self.path)
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
                let target_ok =
                    target.contains("ALL") || target.split(':').any(|p| p.trim() == "root");
                let cmd_clean = after
                    .strip_prefix("NOPASSWD:")
                    .or_else(|| after.strip_prefix("PASSWD:"))
                    .unwrap_or(after)
                    .trim();
                if target_ok && cmd_clean == "ALL" {
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

fn resolve_root_base(data_root: &Path, root_name: Option<&str>) -> NasResult<PathBuf> {
    let raw = root_name.unwrap_or("").trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("nas") || raw.eq_ignore_ascii_case("principal") {
        return Ok(data_root.to_path_buf());
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
    Ok(unit_path)
}

fn is_protected_relative_under_nas(rel: &Path) -> bool {
    let synthetic = Path::new("/srv/nas").join(rel);
    is_protected_share_path(&synthetic.to_string_lossy())
}

/// Resuelve y valida estrictamente el objetivo dentro de carpetas compartidas/unidades.
pub fn resolve_privileged_delete_target(
    data_root: &Path,
    root_name: Option<&str>,
    rel_path: &str,
) -> NasResult<PathBuf> {
    let rel_trimmed = rel_path.trim();
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

    let root_base = resolve_root_base(data_root, root_name)?;
    let candidate = root_base.join(rel_trimmed);
    if candidate == data_root || candidate == root_base {
        return Err(NasError::Denied(
            "no se puede eliminar el directorio raíz".into(),
        ));
    }

    let rel_from_data_root = candidate
        .strip_prefix(data_root)
        .map_err(|_| NasError::Denied("ruta fuera de la raíz NAS".into()))?;
    if is_protected_relative_under_nas(rel_from_data_root) {
        return Err(NasError::Denied(
            "operación prohibida: respaldos y directorios del sistema están protegidos".into(),
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
    let canon_rel = final_target
        .strip_prefix(&canon_data)
        .map_err(|_| NasError::Denied("la ruta escapa de la raíz NAS".into()))?;
    if is_protected_relative_under_nas(canon_rel) {
        return Err(NasError::Denied(
            "operación prohibida: ruta canónica protegida".into(),
        ));
    }
    Ok(final_target)
}

/// Elimina el objetivo validado sin alterar permisos, ACLs ni atributos extendidos.
pub fn delete_validated_target(target: &Path) -> NasResult<String> {
    let meta = std::fs::symlink_metadata(target)
        .map_err(|_| NasError::NotFound(format!("elemento no encontrado: {}", target.display())))?;

    let map_io = |e: std::io::Error| {
        if e.raw_os_error() == Some(1) {
            NasError::Denied(
                "operación denegada (EPERM): el elemento es inmutable (+i) o está en un volumen de solo lectura; no se alteran atributos automáticamente".into(),
            )
        } else {
            NasError::io(target, e)
        }
    };

    if meta.file_type().is_symlink() || !meta.is_dir() {
        std::fs::remove_file(target).map_err(map_io)?;
    } else {
        std::fs::remove_dir_all(target).map_err(map_io)?;
    }
    Ok(format!(
        "Elemento eliminado con privilegios: {}",
        target.display()
    ))
}

/// Autentica al usuario OS (`root` o `sudo`) y elimina el elemento validado.
pub fn privileged_delete_file(
    runner: &Runner,
    data_root: &Path,
    req: &PrivilegedDeleteRequest,
) -> NasResult<String> {
    verify_privileged_os_credentials(runner, &req.auth_user, &req.auth_password)?;
    let target = resolve_privileged_delete_target(data_root, req.root.as_deref(), &req.path)?;
    delete_validated_target(&target)
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
        std::fs::create_dir_all(root.join("unidades/u1/BACKUPS_HISTORICOS")).unwrap();
        std::fs::create_dir_all(root.join("unidades/u1/COMPARTIDA")).unwrap();
        std::fs::write(share.join("copiado_error.txt"), b"dato").unwrap();
        std::fs::write(nested.join("archivo.bin"), b"123").unwrap();
        std::fs::write(&outside, b"externo").unwrap();
        std::os::unix::fs::symlink(&outside, share.join("enlace_externo")).unwrap();
        std::fs::set_permissions(&share, std::fs::Permissions::from_mode(0o750)).unwrap();

        // Bloqueos obligatorios
        for bad in [
            "BACKUPS_HISTORICOS/x",
            "BACKUPS_PERMANENTES/x",
            "LOGS_BACKUP/x",
            ".trash/x",
            ".nas_staging/x",
            "unidades",
            "unidades/u1",
            "unidades/u1/BACKUPS_HISTORICOS/x",
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
}
