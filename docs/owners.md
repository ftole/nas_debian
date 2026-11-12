# Owners por Crate / Módulo

| Crate / Módulo | Responsabilidad | Owner |
|---|---|---|
| `nas-web` (frontend + rutas) | Panel web, assets, paridad | `@frontend` |
| `nas-engine` (storage) | Discos, LVM, Btrfs, formatos | `@storage` |
| `nas-engine` (backups) | rsync, snapshots, retención | `@backups` |
| `nas-engine` (users/samba) | Usuarios, grupos, ACLs, AD | `@identity` |
| `nas-engine` (deploy) | Aprovisionamiento (ola B) | `@infra` |
| `nas-cli` | Comandos `nas` | `@cli` |
| `nas-tui` | Asistente de terminal | `@tui` |
| `nas-exec`, `nas-audit`, `nas-core` | Ejecución segura, auditoría, tipos | `@core` |
| Bash legacy (`src/core/*.sh`) | Despliegue legacy (ola B) | `@infra` |
| Paridad/CI (`scripts/`, `tests/parity/`) | Contrato PHP↔Rust | `@qa` |