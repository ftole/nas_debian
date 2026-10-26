//! Assets originales del panel, servidos byte a byte desde el binario.

/// Comportamiento del panel (sin modificar).
pub const APP_JS: &[u8] = include_bytes!("../assets/js/app.js");
/// Visor de hojas de cálculo.
pub const XLSX_JS: &[u8] = include_bytes!("../assets/js/vendor/xlsx.full.min.js");
/// Compresión en el navegador.
pub const JSZIP_JS: &[u8] = include_bytes!("../assets/js/vendor/jszip.min.js");
/// Vista previa de documentos Word.
pub const DOCX_JS: &[u8] = include_bytes!("../assets/js/vendor/docx-preview.min.js");
