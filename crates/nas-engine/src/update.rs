//! Verificación y planificación de actualizaciones firmadas y regresión A/B.
//!
//! El manifiesto declara versión, commit, artefacto, SHA-256 y firma Ed25519.
//! La firma cubre una carga canónica estable (`version|commit|artifact|sha256|published_at`),
//! de modo que cualquier alteración del manifiesto o del paquete se detecta antes
//! de tocar el sistema.

mod manifest;
mod plan;
mod remote;
mod rollback;

#[cfg(test)]
mod tests;

pub use manifest::{is_valid_version, public_key_from_secret, sign_manifest, ReleaseManifest};
pub use plan::{build_plan, UpdatePlan};
pub use remote::{
    download_and_verify_release, download_file, fetch_latest_release, RemoteReleaseInfo,
};
pub use rollback::{
    build_rollback_plan, list_installed_releases, parse_semver_key, InstalledRelease,
};
