//! Autenticación con sesiones persistentes en SQLite.
//!
//! La cookie transporta un identificador aleatorio de 256 bits; la sesión vive
//! en la base de datos con expiración. Las comparaciones de hash son de tiempo
//! constante y la cookie es `HttpOnly`, `SameSite=Strict` y `Secure` por defecto.

use nas_core::NasResult;
use sha2::{Digest, Sha256};

use crate::db::Db;

/// Configuración de autenticación del panel.
#[derive(Clone)]
pub struct Auth {
    db: Db,
    ttl_secs: i64,
    secure_cookie: bool,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth")
            .field("db", &self.db.path())
            .field("ttl_secs", &self.ttl_secs)
            .field("secure_cookie", &self.secure_cookie)
            .finish()
    }
}

impl Auth {
    /// Nombre de la cookie de sesión.
    pub const COOKIE: &'static str = "nas_session";

    /// Crea la configuración sobre una base de datos ya abierta.
    pub fn new(db: Db, ttl_secs: i64, secure_cookie: bool) -> Self {
        Self {
            db,
            ttl_secs,
            secure_cookie,
        }
    }

    /// Acceso a la base de datos subyacente.
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Valida credenciales y crea una sesión; devuelve el identificador.
    pub fn login(&self, username: &str, password: &str, now: i64) -> NasResult<Option<String>> {
        let Some(stored) = self.db.user_hash(username)? else {
            return Ok(None);
        };

        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let provided = hex::encode(hasher.finalize());

        if !ct_eq(&provided, &stored) {
            return Ok(None);
        }

        let id = random_id()?;
        self.db.create_session(&id, username, now + self.ttl_secs)?;
        Ok(Some(id))
    }

    /// Resuelve el usuario autenticado a partir de las cookies.
    pub fn authenticate(&self, cookies: &str, now: i64) -> NasResult<Option<String>> {
        let Some(id) = cookie_value(cookies) else {
            return Ok(None);
        };
        self.db.session_user(id, now)
    }

    /// Invalida la sesión asociada a las cookies.
    pub fn logout(&self, cookies: &str) -> NasResult<()> {
        if let Some(id) = cookie_value(cookies) {
            self.db.delete_session(id)?;
        }
        Ok(())
    }

    /// Cabecera `Set-Cookie` para una sesión nueva.
    pub fn set_cookie_header(&self, session_id: &str) -> String {
        let mut header = format!(
            "{}={session_id}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
            Self::COOKIE,
            self.ttl_secs
        );
        if self.secure_cookie {
            header.push_str("; Secure");
        }
        header
    }

    /// Cabecera `Set-Cookie` para eliminar la sesión.
    pub fn clear_cookie_header(&self) -> String {
        let mut header = format!(
            "{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
            Self::COOKIE
        );
        if self.secure_cookie {
            header.push_str("; Secure");
        }
        header
    }
}

fn cookie_value(cookies: &str) -> Option<&str> {
    cookies
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(name, _)| *name == Auth::COOKIE)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

fn random_id() -> NasResult<String> {
    let mut buffer = [0u8; 32];
    getrandom::getrandom(&mut buffer)
        .map_err(|error| nas_core::NasError::Invalid(format!("generador aleatorio: {error}")))?;
    Ok(hex::encode(buffer))
}

fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::Auth;
    use crate::db::Db;
    use sha2::{Digest, Sha256};

    fn sha256_hex(value: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(value.as_bytes());
        hex::encode(hasher.finalize())
    }

    fn auth() -> Auth {
        let db = Db::in_memory().expect("db");
        db.upsert_user("admin", &sha256_hex("Secreta123"), "superadmin")
            .expect("usuario");
        Auth::new(db, 3600, true)
    }

    #[test]
    fn inicia_sesion_con_credenciales_correctas() {
        let auth = auth();
        let id = auth
            .login("admin", "Secreta123", 1_000)
            .expect("login")
            .expect("sesión");
        assert_eq!(
            auth.authenticate(&format!("{}= {id}", Auth::COOKIE).replace(' ', ""), 1_000)
                .expect("auth"),
            Some("admin".to_string())
        );
    }

    #[test]
    fn rechaza_credenciales_incorrectas() {
        let auth = auth();
        assert!(auth.login("admin", "otra", 1_000).expect("login").is_none());
        assert!(auth
            .login("root", "Secreta123", 1_000)
            .expect("login")
            .is_none());
    }

    #[test]
    fn rechaza_sesion_expirada_y_cerrada() {
        let auth = auth();
        let id = auth
            .login("admin", "Secreta123", 1_000)
            .expect("login")
            .expect("id");
        let cookie = format!("{}={id}", Auth::COOKIE);

        assert!(auth.authenticate(&cookie, 9_999).expect("auth").is_none());

        let id = auth
            .login("admin", "Secreta123", 1_000)
            .expect("login")
            .expect("id");
        auth.logout(&format!("{}={id}", Auth::COOKIE))
            .expect("logout");
        assert!(auth
            .authenticate(&format!("{}={id}", Auth::COOKIE), 1_000)
            .expect("auth")
            .is_none());
    }

    #[test]
    fn cabeceras_incluyen_atributos_de_seguridad() {
        let auth = auth();
        let header = auth.set_cookie_header("abc");
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("SameSite=Strict"));
        assert!(header.contains("Secure"));
        assert!(auth.clear_cookie_header().contains("Max-Age=0"));
    }
}
