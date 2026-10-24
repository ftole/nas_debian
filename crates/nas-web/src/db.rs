//! Persistencia SQLite del panel: sesiones y usuarios web.
//!
//! Usa la misma base que el panel PHP (`/var/lib/nas/nas.sqlite`) con tablas
//! propias, en modo WAL y con purga de sesiones expiradas.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nas_core::{NasError, NasResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Entrada de auditoría del panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditRow {
    /// Identificador incremental.
    pub id: i64,
    /// Marca de tiempo UNIX.
    pub ts: i64,
    /// Usuario que ejecutó la acción.
    pub username: String,
    /// Acción lógica.
    pub action: String,
    /// Objeto afectado.
    pub target: String,
    /// Resultado (`SUCCESS`, `FAILED`, `DENIED`).
    pub result: String,
    /// Detalle libre.
    pub detail: String,
}

/// Acceso compartido a la base de datos.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").field("path", &self.path).finish()
    }
}

fn db_error(error: rusqlite::Error) -> NasError {
    NasError::Database(error.to_string())
}

impl Db {
    /// Abre (o crea) la base de datos y aplica el esquema.
    pub fn open(path: impl AsRef<Path>) -> NasResult<Self> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| NasError::io(dir, e))?;
            }
        }

        let conn = Connection::open(path).map_err(db_error)?;
        Self::prepare(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path: path.to_path_buf(),
        })
    }

    /// Base de datos en memoria (pruebas).
    pub fn in_memory() -> NasResult<Self> {
        let conn = Connection::open_in_memory().map_err(db_error)?;
        Self::prepare(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path: PathBuf::from(":memory:"),
        })
    }

    fn prepare(conn: &Connection) -> NasResult<()> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS web_users (
                 username TEXT PRIMARY KEY,
                 password_sha256 TEXT NOT NULL,
                 role TEXT NOT NULL DEFAULT 'operator',
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS web_sessions (
                 id TEXT PRIMARY KEY,
                 username TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 expires_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_web_sessions_expires ON web_sessions(expires_at);
             CREATE TABLE IF NOT EXISTS web_audit (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 ts INTEGER NOT NULL,
                 username TEXT NOT NULL,
                 action TEXT NOT NULL,
                 target TEXT NOT NULL,
                 result TEXT NOT NULL,
                 detail TEXT NOT NULL DEFAULT ''
             );
             CREATE INDEX IF NOT EXISTS idx_web_audit_ts ON web_audit(ts DESC);",
        )
        .map_err(db_error)
    }

    /// Ruta del archivo de base de datos.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Inserta o actualiza un usuario web.
    pub fn upsert_user(&self, username: &str, password_sha256: &str, role: &str) -> NasResult<()> {
        let conn = self.conn.lock().expect("mutex de base de datos");
        conn.execute(
            "INSERT INTO web_users (username, password_sha256, role, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(username) DO UPDATE SET password_sha256 = excluded.password_sha256, role = excluded.role",
            params![username, password_sha256, role, now_ts()],
        )
        .map_err(db_error)?;
        Ok(())
    }

    /// Hash almacenado de un usuario.
    pub fn user_hash(&self, username: &str) -> NasResult<Option<String>> {
        let conn = self.conn.lock().expect("mutex de base de datos");
        let hash = conn
            .query_row(
                "SELECT password_sha256 FROM web_users WHERE username = ?1",
                params![username],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?;
        Ok(hash)
    }

    /// Rol almacenado de un usuario.
    pub fn user_role(&self, username: &str) -> NasResult<Option<String>> {
        let conn = self.conn.lock().expect("mutex de base de datos");
        let role = conn
            .query_row(
                "SELECT role FROM web_users WHERE username = ?1",
                params![username],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?;
        Ok(role)
    }

    /// Crea una sesión persistente.
    pub fn create_session(&self, id: &str, username: &str, expires_at: i64) -> NasResult<()> {
        let conn = self.conn.lock().expect("mutex de base de datos");
        conn.execute(
            "INSERT INTO web_sessions (id, username, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, username, now_ts(), expires_at],
        )
        .map_err(db_error)?;
        Ok(())
    }

    /// Devuelve el usuario de una sesión válida, purgando la expirada.
    pub fn session_user(&self, id: &str, now: i64) -> NasResult<Option<String>> {
        let conn = self.conn.lock().expect("mutex de base de datos");
        let row: Option<(String, i64)> = conn
            .query_row(
                "SELECT username, expires_at FROM web_sessions WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(db_error)?;

        match row {
            Some((username, expires_at)) if expires_at > now => Ok(Some(username)),
            Some(_) => {
                conn.execute("DELETE FROM web_sessions WHERE id = ?1", params![id])
                    .map_err(db_error)?;
                Ok(None)
            }
            None => Ok(None),
        }
    }

    /// Elimina una sesión (logout).
    pub fn delete_session(&self, id: &str) -> NasResult<()> {
        let conn = self.conn.lock().expect("mutex de base de datos");
        conn.execute("DELETE FROM web_sessions WHERE id = ?1", params![id])
            .map_err(db_error)?;
        Ok(())
    }

    /// Registra una acción en la auditoría web.
    pub fn append_audit(
        &self,
        username: &str,
        action: &str,
        target: &str,
        result: &str,
        detail: &str,
    ) -> NasResult<()> {
        let conn = self.conn.lock().expect("mutex de base de datos");
        conn.execute(
            "INSERT INTO web_audit (ts, username, action, target, result, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now_ts(), username, action, target, result, detail],
        )
        .map_err(db_error)?;
        Ok(())
    }

    /// Últimas entradas de auditoría (más recientes primero).
    pub fn recent_audit(&self, limit: usize) -> NasResult<Vec<AuditRow>> {
        let conn = self.conn.lock().expect("mutex de base de datos");
        let mut statement = conn
            .prepare(
                "SELECT id, ts, username, action, target, result, detail
                 FROM web_audit ORDER BY id DESC LIMIT ?1",
            )
            .map_err(db_error)?;

        let rows = statement
            .query_map(params![limit as i64], |row| {
                Ok(AuditRow {
                    id: row.get(0)?,
                    ts: row.get(1)?,
                    username: row.get(2)?,
                    action: row.get(3)?,
                    target: row.get(4)?,
                    result: row.get(5)?,
                    detail: row.get(6)?,
                })
            })
            .map_err(db_error)?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(db_error)?);
        }
        Ok(entries)
    }

    /// Purga sesiones expiradas y devuelve cuántas eliminó.
    pub fn purge_expired(&self, now: i64) -> NasResult<usize> {
        let conn = self.conn.lock().expect("mutex de base de datos");
        conn.execute(
            "DELETE FROM web_sessions WHERE expires_at <= ?1",
            params![now],
        )
        .map_err(db_error)
    }
}

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::Db;

    #[test]
    fn crea_y_valida_sesiones() {
        let db = Db::in_memory().expect("db");
        db.create_session("abc", "admin", 2_000).expect("sesión");

        assert_eq!(
            db.session_user("abc", 1_000).expect("consulta"),
            Some("admin".to_string())
        );
        assert_eq!(db.session_user("abc", 2_001).expect("consulta"), None);
        assert_eq!(db.session_user("abc", 1_500).expect("consulta"), None);
    }

    #[test]
    fn elimina_sesiones_al_cerrar_sesion() {
        let db = Db::in_memory().expect("db");
        db.create_session("abc", "admin", 9_999).expect("sesión");
        db.delete_session("abc").expect("borrado");
        assert_eq!(db.session_user("abc", 1).expect("consulta"), None);
    }

    #[test]
    fn purga_expiradas() {
        let db = Db::in_memory().expect("db");
        db.create_session("a", "admin", 100).expect("a");
        db.create_session("b", "admin", 10_000).expect("b");
        assert_eq!(db.purge_expired(5_000).expect("purga"), 1);
        assert_eq!(
            db.session_user("b", 5_000).expect("b"),
            Some("admin".to_string())
        );
    }

    #[test]
    fn guarda_y_actualiza_usuarios() {
        let db = Db::in_memory().expect("db");
        db.upsert_user("admin", &"aa".repeat(32), "superadmin")
            .expect("alta");
        assert_eq!(db.user_hash("admin").expect("hash"), Some("aa".repeat(32)));
        assert_eq!(
            db.user_role("admin").expect("rol"),
            Some("superadmin".to_string())
        );

        db.upsert_user("admin", &"bb".repeat(32), "admin")
            .expect("update");
        assert_eq!(db.user_hash("admin").expect("hash"), Some("bb".repeat(32)));
        assert_eq!(
            db.user_role("admin").expect("rol"),
            Some("admin".to_string())
        );
        assert_eq!(db.user_hash("fantasma").expect("hash"), None);
    }
}
