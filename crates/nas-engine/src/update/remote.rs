//! Consulta y descarga de releases remotas desde GitHub.

use std::path::Path;

use nas_core::{NasError, NasResult};
use serde::{Deserialize, Serialize};

use super::ReleaseManifest;

/// Información de release remota obtenida de GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteReleaseInfo {
    /// Tag de la release (ej. `v1.0.0`).
    pub tag_name: String,
    /// Título descriptivo de la release.
    pub name: Option<String>,
    /// Cuerpo o changelog de la release.
    pub body: Option<String>,
    /// URL de descarga directa del manifiesto `.json`.
    pub manifest_url: Option<String>,
    /// URL de descarga directa del paquete `.tar.gz`.
    pub artifact_url: Option<String>,
}

impl RemoteReleaseInfo {
    /// Parsea la respuesta JSON de la API de GitHub Releases.
    pub fn parse_github_release(json_text: &str) -> NasResult<Self> {
        let val: serde_json::Value = serde_json::from_str(json_text)
            .map_err(|e| NasError::Invalid(format!("respuesta de GitHub inválida: {e}")))?;

        let tag_name = val
            .get("tag_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NasError::Invalid("la respuesta no incluye tag_name".into()))?
            .to_string();

        let name = val.get("name").and_then(|v| v.as_str()).map(String::from);
        let body = val.get("body").and_then(|v| v.as_str()).map(String::from);

        let mut manifest_url = None;
        let mut artifact_url = None;

        if let Some(assets) = val.get("assets").and_then(|v| v.as_array()) {
            for asset in assets {
                let asset_name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let url = asset
                    .get("browser_download_url")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                if asset_name.ends_with(".json") {
                    manifest_url = url;
                } else if asset_name.ends_with(".tar.gz") {
                    artifact_url = url;
                }
            }
        }

        Ok(Self {
            tag_name,
            name,
            body,
            manifest_url,
            artifact_url,
        })
    }
}

/// Consulta la release más reciente desde la API de GitHub usando `curl`.
pub fn fetch_latest_release(runner: &nas_exec::Runner, repo: &str) -> NasResult<RemoteReleaseInfo> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let output = runner.run(
        &nas_exec::CommandSpec::new("curl")
            .args([
                "-sSL",
                "-H",
                "Accept: application/vnd.github.v3+json",
                "-H",
                "User-Agent: nas-updater",
                "--connect-timeout",
                "10",
                "--max-time",
                "30",
                &url,
            ])
            .timeout(std::time::Duration::from_secs(35)),
    )?;

    if !output.is_success() {
        return Err(NasError::Invalid(format!(
            "error al consultar releases de GitHub (código {}): {}",
            output.code, output.stderr
        )));
    }

    RemoteReleaseInfo::parse_github_release(&output.stdout)
}

/// Descarga un archivo por URL a una ruta local usando `curl`.
pub fn download_file(
    runner: &nas_exec::Runner,
    url: &str,
    target_path: impl AsRef<Path>,
) -> NasResult<()> {
    let target = target_path.as_ref();
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
    }

    let output = runner.run(
        &nas_exec::CommandSpec::new("curl")
            .args([
                "-sSL",
                "-f",
                "-H",
                "User-Agent: nas-updater",
                "--connect-timeout",
                "15",
                "--max-time",
                "300",
                "-o",
                &target.to_string_lossy(),
                url,
            ])
            .timeout(std::time::Duration::from_secs(310)),
    )?;

    if !output.is_success() {
        return Err(NasError::Invalid(format!(
            "error al descargar {url}: {}",
            output.stderr
        )));
    }

    Ok(())
}

/// Descarga el manifiesto y el artefacto de una release remota y verifica su integridad SHA-256 y firma Ed25519.
pub fn download_and_verify_release(
    runner: &nas_exec::Runner,
    release: &RemoteReleaseInfo,
    cache_dir: impl AsRef<Path>,
    public_key: &str,
) -> NasResult<(ReleaseManifest, std::path::PathBuf, std::path::PathBuf)> {
    let cache = cache_dir.as_ref();
    let manifest_url = release.manifest_url.as_deref().ok_or_else(|| {
        NasError::Invalid("la release de GitHub no incluye un manifiesto .json firmado".into())
    })?;
    let artifact_url = release.artifact_url.as_deref().ok_or_else(|| {
        NasError::Invalid(
            "la release de GitHub no incluye un paquete .tar.gz de distribución".into(),
        )
    })?;

    let tag_clean = release.tag_name.trim_start_matches('v');
    let manifest_path = cache.join(format!("nas-{tag_clean}.json"));
    let artifact_path = cache.join(format!("nas_debian-{tag_clean}.tar.gz"));

    download_file(runner, manifest_url, &manifest_path)?;
    download_file(runner, artifact_url, &artifact_path)?;

    let manifest = ReleaseManifest::load(&manifest_path)?;
    manifest.verify_artifact(&artifact_path)?;
    manifest.verify_signature(public_key)?;

    Ok((manifest, manifest_path, artifact_path))
}
