//! Cotización de argumentos para invocaciones que atraviesan un shell.
//!
//! Se usa al construir líneas de comando remotas sobre OpenSSH: cada argumento
//! se cotiza de forma individual antes de unirse, de modo que el shell remoto
//! nunca interpreta texto no confiable.

/// Cotiza un argumento con comillas simples POSIX.
pub fn quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c))
    {
        return arg.to_string();
    }

    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// Cotiza y une una lista de argumentos en una línea de comandos.
pub fn quote_all(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{quote, quote_all};

    #[test]
    fn deja_intactos_argumentos_seguros() {
        assert_eq!(quote("simple"), "simple");
        assert_eq!(quote("/var/www/nas-web"), "/var/www/nas-web");
        assert_eq!(quote("--link-dest=/srv/a"), "--link-dest=/srv/a");
    }

    #[test]
    fn cotiza_caracteres_peligrosos() {
        assert_eq!(quote("hola mundo"), "'hola mundo'");
        assert_eq!(quote("a;rm -rf /"), "'a;rm -rf /'");
        assert_eq!(quote("$(id)"), "'$(id)'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote(""), "''");
    }

    #[test]
    fn une_argumentos_cotizados() {
        let args = vec![
            "mkdir".to_string(),
            "-p".to_string(),
            "/tmp/a b".to_string(),
        ];
        assert_eq!(quote_all(&args), "mkdir -p '/tmp/a b'");
    }
}
