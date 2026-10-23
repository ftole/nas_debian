//! Parsers de las salidas del sistema operativo.
//!
//! Cada módulo convierte texto o JSON de utilidades externas (`testparm`,
//! `lsblk`, `ip`, `ss`, `btrfs`) en tipos verificables y probados.

/// `ip -j` (interfaces y ruta por defecto).
pub mod ip;
/// `lsblk -J` (dispositivos de bloque).
///
/// Convierte el árbol JSON en dispositivos tipados y filtra el disco del SO.
pub mod lsblk;
/// `/etc/resolv.conf`.
pub mod resolv;
/// `btrfs scrub status`.
pub mod scrub;
/// `smb.conf` y `testparm -s`.
pub mod smb;
/// `ss -H -tln` / `ss -H -uln`.
pub mod ss;
