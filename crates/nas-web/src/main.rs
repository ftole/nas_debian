//! Arranque del servidor `nas-web`.

use std::net::SocketAddr;
use std::path::Path;

use nas_core::{NasPaths, ResourceBudget, ServerRole};
use nas_web::{router, AppState, Auth, Db};
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("nas-web {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "nas-web {}\nServidor web del panel NAS Debian 13\n\nUSO:\n    nas-web\n\nOPCIONES:\n    -h, --help       Imprime esta ayuda\n    -V, --version    Imprime la versión",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }

    let budget = ResourceBudget::detect();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(budget.tokio_worker_threads)
        .max_blocking_threads(budget.tokio_blocking_threads)
        .thread_name("nas-web-worker")
        .enable_all()
        .build()?;

    runtime.block_on(async_main(budget))
}

async fn async_main(budget: ResourceBudget) -> Result<(), Box<dyn std::error::Error>> {
    let bind = std::env::var("NAS_WEB_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let user = std::env::var("NAS_WEB_USER").unwrap_or_else(|_| "admin".to_string());

    let paths = NasPaths::from_env();
    let db_path = paths.database();
    let db = Db::open(&db_path)?;
    let _ = db.purge_expired(chrono::Utc::now().timestamp());

    // Alta inicial del administrador si la base aún no lo contiene.
    if db.user_hash(&user)?.is_none() {
        let password_hash = match std::env::var("NAS_WEB_PASSWORD_SHA256") {
            Ok(hash)
                if hash.starts_with("$pbkdf2-sha512$")
                    || (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())) =>
            {
                hash
            }
            _ => match std::env::var("NAS_WEB_DEV_PASSWORD") {
                Ok(password) => {
                    eprintln!("aviso: usando NAS_WEB_DEV_PASSWORD (solo desarrollo)");
                    Auth::hash_password(&password)?
                }
                Err(_) => {
                    return Err(
                        "define NAS_WEB_PASSWORD_SHA256 (o NAS_WEB_DEV_PASSWORD en desarrollo)"
                            .into(),
                    )
                }
            },
        };
        db.upsert_user(&user, &password_hash, "superadmin")?;
        eprintln!("aviso: usuario '{user}' creado en la base de datos del panel");
    }

    let secure_cookie = std::env::var("NAS_WEB_INSECURE_COOKIE").as_deref() != Ok("1");
    let ttl_secs = std::env::var("NAS_WEB_TTL_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8 * 3600);

    let csrf_secret = load_or_create_csrf_secret(&paths.state_dir);
    let role = ServerRole::from_file(&paths.role_file)?.unwrap_or(ServerRole::ArquivosBackup);
    let state_dir_for_mig = paths.state_dir.clone();
    let data_root_for_mig = paths.data_root.clone();
    let auth = Auth::with_csrf_secret(db, ttl_secs, secure_cookie, csrf_secret);
    let state = AppState::new(auth, paths, role).with_budget(budget.clone());
    let app = router(state);

    let addr: SocketAddr = bind.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!(
        "nas-web escuchando en http://{addr} (rol {}, sqlite {}, workers {}, blocking {}, sys_ops {}, heavy_ops {})",
        role.as_str(),
        db_path.display(),
        budget.tokio_worker_threads,
        budget.tokio_blocking_threads,
        budget.max_concurrent_system_ops,
        budget.max_concurrent_heavy_ops,
    );

    maybe_trigger_backup_dedup_migration(&state_dir_for_mig, &data_root_for_mig);

    axum::serve(listener, app).await?;
    Ok(())
}

fn maybe_trigger_backup_dedup_migration(state_dir: &Path, data_root: &Path) {
    if std::env::var_os("NAS_WEB_SKIP_MIGRATION").is_some()
        || data_root != Path::new("/srv/nas")
        || !Path::new("/usr/local/bin/nas").is_file()
    {
        return;
    }
    let marker = state_dir.join(nas_engine::backup_dedup::DEDUP_MIGRATION_MARKER);
    if marker.is_file() {
        return;
    }
    let has_backup_dirs = data_root.join("BACKUPS_HISTORICOS").is_dir()
        || data_root.join("BACKUPS_PERMANENTES").is_dir()
        || data_root.join("unidades").is_dir();
    if !has_backup_dirs {
        return;
    }

    drop(tokio::task::spawn_blocking(|| {
        let runner = nas_exec::Runner::new();
        let _ = runner.run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args(["helper", "backup-dedup-migration"])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        );
    }));
}

fn load_or_create_csrf_secret(state_dir: &Path) -> [u8; 32] {
    if let Ok(secret) = std::env::var("NAS_WEB_SECRET") {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            return derive_key(trimmed.as_bytes());
        }
    }

    let secret_file = state_dir.join("csrf.secret");
    if let Ok(existing) = std::fs::read(&secret_file) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&secret_file, std::fs::Permissions::from_mode(0o600));
        }
        if existing.len() >= 32 {
            return derive_key(&existing);
        }
    }

    let mut random = [0u8; 32];
    let _ = getrandom::getrandom(&mut random);
    let hex_key = hex::encode(random);
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&secret_file)
        {
            let _ = f.write_all(hex_key.as_bytes());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::write(&secret_file, &hex_key);
    }
    derive_key(hex_key.as_bytes())
}

fn derive_key(material: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(material);
    hasher.finalize().into()
}
