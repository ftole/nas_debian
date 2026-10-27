//! Usuarios y grupos locales del servidor.
//!
//! Lectura de `/etc/passwd` y `/etc/group` para inventario y validaciones,
//! sin dependencias externas ni llamadas a `getent`.

use serde::Serialize;

/// Usuario local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserEntry {
    /// Nombre de usuario.
    pub name: String,
    /// UID.
    pub uid: u32,
    /// GID principal.
    pub gid: u32,
    /// Nombre completo (GECOS).
    pub full_name: String,
    /// Directorio home.
    pub home: String,
    /// Shell de inicio.
    pub shell: String,
    /// Indica si es una cuenta de sistema.
    pub system: bool,
}

/// Grupo local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupEntry {
    /// Nombre del grupo.
    pub name: String,
    /// GID.
    pub gid: u32,
    /// Miembros secundarios.
    pub members: Vec<String>,
}

const RESERVED_SYSTEM_USERS: &[&str] = &[
    "root",
    "daemon",
    "bin",
    "sys",
    "sync",
    "nobody",
    "nogroup",
    "www-data",
    "nas-web",
    "sshd",
    "messagebus",
];

fn is_system_user(name: &str, uid: u32) -> bool {
    !(1000..60000).contains(&uid)
        || RESERVED_SYSTEM_USERS.contains(&name)
        || name.starts_with("systemd-")
}

/// Indica si una cuenta pertenece al sistema operativo y no puede modificarse desde el panel.
pub fn is_protected_system_account(name: &str, uid: Option<u32>) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if RESERVED_SYSTEM_USERS.contains(&lower.as_str()) || lower.starts_with("systemd-") {
        return true;
    }
    if let Some(uid) = uid {
        if !(1000..60000).contains(&uid) {
            return true;
        }
    }
    false
}

/// Interpreta `/etc/passwd`.
pub fn parse_passwd(text: &str) -> Vec<UserEntry> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() < 7 {
                return None;
            }
            let name = fields[0].to_string();
            let uid: u32 = fields[2].parse().ok()?;
            let gid: u32 = fields[3].parse().ok()?;
            let shell = fields[6].to_string();
            let system = is_system_user(&name, uid);
            Some(UserEntry {
                name,
                uid,
                gid,
                full_name: fields[4].to_string(),
                home: fields[5].to_string(),
                system,
                shell,
            })
        })
        .collect()
}

/// Interpreta `/etc/group`.
pub fn parse_group(text: &str) -> Vec<GroupEntry> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() < 4 {
                return None;
            }
            let gid: u32 = fields[2].parse().ok()?;
            let members = fields[3]
                .split(',')
                .map(str::trim)
                .filter(|member| !member.is_empty())
                .map(str::to_string)
                .collect();
            Some(GroupEntry {
                name: fields[0].to_string(),
                gid,
                members,
            })
        })
        .collect()
}

/// Carga usuarios y grupos locales; en plataformas no Unix devuelve listas vacías.
pub fn load_local() -> (Vec<UserEntry>, Vec<GroupEntry>) {
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    let group = std::fs::read_to_string("/etc/group").unwrap_or_default();
    (parse_passwd(&passwd), parse_group(&group))
}

/// Filtra los usuarios administrables por el panel (no sistema).
pub fn managed_users(users: &[UserEntry]) -> Vec<&UserEntry> {
    users.iter().filter(|user| !user.system).collect()
}

/// Filtra los grupos corporativos (`grp_*`).
pub fn corporate_groups(groups: &[GroupEntry]) -> Vec<&GroupEntry> {
    groups
        .iter()
        .filter(|group| group.name.starts_with("grp_"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        corporate_groups, is_protected_system_account, managed_users, parse_group, parse_passwd,
    };

    const PASSWD: &str = "\
root:x:0:0:root:/root:/bin/bash
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin
sistemas:x:1000:1000:Sistemas:/home/sistemas:/bin/bash
empleado1:x:1001:1000::/home/empleado1:/bin/bash
";

    const GROUP: &str = "\
root:x:0:
grp_samba:x:1001:sistemas,empleado1
grp_web:x:1002:sistemas
users:x:100:empleado1
";

    #[test]
    fn interpreta_passwd_y_marca_cuentas_de_sistema() {
        let users = parse_passwd(PASSWD);
        assert_eq!(users.len(), 4);
        assert!(users[0].system);
        assert!(users[1].system);
        assert!(!users[2].system);
        assert_eq!(users[2].home, "/home/sistemas");
        assert_eq!(users[2].full_name, "Sistemas");
        assert_eq!(managed_users(&users).len(), 2);
    }

    #[test]
    fn incluye_cuentas_samba_con_nologin_y_excluye_nobody() {
        let extra = "\
asesor:x:1002:1002:Asesor Comercial:/home/asesor:/usr/sbin/nologin
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
nas-web:x:998:998:Panel Web:/var/lib/nas:/usr/sbin/nologin
";
        let users = parse_passwd(extra);
        let managed = managed_users(&users);
        assert_eq!(managed.len(), 1);
        assert_eq!(managed[0].name, "asesor");
        assert!(is_protected_system_account("root", Some(0)));
        assert!(is_protected_system_account("nas-web", Some(1005)));
        assert!(is_protected_system_account("nobody", Some(65534)));
        assert!(!is_protected_system_account("asesor", Some(1002)));
    }

    #[test]
    fn interpreta_grupos_y_miembros() {
        let groups = parse_group(GROUP);
        assert_eq!(groups.len(), 4);
        let samba = groups
            .iter()
            .find(|group| group.name == "grp_samba")
            .expect("grp_samba");
        assert_eq!(samba.gid, 1001);
        assert_eq!(samba.members, vec!["sistemas", "empleado1"]);
        assert_eq!(corporate_groups(&groups).len(), 2);
    }

    #[test]
    fn ignora_lineas_invalidas() {
        assert!(parse_passwd("invalida\n\n# comentario\n").is_empty());
        assert!(parse_group("invalida\n").is_empty());
    }
}
