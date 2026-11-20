//! Helper para aplicación atómica y validada de configuraciones de Samba.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

/// Escribe `bytes` en un temporal `O_EXCL` en el mismo directorio que `target`, sincroniza
/// a disco (`sync_all`) y reemplaza `target` mediante `rename` atómico POSIX.
fn atomic_install_bytes(target: &Path, bytes: &[u8], mode: u32) -> NasResult<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("/"));
    let base = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let mut tmp_path = None;
    let mut file = None;
    for attempt in 0..32u32 {
        let candidate = parent.join(format!(".{base}.tmp.{pid}.{nanos}.{attempt}"));
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(mode);
        }
        match opts.open(&candidate) {
            Ok(f) => {
                tmp_path = Some(candidate);
                file = Some(f);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(NasError::Denied(format!(
                    "no se pudo crear temporal atómico para {}: {e}",
                    target.display()
                )));
            }
        }
    }

    let (Some(tmp), Some(mut f)) = (tmp_path, file) else {
        return Err(NasError::Denied(format!(
            "no se pudo asignar temporal atómico para {}",
            target.display()
        )));
    };

    if let Err(e) = f.write_all(bytes).and_then(|()| f.sync_all()) {
        drop(f);
        let _ = std::fs::remove_file(&tmp);
        return Err(NasError::Denied(format!(
            "error al escribir temporal {}: {e}",
            target.display()
        )));
    }
    drop(f);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode));
    }

    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(NasError::Denied(format!(
            "error al reemplazar atómicamente {}: {e}",
            target.display()
        )));
    }

    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Aplica de forma atómica una configuración preparada en staging para Samba.
///
/// Valida la ruta (prefijo `/var/lib/nas/staging/smbconf_`), ausencia de `..`,
/// existencia del archivo, sintaxis con `testparm -s`, respaldo preventivo de
/// `/etc/samba/smb.conf`, copia atómica y recarga de `smbd`.
pub fn apply_staged_samba(runner: &Runner, staged: &Path) -> NasResult<String> {
    let path_str = staged.to_string_lossy();
    if !path_str.starts_with("/var/lib/nas/staging/smbconf_") || path_str.contains("..") {
        return Err(NasError::Denied(format!(
            "ruta no permitida para configuración staged: {path_str}"
        )));
    }
    if !staged.exists() {
        return Err(NasError::NotFound(format!(
            "archivo staged no existe: {path_str}"
        )));
    }

    // 1. Validar sintaxis con testparm
    let test_out = runner.run(
        &CommandSpec::new("testparm")
            .args(["-s", &path_str])
            .timeout(Duration::from_secs(30)),
    )?;
    if !test_out.is_success() {
        let err_msg = if test_out.stderr.is_empty() {
            test_out.stdout
        } else {
            test_out.stderr
        };
        return Err(NasError::Invalid(format!(
            "testparm rechazó la configuración: {err_msg}"
        )));
    }

    let staged_bytes = std::fs::read(staged)
        .map_err(|e| NasError::Denied(format!("no se pudo leer {path_str}: {e}")))?;

    // 2. Respaldo preventivo del smb.conf actual (crítico: sin respaldo no se fija)
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let backup_path = format!("/etc/samba/smb.conf.bak.{stamp}");
    let current_conf = Path::new("/etc/samba/smb.conf");
    let prev_bytes = if current_conf.exists() {
        let existing = std::fs::read(current_conf).map_err(|e| {
            NasError::Denied(format!("no se pudo leer /etc/samba/smb.conf actual: {e}"))
        })?;
        atomic_install_bytes(Path::new(&backup_path), &existing, 0o644)?;
        Some(existing)
    } else {
        None
    };

    // 3. Instalación atómica de la configuración validada (O_EXCL + fsync + rename)
    atomic_install_bytes(current_conf, &staged_bytes, 0o644)?;

    let _ = std::fs::remove_file(staged);

    // 4. Recarga de Samba con reversión automática si falla
    let reload = runner.run(
        &CommandSpec::new("systemctl")
            .args(["reload", "smbd"])
            .timeout(Duration::from_secs(30)),
    )?;
    if let Err(reload_err) = reload.ensure_success() {
        if let Some(prev) = prev_bytes {
            let _ = atomic_install_bytes(current_conf, &prev, 0o644);
        }
        return Err(reload_err);
    }

    Ok(format!("smb.conf aplicado (respaldo: {backup_path})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rechaza_rutas_fuera_de_staging() {
        let runner = Runner::new();
        let err = apply_staged_samba(&runner, Path::new("/tmp/smbconf_1234")).unwrap_err();
        assert!(err.to_string().contains("ruta no permitida"));
    }

    #[test]
    fn rechaza_rutas_con_salto_de_directorio() {
        let runner = Runner::new();
        let err = apply_staged_samba(
            &runner,
            Path::new("/var/lib/nas/staging/smbconf_../etc/passwd"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("ruta no permitida"));
    }

    #[test]
    fn rechaza_archivo_inexistente() {
        let runner = Runner::new();
        let err = apply_staged_samba(
            &runner,
            Path::new("/var/lib/nas/staging/smbconf_inexistente_99999"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no existe"));
    }

    #[test]
    fn atomic_install_bytes_reemplaza_y_no_deja_temporales() {
        let dir = std::env::temp_dir().join(format!(
            "nas_samba_atomic_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let target = dir.join("smb.conf");
        std::fs::write(&target, b"[global]\nworkgroup = OLD\n").expect("seed");

        atomic_install_bytes(&target, b"[global]\nworkgroup = NEW\n", 0o644)
            .expect("atomic install");

        assert_eq!(
            std::fs::read_to_string(&target).expect("read"),
            "[global]\nworkgroup = NEW\n"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("readdir")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
