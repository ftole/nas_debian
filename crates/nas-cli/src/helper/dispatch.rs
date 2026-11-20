//! Ejecución de las acciones de `nas helper`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nas_core::{ApiResponse, NasError, NasResult};
use nas_engine::helpers::{accounts, backup, samba, share, storage, terminal};
use nas_exec::Runner;

use super::{AccountsAction, BackupManageAction, HelperAction, StorageApplyAction, TerminalAction};

fn read_stdin_line() -> String {
    use std::io::Read;
    let mut buffer = String::new();
    let _ = std::io::stdin().read_to_string(&mut buffer);
    buffer.trim_end_matches(['\r', '\n']).to_string()
}

fn trim_stdin_line(raw: &str) -> String {
    raw.strip_suffix("\r\n")
        .or_else(|| raw.strip_suffix('\n'))
        .unwrap_or(raw)
        .to_string()
}

fn read_stdin_single_line() -> String {
    use std::io::Read;
    let mut buffer = String::new();
    let _ = std::io::stdin()
        .lock()
        .take(8192)
        .read_to_string(&mut buffer);
    trim_stdin_line(&buffer)
}

#[cfg(unix)]
fn helper_needs_escape(action: &HelperAction) -> bool {
    match action {
        HelperAction::StorageApply { .. } => true,
        HelperAction::Accounts { action } => !matches!(action, AccountsAction::Status),
        HelperAction::BackupManage { action } => {
            matches!(
                action,
                BackupManageAction::Test { proto, .. } if proto == "cifs"
            ) || matches!(action, BackupManageAction::TestCifs { .. })
        }
        _ => false,
    }
}

#[cfg(unix)]
fn is_root() -> bool {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                let mut parts = rest.split_whitespace();
                let _real = parts.next();
                if let Some(euid) = parts.next() {
                    return euid == "0";
                }
            }
        }
    }
    false
}

/// Ejecuta una acción de helper.
pub fn run_helper(action: HelperAction, json: bool) -> NasResult<ExitCode> {
    #[cfg(unix)]
    {
        if helper_needs_escape(&action)
            && std::env::var("NAS_ESCAPED").as_deref() != Ok("1")
            && is_root()
            && Path::new("/run/systemd/system").exists()
            && Path::new("/run/systemd/private").exists()
        {
            let exe =
                std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/local/bin/nas"));
            let mut cmd = std::process::Command::new("systemd-run");
            cmd.arg("--quiet")
                .arg("--pipe")
                .arg("--wait")
                .arg("--collect")
                .arg("--setenv=NAS_ESCAPED=1")
                .arg(exe);
            cmd.args(std::env::args().skip(1));
            if let Ok(mut child) = cmd.spawn() {
                if let Ok(status) = child.wait() {
                    if let Some(code) = status.code() {
                        return Ok(ExitCode::from(code as u8));
                    }
                }
            }
        }
    }

    let runner = Runner::new();

    let output_msg = match action {
        HelperAction::SambaApply { staged_file } => {
            samba::apply_staged_samba(&runner, &staged_file)?
        }
        HelperAction::ShareDir {
            name,
            unit,
            pos_name,
            pos_unit,
        } => {
            let final_name = name.or(pos_name).ok_or_else(|| {
                NasError::Invalid("falta nombre de recurso (--name o posicional)".into())
            })?;
            let final_unit = unit.or(pos_unit);
            share::create_share_dir(&runner, &final_name, final_unit.as_deref())?
        }
        HelperAction::ShareAcl {
            path,
            principal,
            level,
            pos_path,
            pos_principal,
            pos_level,
        } => {
            let final_path = path.or(pos_path).ok_or_else(|| {
                NasError::Invalid("falta ruta de recurso (--path o posicional)".into())
            })?;
            let final_principal = principal.or(pos_principal).ok_or_else(|| {
                NasError::Invalid("falta principal (--principal o posicional)".into())
            })?;
            let final_level = level
                .or(pos_level)
                .ok_or_else(|| NasError::Invalid("falta nivel (--level o posicional)".into()))?;
            share::apply_share_acl(&runner, &final_path, &final_principal, &final_level)?
        }
        HelperAction::Accounts { action } => match action {
            AccountsAction::CreateUser {
                username,
                full_name,
                password,
                admin,
                web,
                samba,
                groups,
            } => {
                let pass = if password == "-" {
                    read_stdin_single_line()
                } else {
                    password
                };
                let group_vec: Vec<String> = groups
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                accounts::create_user(
                    &runner,
                    &accounts::CreateUserArgs {
                        username,
                        full_name,
                        password: pass,
                        admin: admin == "1",
                        web: web == "1",
                        samba: samba == "1",
                        groups: group_vec,
                    },
                )?
            }
            AccountsAction::UpdateUser {
                username,
                full_name,
                password,
                admin,
                web,
                samba,
                groups,
            } => {
                let pass = if password == "-" {
                    let s = read_stdin_single_line();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                } else if password == "--keep-password" || password.is_empty() {
                    None
                } else {
                    Some(password)
                };
                let group_vec: Vec<String> = groups
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                accounts::update_user(
                    &runner,
                    &accounts::UpdateUserArgs {
                        username,
                        full_name,
                        password: pass,
                        admin: admin == "1",
                        web: web == "1",
                        samba: samba == "1",
                        groups: group_vec,
                    },
                )?
            }
            AccountsAction::ToggleUser { username, enable } => {
                let is_enable = enable != "0";
                accounts::toggle_user(&runner, &username, is_enable)?
            }
            AccountsAction::DeleteUser { username } => accounts::delete_user(&runner, &username)?,
            AccountsAction::CreateGroup { group } => accounts::create_group(&runner, &group)?,
            AccountsAction::RenameGroup { old, new } => {
                accounts::rename_group(&runner, &old, &new)?
            }
            AccountsAction::DeleteGroup { group } => accounts::delete_group(&runner, &group)?,
            AccountsAction::AddGroupMember { username, group } => {
                accounts::add_group_member(&runner, &username, &group)?
            }
            AccountsAction::RemoveGroupMember { username, group } => {
                accounts::remove_group_member(&runner, &username, &group)?
            }
            AccountsAction::Status => accounts::accounts_status(&runner)?,
        },
        HelperAction::BackupManage { action } => match action {
            BackupManageAction::Install { id } => backup::install_backup_task(&runner, &id)?,
            BackupManageAction::InstallSpec => {
                let raw = read_stdin_line();
                let spec: nas_engine::backup_spec::BackupSpec = serde_json::from_str(&raw)
                    .map_err(|e| NasError::Invalid(format!("especificación JSON inválida: {e}")))?;
                backup::install_backup_spec(&runner, &spec)?
            }
            BackupManageAction::WriteCron {
                id,
                file,
                content,
                pos_file,
            } => {
                let (f, c) = if let Some(p) = pos_file {
                    if p.to_string_lossy() == "-" {
                        let text = read_stdin_line();
                        (None, Some(text))
                    } else {
                        (Some(p), None)
                    }
                } else {
                    (file, content)
                };
                backup::write_cron(&id, f.as_deref(), c.as_deref())?
            }
            BackupManageAction::DeleteCron { id } => backup::delete_cron(&id)?,
            BackupManageAction::WriteCred {
                id,
                file,
                content,
                pos_file,
            } => {
                let (f, c) = if let Some(p) = pos_file {
                    if p.to_string_lossy() == "-" {
                        let text = read_stdin_line();
                        (None, Some(text))
                    } else {
                        (Some(p), None)
                    }
                } else {
                    (file, content)
                };
                backup::write_cred(&id, f.as_deref(), c.as_deref())?
            }
            BackupManageAction::DeleteCred { id } => backup::delete_cred(&id)?,
            BackupManageAction::RunManual { id } => backup::run_backup_task(&runner, &id)?,
            BackupManageAction::Stop { id } => backup::stop_backup_task(&runner, &id)?,
            BackupManageAction::Logs { id, lines } => backup::backup_logs(&id, Some(lines))?,
            BackupManageAction::Delete { id } => backup::delete_backup_task(&id)?,
            BackupManageAction::TestCifs {
                host,
                share,
                user,
                password,
            } => {
                let pass = password.unwrap_or_else(read_stdin_line);
                backup::test_cifs_conn(&runner, &host, &share, &user, &pass)?
            }
            BackupManageAction::TestSsh {
                host,
                port,
                user,
                password,
            } => {
                let pass = password.unwrap_or_else(read_stdin_line);
                backup::test_ssh_conn(&runner, &host, port, &user, &pass)?
            }
            BackupManageAction::Test {
                proto,
                host,
                target,
                port,
                user,
            } => {
                let pass = read_stdin_line();
                match proto.as_str() {
                    "cifs" => backup::test_cifs_conn(&runner, &host, &target, &user, &pass)?,
                    "ssh" => backup::test_ssh_conn(&runner, &host, port, &user, &pass)?,
                    "local" => {
                        let p = Path::new(&target);
                        if !p.is_dir() || target.contains("..") {
                            return Err(NasError::Invalid(format!(
                                "ruta local inexistente o inválida: {target}"
                            )));
                        }
                        format!("ruta local verificada: {target}")
                    }
                    other => {
                        return Err(NasError::Invalid(format!(
                            "protocolo no soportado: {other}"
                        )))
                    }
                }
            }
            BackupManageAction::Freeze { id, tag } => {
                let info = backup::freeze_backup_task(&runner, &id, tag.as_deref())?;
                serde_json::to_string(&info)
                    .unwrap_or_else(|_| "Respaldo permanente creado".to_string())
            }
            BackupManageAction::ListPermanent { id } => {
                let list = backup::list_permanent_backups(id.as_deref())?;
                serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
            }
            BackupManageAction::Restore {
                id,
                snapshot,
                target,
            } => backup::restore_backup(&runner, &id, &snapshot, target.as_deref())?,
            BackupManageAction::DeletePermanent { id, name } => {
                backup::delete_permanent_backup(&runner, &id, &name)?
            }
        },
        HelperAction::StorageApply { action } => match action {
            StorageApplyAction::WipeDisk { confirm, device } => {
                storage::wipe_disk(&runner, &confirm, &device)?
            }
            StorageApplyAction::FormatSingle {
                confirm,
                device,
                fstype,
                unmount,
            } => storage::format_single(&runner, &confirm, &device, Some(&fstype), unmount == "1")?,
            StorageApplyAction::SetupLvm {
                confirm,
                disk,
                vg,
                lv,
                size,
                fstype,
                unmount,
            } => storage::setup_lvm(
                &runner,
                &confirm,
                &disk,
                &vg,
                &lv,
                Some(&size),
                Some(&fstype),
                unmount == "1",
            )?,
            StorageApplyAction::ExtendPool { confirm, device } => {
                storage::extend_pool(&runner, &confirm, &device)?
            }
            StorageApplyAction::ExtendPoolTarget {
                confirm,
                target,
                device,
                metadata_raid1,
            } => storage::extend_pool_target(
                &runner,
                &confirm,
                &target,
                &device,
                metadata_raid1 == "1",
            )?,
            StorageApplyAction::CreatePool {
                confirm,
                target,
                mode,
                devices,
                unmount,
            } => {
                let devs: Vec<String> = devices
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                storage::create_pool(&runner, &confirm, &target, &mode, &devs, unmount == "1")?
            }
            StorageApplyAction::SanitizeFstab { confirm } => {
                storage::sanitize_fstab(&runner, &confirm)?
            }
            StorageApplyAction::CreateSubvolume { confirm, name } => {
                storage::create_subvolume(&runner, &confirm, &name)?
            }
            StorageApplyAction::DeleteSubvolume { confirm, name } => {
                storage::delete_subvolume(&runner, &confirm, &name)?
            }
            StorageApplyAction::MountUnit {
                confirm,
                device,
                unit,
                fstype,
            } => storage::mount_unit(&runner, &confirm, &device, &unit, Some(&fstype))?,
            StorageApplyAction::UmountUnit { confirm, unit } => {
                storage::umount_unit(&runner, &confirm, &unit)?
            }
            StorageApplyAction::RemovePoolDevice {
                confirm,
                target,
                device,
            } => storage::remove_pool_device(&runner, &confirm, &target, &device)?,
            StorageApplyAction::ScrubUnitStatus { unit } => {
                storage::scrub_unit_status(&runner, &unit)?
            }
            StorageApplyAction::ScrubUnitStart { unit } => {
                storage::scrub_unit_start(&runner, &unit)?
            }
            StorageApplyAction::TrimUnit { unit } => storage::trim_unit(&runner, &unit)?,
            StorageApplyAction::SetWriteCache {
                confirm,
                device,
                mode,
            } => {
                let enabled = match mode.trim().to_ascii_lowercase().as_str() {
                    "on" | "1" | "true" => true,
                    "off" | "0" | "false" => false,
                    _ => {
                        return Err(NasError::Invalid(
                            "modo write-cache inválido (usa 'on' o 'off')".into(),
                        ))
                    }
                };
                storage::set_write_cache(&runner, &confirm, &device, enabled)?
            }
            StorageApplyAction::UpsConfigure {
                confirm,
                min_battery_pct,
            } => nas_engine::ups::configure_usb(&runner, &confirm, min_battery_pct)?,
            StorageApplyAction::UpsDisable { confirm } => {
                nas_engine::ups::disable(&runner, &confirm)?
            }
        },
        HelperAction::Terminal { user, action } => match action {
            TerminalAction::Start { cols, rows } => {
                terminal::start_terminal(&runner, &user, Some(cols), Some(rows))?
            }
            TerminalAction::Keys { input, enter } => {
                terminal::send_terminal_keys(&runner, &user, &input, enter)?;
                String::new()
            }
            TerminalAction::Raw { input } => {
                terminal::send_terminal_keys(&runner, &user, &input, false)?;
                String::new()
            }
            TerminalAction::RawEnter { input } => {
                terminal::send_terminal_keys(&runner, &user, &input, true)?;
                String::new()
            }
            TerminalAction::Key { key_name } => {
                terminal::send_terminal_special_key(&runner, &user, &key_name)?;
                String::new()
            }
            TerminalAction::Capture => terminal::capture_terminal(&runner, &user)?,
            TerminalAction::Resize { cols, rows } => {
                terminal::resize_terminal(&runner, &user, cols, rows)?;
                String::new()
            }
            TerminalAction::Kill => {
                terminal::kill_terminal(&runner, &user)?;
                String::new()
            }
            TerminalAction::Exists => {
                let exists = terminal::terminal_exists(&runner, &user)?;
                if !exists {
                    return Ok(ExitCode::FAILURE);
                }
                String::new()
            }
        },
        HelperAction::PurgeRecycle { days } => share::purge_recycle(&runner, days)?,
        HelperAction::BackupDedupMigration => run_backup_dedup_migration_helper(&runner)?,
    };

    if json {
        let resp = ApiResponse::ok(output_msg.clone(), "Operación de helper completada");
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else if !output_msg.is_empty() {
        println!("{output_msg}");
    }

    Ok(ExitCode::SUCCESS)
}

fn run_backup_dedup_migration_helper(runner: &Runner) -> NasResult<String> {
    #[cfg(unix)]
    if !is_root() {
        return Err(NasError::Denied(
            "se requieren privilegios de superusuario para ejecutar la migración de respaldos"
                .into(),
        ));
    }

    let runners = nas_engine::backup_dedup::repair_runners(runner, Path::new("/usr/local/bin"))?;
    let marker = Path::new("/var/lib/nas").join(nas_engine::backup_dedup::DEDUP_MIGRATION_MARKER);
    if marker.is_file() {
        return Ok(format!(
            "Runners actualizados ({}); migración de deduplicación ya completada",
            runners.updated.len()
        ));
    }

    let has_backup_dirs = Path::new("/srv/nas/BACKUPS_HISTORICOS").is_dir()
        || Path::new("/srv/nas/BACKUPS_PERMANENTES").is_dir()
        || Path::new("/srv/nas/unidades").is_dir();
    if !has_backup_dirs {
        return Ok(format!(
            "Runners actualizados ({}); sin directorios de respaldo para deduplicar",
            runners.updated.len()
        ));
    }

    if Path::new("/run/systemd/system").exists() && Path::new("/run/systemd/private").exists() {
        let args = nas_engine::backup_dedup::migration_systemd_run_args("/usr/local/bin/nas");
        let cmd = args[0].clone();
        let out = runner.run(
            &nas_exec::CommandSpec::new(&cmd)
                .args(&args[1..])
                .timeout(std::time::Duration::from_secs(30)),
        );
        return match out {
            Ok(res) if res.is_success() => Ok(format!(
                "Runners actualizados ({}); unidad nas-backup-dedup-migration iniciada en segundo plano",
                runners.updated.len()
            )),
            Ok(_) | Err(_) => Ok(format!(
                "Runners actualizados ({}); migración ya en ejecución o diferida",
                runners.updated.len()
            )),
        };
    }

    let report = nas_engine::backup_dedup::run_dedup_migration_if_needed(
        runner,
        &nas_engine::backup_dedup::MigrationOptions::default(),
    )?;
    let hardlinks = report.dedup.as_ref().map_or(0, |d| d.total_hardlinks);
    Ok(format!(
        "Runners actualizados ({}); deduplicación ejecutada ({hardlinks} hardlinks)",
        runners.updated.len()
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::{helper_needs_escape, trim_stdin_line, AccountsAction, HelperAction};

    #[test]
    fn cuentas_mutantes_escapan_y_status_no() {
        assert!(helper_needs_escape(&HelperAction::Accounts {
            action: AccountsAction::CreateUser {
                username: "operador1".into(),
                full_name: "Operador".into(),
                password: "-".into(),
                admin: "0".into(),
                web: "1".into(),
                samba: "0".into(),
                groups: String::new(),
            },
        }));
        assert!(helper_needs_escape(&HelperAction::Accounts {
            action: AccountsAction::ToggleUser {
                username: "operador1".into(),
                enable: "0".into(),
            },
        }));
        assert!(helper_needs_escape(&HelperAction::Accounts {
            action: AccountsAction::CreateGroup {
                group: "grp_ventas".into(),
            },
        }));
        assert!(!helper_needs_escape(&HelperAction::Accounts {
            action: AccountsAction::Status,
        }));
    }

    #[test]
    fn recorta_saltos_de_linea_en_stdin() {
        assert_eq!(trim_stdin_line("ClaveSegura1!\r\n"), "ClaveSegura1!");
        assert_eq!(trim_stdin_line("ClaveSegura1!\n"), "ClaveSegura1!");
        assert_eq!(
            trim_stdin_line("Clave123\nroot:hack\n"),
            "Clave123\nroot:hack"
        );
        assert_eq!(trim_stdin_line("Clave123\n\n"), "Clave123\n");
    }
}
