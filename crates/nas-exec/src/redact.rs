//! Redacción de secretos y credenciales en textos de salida.

use std::sync::OnceLock;

use regex::Regex;

/// Sustituye secretos conocidos y patrones sensibles por una máscara.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    /// Crea un redactor vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra un secreto literal que debe ocultarse en cualquier salida.
    pub fn add_secret(&mut self, secret: impl Into<String>) {
        let secret = secret.into();
        if secret.len() >= 3 && !self.secrets.contains(&secret) {
            self.secrets.push(secret);
            // Ordenar por longitud descendente para evitar enmascaramiento parcial de subcadenas.
            self.secrets.sort_by_key(|a| std::cmp::Reverse(a.len()));
        }
    }

    /// Devuelve el texto con secretos y valores sensibles enmascarados.
    pub fn redact(&self, input: &str) -> String {
        let mut output = input.to_string();

        for secret in &self.secrets {
            if !secret.is_empty() {
                output = output.replace(secret.as_str(), "***");
            }
        }

        static SENSITIVE: OnceLock<Regex> = OnceLock::new();
        let pattern = SENSITIVE.get_or_init(|| {
            Regex::new(
                r"(?i)(password|passwd|contrasena|contraseña|token|secret|clave)\s*[=:]\s*\S+",
            )
            .expect("regex de redacción válida")
        });

        pattern
            .replace_all(&output, |caps: &regex::Captures<'_>| {
                format!("{}=***", &caps[1])
            })
            .to_string()
    }

    /// Indica si hay secretos registrados.
    pub fn has_secrets(&self) -> bool {
        !self.secrets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Redactor;

    #[test]
    fn oculta_secretos_literales() {
        let mut redactor = Redactor::new();
        redactor.add_secret("S3cr3t-Value");
        let masked = redactor.redact("clave usada: S3cr3t-Value");
        assert_eq!(masked, "clave usada: ***");
        assert!(!masked.contains("S3cr3t"));
    }

    #[test]
    fn oculta_asignaciones_sensibles() {
        let redactor = Redactor::new();
        let masked = redactor.redact("password=hunter2\ntoken: abc123\nusuario=juan");
        assert!(!masked.contains("hunter2"));
        assert!(!masked.contains("abc123"));
        assert!(masked.contains("usuario=juan"));
    }

    #[test]
    fn ignora_secretos_demasiado_cortos() {
        let mut redactor = Redactor::new();
        redactor.add_secret("ab");
        assert!(!redactor.has_secrets());
    }

    #[test]
    fn ordena_secretos_por_longitud_descendente() {
        let mut redactor = Redactor::new();
        redactor.add_secret("clave");
        redactor.add_secret("clave-maestra-123");
        let masked = redactor.redact("usando clave-maestra-123 y clave");
        assert_eq!(masked, "usando *** y ***");
        assert!(!masked.contains("maestra"));
        assert!(!masked.contains("123"));
    }
}
