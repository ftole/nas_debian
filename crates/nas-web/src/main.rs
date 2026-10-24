//! Arranque del servidor `nas-web`.

use std::net::SocketAddr;

use nas_core::{NasPaths, ServerRole};
use nas_web::{router, AppState, Auth, Db};
use sha2::{Digest, Sha256};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind = std::env::var("NAS_WEB_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let user = std::env::var("NAS_WEB_USER").unwrap_or_else(|_| "admin".to_string());

    let password_hash = match std::env::var("NAS_WEB_PASSWORD_SHA256") {
        Ok(hash) if hash.len() == 64 => hash,
        _ => match std::env::var("NAS_WEB_DEV_PASSWORD") {
            Ok(password) => {
                eprintln!("aviso: usando NAS_WEB_DEV_PASSWORD (solo desarrollo)");
                sha256_hex(&password)
            }
            Err(_) => {
                return Err(
                    "define NAS_WEB_PASSWORD_SHA256 (o NAS_WEB_DEV_PASSWORD en desarrollo)".into(),
                )
            }
        },
    };

    let secure_cookie = std::env::var("NAS_WEB_INSECURE_COOKIE").as_deref() != Ok("1");
    let ttl_secs = std::env::var("NAS_WEB_TTL_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8 * 3600);

    let paths = NasPaths::from_env();
    let db_path = paths.database();
    let db = Db::open(&db_path)?;

    // Alta inicial del administrador si la base aún no lo contiene.
    if db.user_hash(&user)?.is_none() {
        db.upsert_user(&user, &password_hash, "superadmin")?;
        eprintln!("aviso: usuario '{user}' creado en la base de datos del panel");
    }

    let role = ServerRole::from_file(&paths.role_file)?.unwrap_or(ServerRole::ArquivosBackup);
    let auth = Auth::new(db, ttl_secs, secure_cookie);
    let state = AppState::new(auth, paths, role);
    let app = router(state);

    let addr: SocketAddr = bind.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!(
        "nas-web escuchando en http://{addr} (rol {}, sqlite {})",
        role.as_str(),
        db_path.display()
    );

    axum::serve(listener, app).await?;
    Ok(())
}

fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}
