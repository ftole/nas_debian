//! Manifiesto de release, verificación SHA-256 y firma Ed25519.

use std::path::Path;

use ed25519_dalek::{Signature, VerifyingKey};
use nas_core::{NasError, NasResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

/// Valida que un identificador de versión sea seguro y no contenga traversal de rutas.
pub fn is_valid_version(version: &str) -> bool {
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

    /// Verifica que el artefacto coincida con el SHA-256 declarado mediante lectura en streaming O(1).
    pub fn verify_artifact(&self, artifact: impl AsRef<Path>) -> NasResult<()> {
        use std::io::Read;

        let path = artifact.as_ref();
        let mut file = std::fs::File::open(path).map_err(|e| NasError::io(path, e))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(|e| NasError::io(path, e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
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
