//! Papelera de reciclaje interna (`.trash`) con metadatos de autoría y restauración.

use std::path::{Path, PathBuf};

use nas_core::{NasError, NasResult};

use super::{
    dir_size_recursive, set_dir_permissions,
    util::{format_trash_stamp, human_size, split_trash_name},
    FileRoot,
};

/// Entrada de la papelera interna.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TrashEntry {
    /// Identificador único en `.trash` (`<epoch_ms>_<nombre>`).
    pub id: String,
    /// Nombre original del elemento.
    pub name: String,
    /// Alias de nombre consumido por el frontend (`app.js`).
    pub filename: String,
    /// Ruta relativa original antes del envío a la papelera.
    pub original_path: String,
    /// Usuario que envió el elemento a la papelera.
    pub deleted_by: String,
    /// Indica si el elemento es un directorio.
    pub is_dir: bool,
    /// Tamaño real en bytes (recursivo para carpetas).
    pub size: u64,
    /// Tamaño legible.
    pub size_formatted: String,
    /// Marca de borrado (`YYYY-MM-DD HH:MM:SS`).
    pub deleted_at: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct TrashMetaRecord {
    #[serde(default)]
    original_path: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    deleted_by: String,
    #[serde(default)]
    deleted_at: String,
}

impl FileRoot {
    /// Mueve un elemento a la papelera interna (`.trash`).
    pub fn trash(&self, relative: &str) -> NasResult<PathBuf> {
        self.trash_with_actor(relative, "sistemas")
    }

    /// Mueve un elemento a la papelera interna registrando el usuario que lo eliminó.
    pub fn trash_with_actor(&self, relative: &str, actor: &str) -> NasResult<PathBuf> {
        let source = self.resolve(relative)?;
        if source == self.root {
            return Err(NasError::Denied("no se puede eliminar la raíz".into()));
        }
        if let Some(desc) = self.is_protected_path(relative) {
            return Err(NasError::Denied(format!(
                "operación no permitida: {desc} están protegidos contra escritura e inmutables (chattr +i); no se pueden mover a la papelera. Para gestionar o depurar copias de seguridad, utilice el módulo Respaldos"
            )));
        }
        if !source.exists() {
            return Err(NasError::NotFound(format!("no existe: {relative}")));
        }

        let trash = self.root.join(".trash");
        std::fs::create_dir_all(&trash).map_err(|e| NasError::io(&trash, e))?;
        set_dir_permissions(&trash);

        let stamp_ms = next_unique_trash_ms(&trash);
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "elemento".to_string());
        let id = format!("{stamp_ms}_{name}");
        let target = trash.join(&id);
        let deleted_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        std::fs::rename(&source, &target).map_err(|e| {
            if e.raw_os_error() == Some(1) {
                NasError::Denied(format!(
                    "operación no permitida (EPERM): el elemento '{relative}' o su directorio contenedor es de solo lectura o inmutable"
                ))
            } else if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(13) {
                NasError::Denied(format!(
                    "permiso denegado: el servicio web no tiene permisos suficientes para mover '{relative}' a la papelera"
                ))
            } else if e.raw_os_error() == Some(18) {
                NasError::Invalid(format!(
                    "no se puede mover '{relative}' a la papelera entre diferentes sistemas de archivos o unidades de disco"
                ))
            } else {
                NasError::io(&source, e)
            }
        })?;
        if let Err(error) = self.write_trash_meta(&id, relative, &name, actor, &deleted_at) {
            let _ = std::fs::rename(&target, &source);
            return Err(error);
        }
        Ok(target)
    }

    /// Lista la papelera interna con metadatos completos.
    pub fn trash_entries(&self) -> NasResult<Vec<TrashEntry>> {
        let trash = self.root.join(".trash");
        if !trash.is_dir() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for item in std::fs::read_dir(&trash).map_err(|e| NasError::io(&trash, e))? {
            let item = item.map_err(|e| NasError::io(&trash, e))?;
            let id = item.file_name().to_string_lossy().to_string();
            if id.starts_with('.') {
                continue;
            }
            let entry_path = item.path();
            let metadata = item.metadata().ok();
            let is_dir = metadata.as_ref().is_some_and(|m| m.is_dir());
            let size = if is_dir {
                dir_size_recursive(&entry_path)
            } else {
                metadata.as_ref().map_or(0, |m| m.len())
            };

            let (stamp, raw_name) = split_trash_name(&id);
            let meta = self.read_trash_meta_record(&id);
            let name = meta
                .as_ref()
                .map(|m| m.filename.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(raw_name)
                .to_string();
            let original_path = meta
                .as_ref()
                .map(|m| m.original_path.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(name.as_str())
                .to_string();
            let deleted_by = meta
                .as_ref()
                .map(|m| m.deleted_by.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("sistemas")
                .to_string();
            let deleted_at = meta
                .as_ref()
                .map(|m| m.deleted_at.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format_trash_stamp(stamp));

            entries.push(TrashEntry {
                id,
                filename: name.clone(),
                name,
                original_path,
                deleted_by,
                is_dir,
                size,
                size_formatted: human_size(size),
                deleted_at,
            });
        }
        entries.sort_by(|a, b| {
            b.deleted_at
                .cmp(&a.deleted_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(entries)
    }

    /// Restaura un elemento de la papelera a su ruta original.
    pub fn restore_trash(&self, id: &str) -> NasResult<String> {
        let (real_id, source) = self.resolve_trash_entry(id)?;
        let (_, fallback_name) = split_trash_name(&real_id);
        let meta = self.read_trash_meta_record(&real_id);
        let name = meta
            .as_ref()
            .map(|m| m.filename.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_name);
        let relative = meta
            .as_ref()
            .map(|m| m.original_path.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| name.to_string());

        let mut target = self.resolve(&relative)?;
        if target == self.root || target.parent().is_none() {
            target = self.root.join(name);
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
            set_dir_permissions(parent);
        }

        let mut counter = 1;
        while target.exists() {
            let candidate = format!("{name}.{counter}");
            target = match target.parent() {
                Some(parent) => parent.join(&candidate),
                None => self.root.join(&candidate),
            };
            counter += 1;
        }

        std::fs::rename(&source, &target).map_err(|e| {
            if e.raw_os_error() == Some(1) {
                NasError::Denied(format!(
                    "operación no permitida (EPERM): el destino '{relative}' o su directorio contenedor es inmutable o de solo lectura"
                ))
            } else if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(13) {
                NasError::Denied(format!(
                    "permiso denegado: el servicio web no tiene permisos suficientes para restaurar '{relative}'"
                ))
            } else {
                NasError::io(&target, e)
            }
        })?;
        let _ = self.remove_trash_meta(&real_id);
        Ok(target
            .strip_prefix(&self.root)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.to_string()))
    }

    /// Elimina definitivamente un elemento de la papelera.
    pub fn delete_trash(&self, id: &str) -> NasResult<()> {
        let (real_id, path) = self.resolve_trash_entry(id)?;
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| NasError::io(&path, e))
        } else {
            std::fs::remove_file(&path).map_err(|e| NasError::io(&path, e))
        };
        let _ = self.remove_trash_meta(&real_id);
        result
    }

    fn trash_meta_dir(&self) -> PathBuf {
        self.root.join(".trash").join(".meta")
    }

    fn write_trash_meta(
        &self,
        id: &str,
        relative: &str,
        filename: &str,
        actor: &str,
        deleted_at: &str,
    ) -> NasResult<()> {
        let dir = self.trash_meta_dir();
        std::fs::create_dir_all(&dir).map_err(|e| NasError::io(&dir, e))?;
        let record = TrashMetaRecord {
            original_path: relative.to_string(),
            filename: filename.to_string(),
            deleted_by: if actor.trim().is_empty() {
                "sistemas".to_string()
            } else {
                actor.trim().to_string()
            },
            deleted_at: deleted_at.to_string(),
        };
        let json = serde_json::to_string(&record)
            .map_err(|e| NasError::Invalid(format!("metadatos de papelera: {e}")))?;
        let path = dir.join(id);
        std::fs::write(&path, json).map_err(|e| NasError::io(&path, e))
    }

    fn read_trash_meta_record(&self, id: &str) -> Option<TrashMetaRecord> {
        let path = self.trash_meta_dir().join(id);
        let value = std::fs::read_to_string(path).ok()?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with('{') {
            if let Ok(record) = serde_json::from_str::<TrashMetaRecord>(trimmed) {
                return Some(record);
            }
        }
        Some(TrashMetaRecord {
            original_path: trimmed.to_string(),
            ..TrashMetaRecord::default()
        })
    }

    fn remove_trash_meta(&self, id: &str) -> std::io::Result<()> {
        let path = self.trash_meta_dir().join(id);
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Vacía la papelera completa.
    pub fn empty_trash(&self) -> NasResult<u32> {
        let entries = self.trash_entries()?;
        let mut removed = 0;
        for entry in entries {
            if self.delete_trash(&entry.id).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn resolve_trash_entry(&self, id: &str) -> NasResult<(String, PathBuf)> {
        let id = id.trim();
        if id.is_empty()
            || id.starts_with('.')
            || id.contains('/')
            || id.contains('\\')
            || id.contains("..")
            || id.contains('\0')
        {
            return Err(NasError::Denied(
                "identificador de papelera inválido".into(),
            ));
        }
        let trash = self.root.join(".trash");
        let exact = trash.join(id);
        if exact.exists() {
            return Ok((id.to_string(), exact));
        }
        if id.bytes().all(|b| b.is_ascii_digit()) && trash.is_dir() {
            let prefix = format!("{id}_");
            if let Ok(entries) = std::fs::read_dir(&trash) {
                let mut matches: Vec<(String, PathBuf)> = entries
                    .flatten()
                    .filter_map(|entry| {
                        let name = entry.file_name().to_string_lossy().to_string();
                        name.starts_with(&prefix).then(|| (name, entry.path()))
                    })
                    .collect();
                if matches.len() == 1 {
                    return Ok(matches.remove(0));
                }
            }
        }
        Err(NasError::NotFound(format!("no existe en papelera: {id}")))
    }
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

#[cfg(test)]
mod trash_tests {
    use super::FileRoot;

    fn root(tag: &str) -> FileRoot {
        let dir = std::env::temp_dir().join(format!(
            "nas-trash-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        FileRoot::new(dir).expect("root")
    }

    #[test]
    fn escribe_lista_restaura_y_vacia() {
        let root = root("flujo");
        root.write("SISTEMAS/nota.txt", "hola").expect("write");
        assert_eq!(root.read("SISTEMAS/nota.txt", 1024).expect("read"), b"hola");

        root.trash("SISTEMAS/nota.txt").expect("trash");
        let entries = root.trash_entries().expect("lista");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "nota.txt");

        let restored = root.restore_trash(&entries[0].id).expect("restore");
        assert_eq!(restored, "SISTEMAS/nota.txt");
        assert!(root.read("SISTEMAS/nota.txt", 1024).is_ok());

        root.trash("SISTEMAS/nota.txt").expect("trash 2");
        assert_eq!(root.empty_trash().expect("vaciar"), 1);
        assert!(root.trash_entries().expect("lista").is_empty());
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn restaura_a_la_ruta_original_con_subdirectorios() {
        let root = root("anidado");
        root.write("A/B/nota.txt", "datos").expect("write");
        root.trash("A/B/nota.txt").expect("trash");

        let entries = root.trash_entries().expect("lista");
        assert_eq!(entries[0].name, "nota.txt");

        let restored = root.restore_trash(&entries[0].id).expect("restore");
        assert_eq!(restored, "A/B/nota.txt");
        assert_eq!(root.read("A/B/nota.txt", 64).expect("contenido"), b"datos");
        assert!(root.trash_entries().expect("lista").is_empty());
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn rechaza_ids_peligrosos() {
        let root = root("seguridad");
        assert!(root.restore_trash("../etc/passwd").is_err());
        assert!(root.delete_trash("a/b").is_err());
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn rechaza_modificaciones_en_rutas_protegidas() {
        let root = root("protegidas");
        assert!(root.trash("BACKUPS_HISTORICOS/snap/hosts").is_err());
        assert!(root.trash("BACKUPS_PERMANENTES/full/hosts").is_err());
        assert!(root.trash("LOGS_BACKUP/tarea.log").is_err());
        assert!(root.trash(".trash/algo").is_err());
        assert!(root.delete_permanent("BACKUPS_HISTORICOS/snap").is_err());
        assert!(root
            .rename("BACKUPS_HISTORICOS/snap", "BACKUPS_HISTORICOS/snap2")
            .is_err());
        assert!(root.mkdir("BACKUPS_HISTORICOS/nueva").is_err());
        assert!(root.write("BACKUPS_HISTORICOS/nuevo.txt", "x").is_err());
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn soporta_id_numerico_parseint_tamano_recursivo_y_borrado_permanente() {
        let root = root("parseint");
        root.write("CARPETA/a.txt", "12345").expect("a");
        root.write("CARPETA/b.txt", "6789").expect("b");
        root.trash_with_actor("CARPETA", "admin").expect("trash");

        let entries = root.trash_entries().expect("lista");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "CARPETA");
        assert_eq!(entries[0].original_path, "CARPETA");
        assert_eq!(entries[0].deleted_by, "admin");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, 9);

        let numeric_prefix = entries[0].id.split('_').next().expect("prefijo");
        assert!(numeric_prefix.bytes().all(|b| b.is_ascii_digit()));
        let restored = root.restore_trash(numeric_prefix).expect("restore numeric");
        assert_eq!(restored, "CARPETA");
        root.delete_permanent("CARPETA")
            .expect("borrado permanente");
        assert!(!root.root().join("CARPETA").exists());
        let _ = std::fs::remove_dir_all(root.root());
    }
}
