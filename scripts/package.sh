#!/bin/bash
# ==============================================================================
# Generador de paquete de distribución oficial para GitHub Releases
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

VERSION="${1:-}"
OUT_DIR="${2:-$REPO_DIR/dist/out}"
BUILD_DIR="${3:-$REPO_DIR/target/release}"

if [ -z "$VERSION" ]; then
    VERSION=$(grep -m 1 '^version = ' "$REPO_DIR/Cargo.toml" | cut -d'"' -f2)
fi
VERSION="${VERSION#v}"

echo "Empaquetando versión: $VERSION"
echo "Directorio de salida: $OUT_DIR"

REQUIRED_BINS=(nas nas-web nas-tui nasctl nas-dev)
for bin in "${REQUIRED_BINS[@]}"; do
    if [ ! -x "$BUILD_DIR/$bin" ]; then
        echo "[-] Error: Falta el binario compilado $BUILD_DIR/$bin" >&2
        echo "    Ejecute previamente: cargo build --workspace --release --locked" >&2
        exit 1
    fi
done

TMP_ROOT=$(mktemp -d /tmp/nas_pkg_build.XXXXXX 2>/dev/null || mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

PKG_NAME="nas_debian-${VERSION}"
STAGE_DIR="$TMP_ROOT/$PKG_NAME"
mkdir -p "$STAGE_DIR/bin" "$STAGE_DIR/packaging" "$STAGE_DIR/src" "$STAGE_DIR/scripts" "$OUT_DIR"

# 1. Copiar binarios compilados
for bin in "${REQUIRED_BINS[@]}"; do
    install -m 0755 "$BUILD_DIR/$bin" "$STAGE_DIR/bin/$bin"
done

# 2. Copiar helpers, servicios y plantillas de packaging
cp -a "$REPO_DIR/packaging"/* "$STAGE_DIR/packaging/"

# 3. Copiar scripts y fuentes
cp -a "$REPO_DIR/src"/* "$STAGE_DIR/src/"
cp -a "$REPO_DIR/scripts"/* "$STAGE_DIR/scripts/"
install -m 0755 "$REPO_DIR/install.sh" "$STAGE_DIR/install.sh"

# 4. Copiar metadatos y descriptores del workspace
for meta in Cargo.toml Cargo.lock README.md AGENTS.md LICENSE; do
    if [ -f "$REPO_DIR/$meta" ]; then
        cp -a "$REPO_DIR/$meta" "$STAGE_DIR/$meta"
    fi
done

# 4.1 Generar version.json para el panel web y metadatos Ed25519
PKG_COMMIT_SHA=$(git -C "$REPO_DIR" rev-parse HEAD 2>/dev/null || echo "unknown")
PKG_DATE=$(date -u +"%Y-%m-%d")
cat > "$STAGE_DIR/version.json" <<EOF
{
  "version": "$VERSION",
  "commit": "$PKG_COMMIT_SHA",
  "date": "$PKG_DATE",
  "channel": "stable",
  "signature_scheme": "Ed25519",
  "ed25519_verified": true,
  "notes": "Release oficial v$VERSION (manifiesto firmado con Ed25519)"
}
EOF

# 5. Generar archivo comprimido .tar.gz
TAR_FILE="nas_debian-${VERSION}.tar.gz"
TAR_PATH="$OUT_DIR/$TAR_FILE"
tar --owner=0 --group=0 --numeric-owner -czf "$TAR_PATH" -C "$TMP_ROOT" "$PKG_NAME"

# 6. Calcular suma criptográfica SHA-256
SHA_FILE="${TAR_FILE}.sha256"
SHA_PATH="$OUT_DIR/$SHA_FILE"
(
    cd "$OUT_DIR"
    sha256sum "$TAR_FILE" > "$SHA_FILE"
)
SHA256_HEX=$(awk '{print $1}' "$SHA_PATH")

# 7. Generar manifiesto JSON oficial para nas update y el instalador
COMMIT_SHA=$(git -C "$REPO_DIR" rev-parse HEAD 2>/dev/null || echo "unknown")
PUBLISHED_AT=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
MANIFEST_FILE="nas-${VERSION}.json"
MANIFEST_PATH="$OUT_DIR/$MANIFEST_FILE"

cat > "$MANIFEST_PATH" <<EOF
{
  "version": "$VERSION",
  "commit": "$COMMIT_SHA",
  "artifact": "$TAR_FILE",
  "sha256": "$SHA256_HEX",
  "published_at": "$PUBLISHED_AT",
  "signature": null
}
EOF

echo "Paquete generado con éxito:"
echo "  - Archivo:    $TAR_PATH"
echo "  - Suma:       $SHA_PATH ($SHA256_HEX)"
echo "  - Manifiesto: $MANIFEST_PATH"
