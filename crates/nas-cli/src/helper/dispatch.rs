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
        let needs_escape = match &action {
            HelperAction::StorageApply { .. } => true,
            HelperAction::BackupManage { action } => {
                matches!(
                    action,
                    BackupManageAction::Test { proto, .. } if proto == "cifs"
                ) || matches!(action, BackupManageAction::TestCifs { .. })
            }
            _ => false,
        };

        if needs_escape
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
                    read_stdin_line()
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
                    let s = read_stdin_line();
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
    };

    if json {
        let resp = ApiResponse::ok(output_msg.clone(), "Operación de helper completada");
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else if !output_msg.is_empty() {
        println!("{output_msg}");
    }

    Ok(ExitCode::SUCCESS)
}
