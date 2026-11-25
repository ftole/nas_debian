use std::process::ExitCode;

use nas_core::{ApiResponse, NasPaths, NasResult};

use super::common::print_json;

pub fn audit_verify(json: bool) -> NasResult<ExitCode> {
    let paths = NasPaths::from_env();
    let log = nas_audit::AuditLog::new(paths.audit_log());

    let result = log.verify();
    let (ok, detail) = match &result {
        Ok(count) => (true, format!("{count} entrada(s) verificadas")),
        Err(error) => (false, error.to_string()),
    };

    if json {
        let data = serde_json::json!({ "ok": ok, "detail": detail, "path": log.path().display().to_string() });
        let payload = if ok {
            ApiResponse::ok(data, "Verificación de auditoría")
        } else {
            ApiResponse::err(data, "Verificación de auditoría fallida")
        };
        print_json(&payload)?;
    } else {
        println!("AUDITORÍA: {detail}");
    }

    if ok {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}
