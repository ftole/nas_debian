//! Autenticación con sesiones persistentes en SQLite, PBKDF2-HMAC-SHA512 y CSRF.
//!
//! La cookie transporta un identificador aleatorio de 256 bits; la sesión vive
//! en la base de datos con expiración. Las comparaciones de hash son de tiempo
//! constante y la cookie es `HttpOnly`, `SameSite=Strict` y `Secure` por defecto.

use hmac::{Hmac, Mac};
use nas_core::{NasError, NasResult};
use sha2::{Digest, Sha256, Sha512};

use crate::db::Db;

type HmacSha256 = Hmac<Sha256>;
type HmacSha512 = Hmac<Sha512>;

/// Iteraciones por defecto de PBKDF2-HMAC-SHA512 (recomendación OWASP).
pub const PBKDF2_ITERATIONS: u32 = 210_000;

/// Ventana de validez de un token CSRF emitido en `/login` (segundos).
const LOGIN_CSRF_TTL_SECS: i64 = 7200;

/// Configuración de autenticación del panel.
#[derive(Clone)]
pub struct Auth {
    db: Db,
    ttl_secs: i64,
    secure_cookie: bool,
    csrf_secret: [u8; 32],
    enforce_csrf: bool,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth")
            .field("db", &self.db.path())
            .field("ttl_secs", &self.ttl_secs)
            .field("secure_cookie", &self.secure_cookie)
            .field("enforce_csrf", &self.enforce_csrf)
            .finish()
    }
}

impl Auth {
    /// Nombre de la cookie de sesión.
    pub const COOKIE: &'static str = "nas_session";

    /// Crea la configuración sobre una base de datos ya abierta.
    pub fn new(db: Db, ttl_secs: i64, secure_cookie: bool) -> Self {
        let mut csrf_secret = [0x5au8; 32];
        let _ = getrandom::getrandom(&mut csrf_secret);
        Self {
            db,
            ttl_secs,
            secure_cookie,
            csrf_secret,
            enforce_csrf: false,
        }
    }

    /// Crea la configuración con clave secreta explícita y exigencia estricta de CSRF.
    pub fn with_csrf_secret(
        db: Db,
        ttl_secs: i64,
        secure_cookie: bool,
        csrf_secret: [u8; 32],
    ) -> Self {
        Self {
            db,
            ttl_secs,
            secure_cookie,
            csrf_secret,
            enforce_csrf: true,
        }
    }

    /// Acceso a la base de datos subyacente.
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Indica si la validación CSRF obligatoria está activa.
    pub fn enforce_csrf(&self) -> bool {
        self.enforce_csrf
    }

    /// Deriva un hash PBKDF2-HMAC-SHA512 con sal aleatoria de 128 bits.
    pub fn hash_password(password: &str) -> NasResult<String> {
        let mut salt = [0u8; 16];
        getrandom::getrandom(&mut salt)
            .map_err(|error| NasError::Invalid(format!("generador aleatorio: {error}")))?;
        let dk = pbkdf2_sha512(password.as_bytes(), &salt, PBKDF2_ITERATIONS);
        Ok(format!(
            "$pbkdf2-sha512${PBKDF2_ITERATIONS}${}${}",
            hex::encode(salt),
            hex::encode(dk)
        ))
    }

    /// Verifica una contraseña contra un hash PBKDF2-HMAC-SHA512 o legado SHA-256.
    pub fn verify_password(password: &str, stored: &str) -> bool {
        if let Some(rest) = stored.strip_prefix("$pbkdf2-sha512$") {
            let mut parts = rest.split('$');
            let Some(iter_str) = parts.next() else {
                return false;
            };
            let Some(salt_hex) = parts.next() else {
                return false;
            };
            let Some(dk_hex) = parts.next() else {
                return false;
            };
            if parts.next().is_some() {
                return false;
            }
            let Ok(iterations) = iter_str.parse::<u32>() else {
                return false;
            };
            if !(1_000..=1_000_000).contains(&iterations) {
                return false;
            }
            let Ok(salt) = hex::decode(salt_hex) else {
                return false;
            };
            let Ok(expected) = hex::decode(dk_hex) else {
                return false;
            };
            if salt.len() < 8 || expected.len() != 64 {
                return false;
            }
            let derived = pbkdf2_sha512(password.as_bytes(), &salt, iterations);
            ct_eq_bytes(&derived, &expected)
        } else if stored.len() == 64 && stored.bytes().all(|b| b.is_ascii_hexdigit()) {
            let mut hasher = Sha256::new();
            hasher.update(password.as_bytes());
            let provided = hex::encode(hasher.finalize());
            let normalized = stored.to_ascii_lowercase();
            ct_eq(&provided, &normalized)
        } else {
            false
        }
    }

    /// Valida credenciales y crea una sesión; actualiza hashes legados a PBKDF2.
    pub fn login(&self, username: &str, password: &str, now: i64) -> NasResult<Option<String>> {
        let _ = self.db.purge_expired(now);

        let (canonical_user, stored) = match self.db.user_hash(username)? {
            Some(hash) => (username.to_string(), hash),
            None if username.eq_ignore_ascii_case("admin") => match self.db.user_hash("admin")? {
                Some(hash) => ("admin".to_string(), hash),
                None => return Ok(None),
            },
            None => return Ok(None),
        };

        if !Self::verify_password(password, &stored) {
            return Ok(None);
        }

        if !stored.starts_with("$pbkdf2-sha512$") {
            if let Ok(upgraded) = Self::hash_password(password) {
                let role = self
                    .db
                    .user_role(&canonical_user)?
                    .unwrap_or_else(|| "operator".to_string());
                let _ = self.db.upsert_user(&canonical_user, &upgraded, &role);
            }
        }

        let id = random_id()?;
        self.db
            .create_session(&id, &canonical_user, now + self.ttl_secs)?;
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

    /// Emite un token CSRF ligado a un identificador de sesión concreto.
    pub fn csrf_for_session(&self, session_id: &str) -> String {
        self.hmac_hex(&format!("session:{session_id}"))
    }

    /// Emite el token CSRF correspondiente a las cookies actuales.
    pub fn csrf_for_cookies(&self, cookies: &str) -> String {
        match cookie_value(cookies) {
            Some(id) => self.csrf_for_session(id),
            None => self.issue_login_csrf(chrono::Utc::now().timestamp()),
        }
    }

    /// Emite un token CSRF firmado (hex minúsculas) para el formulario de `/login`.
    pub fn issue_login_csrf(&self, now: i64) -> String {
        let ts_hex = format!("{:016x}", now.max(0) as u64);
        let mac = self.hmac_hex(&format!("login:{ts_hex}"));
        format!("{ts_hex}{mac}")
    }

    /// Verifica el token CSRF de `/login` o `/api/auth/login`.
    pub fn verify_login_csrf(&self, token: Option<&str>, now: i64) -> bool {
        let provided = token.map(str::trim).unwrap_or_default();
        if provided.is_empty() {
            return !self.enforce_csrf;
        }
        if provided.len() != 80 || !provided.bytes().all(|b| b.is_ascii_hexdigit()) {
            return false;
        }
        let ts_hex = &provided[..16];
        let mac_hex = provided[16..].to_ascii_lowercase();
        let Ok(ts_u64) = u64::from_str_radix(ts_hex, 16) else {
            return false;
        };
        let Ok(ts) = i64::try_from(ts_u64) else {
            return false;
        };
        if now < ts.saturating_sub(60) || now.saturating_sub(ts) > LOGIN_CSRF_TTL_SECS {
            return false;
        }
        let expected = self.hmac_hex(&format!("login:{}", ts_hex.to_ascii_lowercase()));
        ct_eq(&mac_hex, &expected)
    }

    /// Verifica el token CSRF de una petición autenticada por sesión.
    pub fn verify_session_csrf(&self, cookies: &str, header_token: Option<&str>) -> bool {
        let Some(session_id) = cookie_value(cookies) else {
            return !self.enforce_csrf;
        };
        let provided = header_token.map(str::trim).unwrap_or_default();
        if provided.is_empty() {
            return !self.enforce_csrf;
        }
        let expected = self.csrf_for_session(session_id);
        let normalized = provided.to_ascii_lowercase();
        ct_eq(&normalized, &expected)
    }

    fn hmac_hex(&self, message: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.csrf_secret).expect("HMAC admite claves de 32 bytes");
        mac.update(message.as_bytes());
        hex::encode(mac.finalize().into_bytes())
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

fn pbkdf2_sha512(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 64] {
    let prf = HmacSha512::new_from_slice(password).expect("HMAC admite cualquier tamaño de clave");
    let mut first = prf.clone();
    first.update(salt);
    first.update(&1u32.to_be_bytes());
    let mut u: [u8; 64] = first.finalize().into_bytes().into();
    let mut dk = u;

    for _ in 1..iterations {
        let mut mac = prf.clone();
        mac.update(&u);
        u = mac.finalize().into_bytes().into();
        for (d, b) in dk.iter_mut().zip(u.iter()) {
            *d ^= *b;
        }
    }
    dk
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
        .map_err(|error| NasError::Invalid(format!("generador aleatorio: {error}")))?;
    Ok(hex::encode(buffer))
}

fn ct_eq(a: &str, b: &str) -> bool {
    ct_eq_bytes(a.as_bytes(), b.as_bytes())
}

fn ct_eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
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

    #[test]
    fn migra_sha256_a_pbkdf2_y_valida_csrf() {
        let db = Db::in_memory().expect("db");
        db.upsert_user("admin", &sha256_hex("ClaveFuerte!99"), "superadmin")
            .expect("alta");
        let auth = Auth::with_csrf_secret(db.clone(), 3600, true, [0x42u8; 32]);

        let session = auth
            .login("Admin", "ClaveFuerte!99", 5_000)
            .expect("login")
            .expect("sesión");
        let upgraded = db.user_hash("admin").expect("hash").expect("valor");
        assert!(upgraded.starts_with("$pbkdf2-sha512$210000$"));
        assert!(Auth::verify_password("ClaveFuerte!99", &upgraded));
        assert!(!Auth::verify_password("Incorrecta", &upgraded));

        let login_token = auth.issue_login_csrf(5_000);
        assert!(login_token.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(auth.verify_login_csrf(Some(&login_token), 5_010));
        assert!(!auth.verify_login_csrf(None, 5_010));
        assert!(!auth.verify_login_csrf(Some(&login_token), 20_000));

        let cookie = format!("{}={session}", Auth::COOKIE);
        let csrf = auth.csrf_for_cookies(&cookie);
        assert!(auth.verify_session_csrf(&cookie, Some(&csrf)));
        assert!(!auth.verify_session_csrf(&cookie, None));
        assert!(!auth.verify_session_csrf(&cookie, Some("deadbeef")));
    }
}
