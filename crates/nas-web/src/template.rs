//! Renderizador de las plantillas portadas 1:1 desde `layout.php`/`login.php`.
//!
//! Las plantillas generadas por `scripts/php2html.py` conservan el HTML original
//! y usan dos marcadores:
//! - `<!--IF:flag--> ... <!--ELSE--> ... <!--ENDIF-->` para condicionales.
//! - `%%token%%` para valores dinámicos (ya escapados por el llamador).

use std::collections::HashMap;

/// Plantilla con marcadores.
#[derive(Debug, Clone, Copy)]
pub struct Template<'a> {
    source: &'a str,
}

impl<'a> Template<'a> {
    /// Crea la plantilla a partir del texto generado.
    pub const fn new(source: &'a str) -> Self {
        Self { source }
    }

    /// Renderiza con los flags y tokens indicados.
    pub fn render(&self, flags: &HashMap<&str, bool>, tokens: &HashMap<&str, String>) -> String {
        let source = self.source;
        let mut out = String::with_capacity(source.len());
        let mut stack: Vec<Frame> = Vec::new();
        let mut active = true;
        let mut index = 0usize;

        while index < source.len() {
            let next = next_marker(source, index);

            match next {
                None => {
                    push_text(&mut out, &source[index..], active, tokens);
                    break;
                }
                Some((position, Kind::If(name), length)) => {
                    push_text(&mut out, &source[index..position], active, tokens);
                    let condition = *flags.get(name).unwrap_or(&false);
                    stack.push(Frame {
                        parent_active: active,
                        condition,
                        in_else: false,
                    });
                    active = active && condition;
                    index = position + length;
                }
                Some((position, Kind::Else, length)) => {
                    push_text(&mut out, &source[index..position], active, tokens);
                    if let Some(frame) = stack.last_mut() {
                        frame.in_else = true;
                        active = frame.parent_active && !frame.condition;
                    }
                    index = position + length;
                }
                Some((position, Kind::EndIf, length)) => {
                    push_text(&mut out, &source[index..position], active, tokens);
                    if let Some(frame) = stack.pop() {
                        active = frame.parent_active;
                    }
                    index = position + length;
                }
                Some((position, Kind::Token, length)) => {
                    push_text(&mut out, &source[index..position], active, tokens);
                    if active {
                        let name = &source[position + 2..position + length - 2];
                        if let Some(value) = tokens.get(name) {
                            out.push_str(value);
                        }
                    }
                    index = position + length;
                }
            }
        }

        out
    }
}

#[derive(Debug)]
struct Frame {
    parent_active: bool,
    condition: bool,
    in_else: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Kind<'a> {
    If(&'a str),
    Else,
    EndIf,
    Token,
}

fn next_marker(source: &str, from: usize) -> Option<(usize, Kind<'_>, usize)> {
    let rest = &source[from..];
    let if_pos = rest.find("<!--IF:");
    let else_pos = rest.find("<!--ELSE-->");
    let endif_pos = rest.find("<!--ENDIF-->");
    let token_pos = rest.find("%%");

    let candidates = [(if_pos, 0u8), (else_pos, 1), (endif_pos, 2), (token_pos, 3)];
    let (position, kind_id) = candidates
        .into_iter()
        .filter_map(|(p, k)| p.map(|p| (p, k)))
        .min_by_key(|(p, _)| *p)?;

    match kind_id {
        0 => {
            let end = rest[position..].find("-->")? + position;
            let name = &rest[position + 7..end];
            Some((from + position, Kind::If(name), end + 3 - position))
        }
        1 => Some((from + position, Kind::Else, "<!--ELSE-->".len())),
        2 => Some((from + position, Kind::EndIf, "<!--ENDIF-->".len())),
        _ => {
            let end = rest[position + 2..].find("%%")? + position + 2;
            let length = end + 2 - position;
            Some((from + position, Kind::Token, length))
        }
    }
}

fn push_text(out: &mut String, text: &str, active: bool, tokens: &HashMap<&str, String>) {
    if !active {
        return;
    }
    let mut index = 0usize;
    while let Some(position) = text[index..].find("%%") {
        out.push_str(&text[index..index + position]);
        let start = index + position + 2;
        let Some(end) = text[start..].find("%%") else {
            out.push_str(&text[index + position..]);
            return;
        };
        let name = &text[start..start + end];
        if let Some(value) = tokens.get(name) {
            out.push_str(value);
        }
        index = start + end + 2;
    }
    out.push_str(&text[index..]);
}

#[cfg(test)]
mod tests {
    use super::Template;
    use std::collections::HashMap;

    #[test]
    fn resuelve_tokens_y_condicionales() {
        let template = Template::new(
            "<p>%%a%%</p><!--IF:x-->X<!--ELSE-->Y<!--ENDIF--><!--IF:y-->Z<!--ENDIF-->",
        );
        let mut flags = HashMap::new();
        flags.insert("x", true);
        flags.insert("y", false);

        let mut tokens = HashMap::new();
        tokens.insert("a", "hola".to_string());

        assert_eq!(template.render(&flags, &tokens), "<p>hola</p>X");
    }

    #[test]
    fn deja_vacio_el_token_desconocido() {
        let template = Template::new("a%%desconocido%%b");
        assert_eq!(template.render(&HashMap::new(), &HashMap::new()), "ab");
    }
}
