//! Verificación y planificación de actualizaciones firmadas.
//!
//! El manifiesto declara versión, commit, artefacto, SHA-256 y firma Ed25519.
//! La firma cubre una carga canónica estable (`version|commit|artifact|sha256|published_at`),
//! de modo que cualquier alteración del manifiesto o del paquete se detecta antes
//! de tocar el sistema.

use std::path::Path;

use ed25519_dalek::{Signature, VerifyingKey};
use nas_core::{NasError, NasResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod remote;

pub use remote::{
    download_and_verify_release, download_file, fetch_latest_release, RemoteReleaseInfo,
};

/// Manifiesto de una release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    /// Versión semántica (`1.2.3`).
    pub version: String,
    /// Commit de origen.
    pub commit: String,
    /// Nombre o ruta del artefacto (`nas-1.2.3.tar.gz`).
    pub artifact: String,
    /// SHA-256 hexadecimal del artefacto.
    pub sha256: String,
    /// Fecha de publicación en ISO-8601.
    pub published_at: String,
    /// Firma Ed25519 hexadecimal de la carga canónica.
    #[serde(default)]
    pub signature: Option<String>,
}

fn is_valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && !version.contains("..")
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

impl ReleaseManifest {
    /// Interpreta el manifiesto desde JSON.
    pub fn from_json(text: &str) -> NasResult<Self> {
        let manifest: Self = serde_json::from_str(text)
            .map_err(|error| NasError::Invalid(format!("manifiesto inválido: {error}")))?;
        if !is_valid_version(&manifest.version) {
            return Err(NasError::Invalid(format!(
                "versión inválida en el manifiesto: {}",
                manifest.version
            )));
        }
        Ok(manifest)
    }

    /// Carga el manifiesto desde disco.
    pub fn load(path: impl AsRef<Path>) -> NasResult<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| NasError::io(path, e))?;
        Self::from_json(&text)
    }

    /// Carga canónica cubierta por la firma (estable y sin ambigüedades).
    pub fn canonical_payload(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.version, self.commit, self.artifact, self.sha256, self.published_at
        )
    }

    /// Verifica que el artefacto coincida con el SHA-256 declarado.
    pub fn verify_artifact(&self, artifact: impl AsRef<Path>) -> NasResult<()> {
        let path = artifact.as_ref();
        let bytes = std::fs::read(path).map_err(|e| NasError::io(path, e))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let digest = hex::encode(hasher.finalize());

        if digest.eq_ignore_ascii_case(&self.sha256) {
            Ok(())
        } else {
            Err(NasError::Denied(format!(
                "SHA-256 del artefacto no coincide (esperado {}, obtenido {digest})",
                self.sha256
            )))
        }
    }

    /// Verifica la firma Ed25519 del manifiesto con la clave pública indicada.
    pub fn verify_signature(&self, public_key_hex: &str) -> NasResult<()> {
        let signature_hex = self.signature.as_deref().ok_or_else(|| {
            NasError::Denied("el manifiesto no incluye firma; actualización rechazada".into())
        })?;

        let key_bytes = decode_fixed::<32>(public_key_hex, "clave pública")?;
        let sig_bytes = decode_fixed::<64>(signature_hex, "firma")?;

        let key = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|error| NasError::Invalid(format!("clave pública inválida: {error}")))?;
        let signature = Signature::from_bytes(&sig_bytes);

        key.verify_strict(self.canonical_payload().as_bytes(), &signature)
            .map_err(|_| NasError::Denied("firma Ed25519 del manifiesto inválida".into()))
    }
}

/// Firma el manifiesto con una clave privada Ed25519 (hex de 32 bytes).
pub fn sign_manifest(manifest: &mut ReleaseManifest, signing_key: &str) -> NasResult<()> {
    use ed25519_dalek::Signer;

    let key_bytes = decode_fixed::<32>(signing_key, "clave privada")?;
    let key = ed25519_dalek::SigningKey::from_bytes(&key_bytes);
    let signature = key.sign(manifest.canonical_payload().as_bytes());
    manifest.signature = Some(hex::encode(signature.to_bytes()));
    Ok(())
}

/// Deriva la clave pública (hex) de una clave privada Ed25519 (hex).
pub fn public_key_from_secret(signing_key: &str) -> NasResult<String> {
    let key_bytes = decode_fixed::<32>(signing_key, "clave privada")?;
    let key = ed25519_dalek::SigningKey::from_bytes(&key_bytes);
    Ok(hex::encode(key.verifying_key().to_bytes()))
}

fn decode_fixed<const N: usize>(value: &str, label: &str) -> NasResult<[u8; N]> {
    let bytes = hex::decode(value.trim())
        .map_err(|error| NasError::Invalid(format!("{label} no es hexadecimal: {error}")))?;
    <[u8; N]>::try_from(bytes.as_slice())
        .map_err(|_| NasError::Invalid(format!("{label} debe tener {} bytes", N)))
}

/// Plan de actualización A/B sobre enlaces simbólicos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan {
    /// Versión objetivo.
    pub version: String,
    /// Directorio de la release.
    pub release_dir: String,
    /// Pasos ordenados de la actualización.
    pub steps: Vec<Vec<String>>,
    /// Pasos de reversión al estado anterior.
    pub rollback: Vec<Vec<String>>,
}

/// Construye el plan A/B para una release.
pub fn build_plan(
    manifest: &ReleaseManifest,
    root: &str,
    artifact: &str,
    previous_version: Option<&str>,
) -> UpdatePlan {
    let root = root.trim_end_matches('/');
    let release_dir = format!("{root}/releases/{}", manifest.version);
    let link = format!("{root}/current");
    let link_next = format!("{root}/current.new");

    let reloads = vec![
        vec![
            "systemctl".to_string(),
            "reload".to_string(),
            "nginx".to_string(),
        ],
        vec![
            "systemctl".to_string(),
            "restart".to_string(),
            "nas-web".to_string(),
        ],
    ];

    let mut steps = vec![
        vec!["mkdir".to_string(), "-p".to_string(), release_dir.clone()],
        vec![
            "tar".to_string(),
            "-xzf".to_string(),
            artifact.to_string(),
            "-C".to_string(),
            release_dir.clone(),
            "--strip-components=1".to_string(),
        ],
        vec![
            "test".to_string(),
            "-f".to_string(),
            format!("{release_dir}/Cargo.toml"),
        ],
        vec![
            "ln".to_string(),
            "-sfn".to_string(),
            release_dir.clone(),
            link_next.clone(),
        ],
        vec![
            "mv".to_string(),
            "-T".to_string(),
            link_next.clone(),
            link.clone(),
        ],
    ];

    for bin in ["nas", "nas-web", "nas-tui", "nasctl", "nas-dev"] {
        steps.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0755".to_string(),
            format!("{link}/bin/{bin}"),
            format!("/usr/local/bin/{bin}"),
        ]);
    }
    for helper in crate::deploy::helpers::ROOT_HELPERS {
        steps.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0755".to_string(),
            "-o".to_string(),
            "root".to_string(),
            "-g".to_string(),
            "root".to_string(),
            format!("{link}/packaging/{helper}"),
            format!("/usr/local/sbin/{helper}"),
        ]);
    }
    steps.push(vec![
        "install".to_string(),
        "-m".to_string(),
        "0644".to_string(),
        format!("{link}/version.json"),
        "/etc/nas/version.json".to_string(),
    ]);
    steps.push(vec![
        "install".to_string(),
        "-m".to_string(),
        "0644".to_string(),
        format!("{link}/version.json"),
        format!("{root}/version.json"),
    ]);

    steps.push(vec!["nginx".to_string(), "-t".to_string()]);
    steps.extend(reloads.clone());
    steps.push(vec![
        "curl".to_string(),
        "-k".to_string(),
        "-s".to_string(),
        "-o".to_string(),
        "/dev/null".to_string(),
        "-w".to_string(),
        "%{http_code}".to_string(),
        "https://127.0.0.1/login".to_string(),
    ]);

    let mut rollback = Vec::new();
    if let Some(previous) = previous_version {
        let previous_dir = format!("{root}/releases/{previous}");
        rollback.push(vec![
            "ln".to_string(),
            "-sfn".to_string(),
            previous_dir,
            link_next.clone(),
        ]);
        rollback.push(vec![
            "mv".to_string(),
            "-T".to_string(),
            link_next,
            link.clone(),
        ]);
        for bin in ["nas", "nas-web", "nas-tui", "nasctl", "nas-dev"] {
            rollback.push(vec![
                "install".to_string(),
                "-m".to_string(),
                "0755".to_string(),
                format!("{link}/bin/{bin}"),
                format!("/usr/local/bin/{bin}"),
            ]);
        }
        for helper in crate::deploy::helpers::ROOT_HELPERS {
            rollback.push(vec![
                "install".to_string(),
                "-m".to_string(),
                "0755".to_string(),
                "-o".to_string(),
                "root".to_string(),
                "-g".to_string(),
                "root".to_string(),
                format!("{link}/packaging/{helper}"),
                format!("/usr/local/sbin/{helper}"),
            ]);
        }
        rollback.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0644".to_string(),
            format!("{link}/version.json"),
            "/etc/nas/version.json".to_string(),
        ]);
        rollback.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0644".to_string(),
            format!("{link}/version.json"),
            format!("{root}/version.json"),
        ]);
        rollback.extend(reloads);
    }

    UpdatePlan {
        version: manifest.version.clone(),
        release_dir,
        steps,
        rollback,
    }
}

#[cfg(test)]
mod tests {
    use super::{build_plan, ReleaseManifest};
    use ed25519_dalek::{Signer, SigningKey};
    use rand_core::OsRng;

    fn manifest() -> ReleaseManifest {
        ReleaseManifest {
            version: "1.2.3".to_string(),
            commit: "abc123".to_string(),
            artifact: "nas-1.2.3.tar.gz".to_string(),
            sha256: "00".repeat(32),
            published_at: "2026-10-23T12:00:00Z".to_string(),
            signature: None,
        }
    }

    #[test]
    fn carga_canonica_es_estable() {
        let payload = manifest().canonical_payload();
        assert_eq!(
            payload,
            "1.2.3|abc123|nas-1.2.3.tar.gz|0000000000000000000000000000000000000000000000000000000000000000|2026-10-23T12:00:00Z"
        );
    }

    #[test]
    fn acepta_firma_valida_y_rechaza_alteraciones() {
        let signing = SigningKey::generate(&mut OsRng);
        let mut signed = manifest();
        let signature = signing.sign(signed.canonical_payload().as_bytes());
        signed.signature = Some(hex::encode(signature.to_bytes()));

        let public = hex::encode(signing.verifying_key().to_bytes());
        assert!(signed.verify_signature(&public).is_ok());

        let mut tampered = signed.clone();
        tampered.version = "9.9.9".to_string();
        assert!(tampered.verify_signature(&public).is_err());
    }

    #[test]
    fn rechaza_manifiesto_sin_firma() {
        assert!(manifest().verify_signature(&"11".repeat(32)).is_err());
    }

    #[test]
    fn rechaza_clave_o_firma_con_longitud_incorrecta() {
        let mut signed = manifest();
        signed.signature = Some("ab".repeat(10));
        assert!(signed.verify_signature(&"11".repeat(32)).is_err());
    }

    #[test]
    fn verifica_sha256_del_artefacto() {
        use sha2::{Digest, Sha256};

        let dir = std::env::temp_dir().join(format!("nas-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let artifact = dir.join("nas-1.2.3.tar.gz");
        std::fs::write(&artifact, b"contenido").expect("write");

        let mut hasher = Sha256::new();
        hasher.update(b"contenido");
        let digest = hex::encode(hasher.finalize());

        let mut release = manifest();
        release.sha256 = digest.clone();
        assert!(release.verify_artifact(&artifact).is_ok());

        release.sha256 = "ff".repeat(32);
        assert!(release.verify_artifact(&artifact).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_abi_cambia_enlace_y_revierte() {
        let plan = build_plan(
            &manifest(),
            "/opt/nas_debian",
            "/tmp/nas.tar.gz",
            Some("1.2.2"),
        );

        assert_eq!(plan.release_dir, "/opt/nas_debian/releases/1.2.3");
        assert!(plan
            .steps
            .iter()
            .any(|step| step.first().is_some_and(|arg| arg == "ln")));
        assert!(plan
            .steps
            .iter()
            .any(|step| step.contains(&"--strip-components=1".to_string())));
        assert!(plan
            .steps
            .iter()
            .any(|step| step.last().is_some_and(|arg| arg.contains("/login"))));

        assert_eq!(plan.rollback.len(), 18);
        assert!(plan.rollback[0].contains(&"/opt/nas_debian/releases/1.2.2".to_string()));
        assert!(plan.steps.iter().any(|step| {
            step == &vec![
                "install",
                "-m",
                "0755",
                "/opt/nas_debian/current/bin/nas-web",
                "/usr/local/bin/nas-web",
            ]
        }));
        assert!(plan.steps.iter().any(|step| {
            step == &vec![
                "install",
                "-m",
                "0755",
                "-o",
                "root",
                "-g",
                "root",
                "/opt/nas_debian/current/packaging/nas-share-dir",
                "/usr/local/sbin/nas-share-dir",
            ]
        }));
        assert!(plan.steps.iter().any(|step| {
            step == &vec![
                "install",
                "-m",
                "0644",
                "/opt/nas_debian/current/version.json",
                "/etc/nas/version.json",
            ]
        }));
        assert!(plan.steps.iter().any(|step| {
            step == &vec![
                "install",
                "-m",
                "0644",
                "/opt/nas_debian/current/version.json",
                "/opt/nas_debian/version.json",
            ]
        }));
        assert!(plan.rollback.iter().any(|step| {
            step == &vec![
                "install",
                "-m",
                "0644",
                "/opt/nas_debian/current/version.json",
                "/etc/nas/version.json",
            ]
        }));
    }

    #[test]
    fn plan_sin_version_previa_no_tiene_rollback() {
        let plan = build_plan(&manifest(), "/opt/nas_debian", "/tmp/nas.tar.gz", None);
        assert!(plan.rollback.is_empty());
    }

    #[test]
    fn rechaza_version_con_traversal_y_reinicia_nas_web() {
        let bad = r#"{"version":"../../etc","commit":"abc","artifact":"a.tar.gz","sha256":"00","published_at":"2026-01-01T00:00:00Z"}"#;
        assert!(ReleaseManifest::from_json(bad).is_err());

        let plan = build_plan(&manifest(), "/opt/nas_debian", "/tmp/nas.tar.gz", None);
        assert!(plan
            .steps
            .iter()
            .any(|step| step == &vec!["systemctl", "restart", "nas-web"]));
        assert!(!plan
            .steps
            .iter()
            .any(|step| step.iter().any(|arg| arg.contains("php"))));
    }

    #[test]
    fn parsea_respuesta_de_github_releases() {
        use super::RemoteReleaseInfo;

        let sample = r#"{
            "tag_name": "v1.0.0",
            "name": "Release v1.0.0",
            "body": "Novedades de la versión",
            "assets": [
                {
                    "name": "nas-1.0.0.json",
                    "browser_download_url": "https://github.com/ftole/nas_debian/releases/download/v1.0.0/nas-1.0.0.json"
                },
                {
                    "name": "nas_debian-1.0.0.tar.gz",
                    "browser_download_url": "https://github.com/ftole/nas_debian/releases/download/v1.0.0/nas_debian-1.0.0.tar.gz"
                }
            ]
        }"#;

        let info = RemoteReleaseInfo::parse_github_release(sample).expect("parse ok");
        assert_eq!(info.tag_name, "v1.0.0");
        assert_eq!(info.name.as_deref(), Some("Release v1.0.0"));
        assert_eq!(
            info.manifest_url.as_deref(),
            Some("https://github.com/ftole/nas_debian/releases/download/v1.0.0/nas-1.0.0.json")
        );
        assert_eq!(
            info.artifact_url.as_deref(),
            Some("https://github.com/ftole/nas_debian/releases/download/v1.0.0/nas_debian-1.0.0.tar.gz")
        );
    }
}
