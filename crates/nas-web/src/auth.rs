//! Autenticación por cookie firmada (HMAC-SHA256).
//!
//! Sesión sin estado en servidor: la cookie contiene `usuario|expiración|firma`.
//! La contraseña se valida contra su SHA-256 y todas las comparaciones son de
//! tiempo constante.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Configuración de autenticación del panel.
#[derive(Clone)]
pub struct Auth {
    user: String,
    password_sha256: String,
    secret: Vec<u8>,
    ttl_secs: i64,
    secure_cookie: bool,
}

impl Auth {
    /// Crea la configuración a partir de usuario, hash SHA-256 de la contraseña y secreto HMAC.
    pub fn new(
        user: impl Into<String>,
        password_sha256: impl Into<String>,
        secret: impl AsRef<[u8]>,
        ttl_secs: i64,
        secure_cookie: bool,
    ) -> Self {
        Self {
            user: user.into(),
            password_sha256: password_sha256.into().to_lowercase(),
            secret: secret.as_ref().to_vec(),
            ttl_secs,
            secure_cookie,
        }
    }

    /// Nombre de la cookie de sesión.
    pub const COOKIE: &'static str = "nas_session";

    /// Verifica usuario y contraseña.
    pub fn verify_credentials(&self, user: &str, password: &str) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let provided = hex::encode(hasher.finalize());

        ct_eq(user, &self.user) & ct_eq(&provided, &self.password_sha256)
    }

    /// Emite el valor de cookie para el usuario, con expiración y firma.
    pub fn issue_cookie(&self, now: i64) -> String {
        let exp = now + self.ttl_secs;
        let payload = format!("{}|{}", self.user, exp);
        format!("{payload}|{}", self.sign(&payload))
    }

    /// Construye la cabecera `Set-Cookie` correspondiente.
    pub fn set_cookie_header(&self, value: &str) -> String {
        let mut header = format!(
            "{}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
            Self::COOKIE,
            self.ttl_secs
        );
        if self.secure_cookie {
            header.push_str("; Secure");
        }
        header
    }

    /// Cabecera para eliminar la cookie.
    pub fn clear_cookie_header(&self) -> String {
        format!(
            "{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
            Self::COOKIE
        )
    }

    /// Comprueba la cookie de una petición y devuelve el usuario autenticado.
    pub fn authenticate(&self, cookies: &str, now: i64) -> Option<String> {
        let raw = cookies
            .split(';')
            .filter_map(|part| part.trim().split_once('='))
            .find(|(name, _)| *name == Self::COOKIE)
            .map(|(_, value)| value)?;

        let mut parts = raw.split('|');
        let user = parts.next()?;
        let exp: i64 = parts.next()?.parse().ok()?;
        let signature = parts.next()?;
        if parts.next().is_some() {
            return None;
        }

        if exp <= now {
            return None;
        }

        let expected = self.sign(&format!("{user}|{exp}"));
        if ct_eq(signature, &expected) {
            Some(user.to_string())
        } else {
            None
        }
    }

    fn sign(&self, payload: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("clave HMAC válida");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth")
            .field("user", &self.user)
            .field("ttl_secs", &self.ttl_secs)
            .field("secure_cookie", &self.secure_cookie)
            .finish_non_exhaustive()
    }
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

    fn auth() -> Auth {
        Auth::new("admin", sha256_hex("Secreta123"), b"clave-hmac", 3600, true)
    }

    fn sha256_hex(value: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(value.as_bytes());
        hex::encode(hasher.finalize())
    }

    #[test]
    fn valida_credenciales_correctas() {
        assert!(auth().verify_credentials("admin", "Secreta123"));
        assert!(!auth().verify_credentials("admin", "otra"));
        assert!(!auth().verify_credentials("root", "Secreta123"));
    }

    #[test]
    fn emite_y_valida_cookie() {
        let auth = auth();
        let cookie = auth.issue_cookie(1_000);
        assert!(auth
            .authenticate(&format!("otra=1; nas_session={cookie}"), 1_000)
            .is_some());
    }

    #[test]
    fn rechaza_cookie_expirada_o_manipulada() {
        let auth = auth();
        let cookie = auth.issue_cookie(1_000);
        assert!(auth.authenticate(&cookie, 9_999).is_none());

        let tampered = cookie.replace("admin", "root");
        assert!(auth.authenticate(&tampered, 1_000).is_none());
    }

    #[test]
    fn cabeceras_de_cookie_incluyen_atributos_de_seguridad() {
        let auth = auth();
        let header = auth.set_cookie_header("valor");
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("SameSite=Strict"));
        assert!(header.contains("Secure"));
        assert!(auth.clear_cookie_header().contains("Max-Age=0"));
    }
}
