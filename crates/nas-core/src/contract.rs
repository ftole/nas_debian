//! Contrato de salida compartido por CLI, web y herramientas internas.
//!
//! Mantiene la misma envoltura que el panel PHP (`success`/`message`/`data`/`error`)
//! para que scripts, pruebas y clientes existentes sigan funcionando sin cambios.

use serde::{Deserialize, Serialize};

/// Respuesta estándar de la API y de la CLI en modo `--json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiResponse<T> {
    /// Indica si la operación concluyó correctamente.
    pub success: bool,
    /// Mensaje legible para el operador.
    pub message: String,
    /// Carga útil cuando `success` es verdadero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// Detalle del error cuando `success` es falso.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    /// Respuesta exitosa con datos.
    pub fn ok(data: T, message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: Some(data),
            error: None,
        }
    }

    /// Respuesta exitosa sin datos.
    pub fn ok_empty(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: None,
            error: None,
        }
    }
}

impl ApiResponse<serde_json::Value> {
    /// Respuesta de error con el mismo formato que usa el panel web.
    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            success: false,
            message: message.clone(),
            data: None,
            error: Some(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ApiResponse;

    #[test]
    fn serializa_envoltura_exitosa() {
        let payload = ApiResponse::ok(42, "ok");
        let json = serde_json::to_string(&payload).expect("json");
        assert_eq!(json, r#"{"success":true,"message":"ok","data":42}"#);
    }

    #[test]
    fn serializa_error_sin_data() {
        let payload = ApiResponse::<serde_json::Value>::error("fallo");
        let json = serde_json::to_string(&payload).expect("json");
        assert_eq!(
            json,
            r#"{"success":false,"message":"fallo","error":"fallo"}"#
        );
    }
}
