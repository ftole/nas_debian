//! Gestión opcional Plug & Play de UPS/No-Break mediante NUT (`nut`).
//!
//! Mantiene cero sobrecarga y degradación segura cuando no existe UPS ni `nut`
//! instalado: únicamente auto-instala y configura `nut` bajo demanda del operador.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;

/// Estado operativo del subsistema UPS/NUT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpsStatus {
    /// Indica si el paquete `nut` / binario `upsc` está instalado.
    pub installed: bool,
    /// Indica si `nut` está configurado en modo activo (`standalone` / `netserver`).
    pub configured: bool,
    /// Indica si el UPS responde en línea (`OL`).
    pub online: bool,
    /// Indica si el UPS está alimentando el servidor con batería (`OB`).
    pub on_battery: bool,
    /// Indica si el UPS reporta batería baja (`LB`).
    pub low_battery: bool,
    /// Controlador NUT configurado (`usbhid-ups`, `blazer_usb`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    /// Modelo del UPS detectado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Nivel de carga de batería (0..=100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_charge: Option<u8>,
    /// Autonomía restante estimada en segundos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_seconds: Option<u64>,
    /// Carga eléctrica conectada en porcentaje.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_percent: Option<u8>,
    /// Umbral mínimo de batería (%) configurado para apagado limpio.
    pub min_battery_pct: u8,
    /// Mensaje legible del estado actual.
    pub detail: String,
}

/// Configuración generada para `/etc/nut/*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NutConfigs {
    /// Contenido de `/etc/nut/nut.conf`.
    pub nut_conf: String,
    /// Contenido de `/etc/nut/ups.conf`.
    pub ups_conf: String,
    /// Contenido de `/etc/nut/upsd.conf`.
    pub upsd_conf: String,
    /// Contenido de `/etc/nut/upsd.users`.
    pub upsd_users: String,
    /// Contenido de `/etc/nut/upsmon.conf`.
    pub upsmon_conf: String,
}

fn is_valid_driver(driver: &str) -> bool {
    !driver.is_empty()
        && driver.len() <= 48
        && driver
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Extrae el nombre de driver USB reportado por `nut-scanner -U`.
pub fn parse_nut_scanner_driver(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("driver") {
            let val = rest
                .trim_start_matches(|c: char| c == '=' || c.is_whitespace())
                .trim_matches('"')
                .trim();
            if is_valid_driver(val) {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Parsea la salida `clave: valor` de `upsc <ups>@localhost`.
pub fn parse_upsc_output(raw: &str, driver: Option<String>, min_battery_pct: u8) -> UpsStatus {
    let mut status_tokens = String::new();
    let mut model = None;
    let mut charge = None;
    let mut runtime = None;
    let mut load = None;

    for line in raw.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim();
        match key {
            "ups.status" => status_tokens = val.to_string(),
            "ups.model" | "device.model" if model.is_none() && !val.is_empty() => {
                model = Some(val.to_string());
            }
            "battery.charge" => {
                charge = val
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(|n| n.clamp(0.0, 100.0) as u8);
            }
            "battery.runtime" => {
                runtime = val
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(|n| n.max(0.0) as u64);
            }
            "ups.load" => {
                load = val
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(|n| n.clamp(0.0, 100.0) as u8);
            }
            _ => {}
        }
    }

    let tokens: Vec<&str> = status_tokens.split_whitespace().collect();
    let online = tokens.contains(&"OL");
    let on_battery = tokens.contains(&"OB");
    let low_battery = tokens.contains(&"LB");

    let detail = if status_tokens.is_empty() {
        "UPS configurado, sin telemetría reciente de upsc".to_string()
    } else if on_battery {
        format!(
            "En batería ({status_tokens}) · carga {}%",
            charge.map_or("--".to_string(), |c| c.to_string())
        )
    } else {
        format!(
            "En línea ({status_tokens}) · batería {}%",
            charge.map_or("--".to_string(), |c| c.to_string())
        )
    };

    UpsStatus {
        installed: true,
        configured: true,
        online,
        on_battery,
        low_battery,
        driver,
        model,
        battery_charge: charge,
        runtime_seconds: runtime,
        load_percent: load,
        min_battery_pct,
        detail,
    }
}

/// Genera los archivos de configuración `/etc/nut/*` para operación local segura (`standalone`).
pub fn render_nut_configs(driver: &str, upsmon_pass: &str, min_battery_pct: u8) -> NutConfigs {
    let min_pct = min_battery_pct.clamp(15, 80);
    NutConfigs {
        nut_conf: "# Generado por nas_debian\nMODE=standalone\n".to_string(),
        ups_conf: format!(
            "# Generado por nas_debian\n\
             [nasups]\n\
             \tdriver = {driver}\n\
             \tport = auto\n\
             \tdesc = \"UPS USB local protegido por nas_debian\"\n\
             \toverride.battery.charge.low = {min_pct}\n"
        ),
        upsd_conf: "# Generado por nas_debian\nLISTEN 127.0.0.1 3493\n".to_string(),
        upsd_users: format!(
            "# Generado por nas_debian\n\
             [upsmon]\n\
             \tpassword = {upsmon_pass}\n\
             \tupsmon primary\n"
        ),
        upsmon_conf: format!(
            "# Generado por nas_debian\n\
             MONITOR nasups@localhost 1 upsmon {upsmon_pass} primary\n\
             MINSUPPLIES 1\n\
             SHUTDOWNCMD \"/sbin/shutdown -h +0\"\n\
             POLLFREQ 5\n\
             POLLFREQALERT 5\n\
             HOSTSYNC 15\n\
             DEADTIME 15\n\
             POWERDOWNFLAG /etc/killpower\n\
             RBWARNTIME 43200\n\
             NOCOMMWARNTIME 300\n\
             FINALDELAY 5\n"
        ),
    }
}

fn read_configured_nut_meta() -> (bool, Option<String>, u8) {
    let Ok(nut_conf) = std::fs::read_to_string("/etc/nut/nut.conf") else {
        return (false, None, 35);
    };
    let active = nut_conf.lines().any(|l| {
        let t = l.trim();
        !t.starts_with('#') && (t == "MODE=standalone" || t == "MODE=netserver")
    });
    if !active {
        return (false, None, 35);
    }
    let mut driver = None;
    let mut min_pct = 35u8;
    if let Ok(ups_conf) = std::fs::read_to_string("/etc/nut/ups.conf") {
        for line in ups_conf.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("driver") {
                if let Some((_, val)) = rest.split_once('=') {
                    let clean = val.trim().trim_matches('"');
                    if is_valid_driver(clean) {
                        driver = Some(clean.to_string());
                    }
                }
            } else if let Some(rest) = t.strip_prefix("override.battery.charge.low") {
                if let Some((_, val)) = rest.split_once('=') {
                    if let Ok(n) = val.trim().parse::<u8>() {
                        min_pct = n.clamp(15, 80);
                    }
                }
            }
        }
    }
    (true, driver, min_pct)
}

/// Consulta el estado actual del UPS/NUT sin bloquear cuando no hay UPS configurado.
pub fn status(runner: &Runner) -> UpsStatus {
    let upsc_exists = Path::new("/usr/bin/upsc").exists() || Path::new("/bin/upsc").exists();
    let (configured, driver, min_pct) = read_configured_nut_meta();
    if !upsc_exists || !configured {
        return UpsStatus {
            installed: upsc_exists,
            configured,
            online: false,
            on_battery: false,
            low_battery: false,
            driver,
            model: None,
            battery_charge: None,
            runtime_seconds: None,
            load_percent: None,
            min_battery_pct: min_pct,
            detail: if upsc_exists {
                "NUT instalado pero sin UPS configurado (opcional — recomendado en producción)"
                    .to_string()
            } else {
                "Sin UPS configurado (módulo opcional bajo demanda)".to_string()
            },
        };
    }

    let Ok(out) = runner.run(
        &CommandSpec::new("upsc")
            .args(["nasups@localhost"])
            .timeout(Duration::from_secs(5)),
    ) else {
        return UpsStatus {
            installed: true,
            configured: true,
            online: false,
            on_battery: false,
            low_battery: false,
            driver,
            model: None,
            battery_charge: None,
            runtime_seconds: None,
            load_percent: None,
            min_battery_pct: min_pct,
            detail: "NUT activo pero no se pudo consultar upsc nasups@localhost".to_string(),
        };
    };

    if !out.is_success() {
        return UpsStatus {
            installed: true,
            configured: true,
            online: false,
            on_battery: false,
            low_battery: false,
            driver,
            model: None,
            battery_charge: None,
            runtime_seconds: None,
            load_percent: None,
            min_battery_pct: min_pct,
            detail: format!("Sin comunicación con UPS: {}", out.stderr.trim()),
        };
    }

    parse_upsc_output(&out.stdout, driver, min_pct)
}

fn generate_random_hex_secret() -> NasResult<String> {
    let mut buf = [0u8; 16];
    let mut f = std::fs::File::open("/dev/urandom")
        .map_err(|e| NasError::io(Path::new("/dev/urandom"), e))?;
    f.read_exact(&mut buf)
        .map_err(|e| NasError::io(Path::new("/dev/urandom"), e))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

fn write_nut_file(path: &str, content: &str, mode: u32) -> NasResult<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let p = Path::new(path);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(p)
        .map_err(|e| NasError::io(p, e))?;
    f.write_all(content.as_bytes())
        .map_err(|e| NasError::io(p, e))?;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
        .map_err(|e| NasError::io(p, e))?;
    Ok(())
}

/// Auto-instala `nut` si falta, detecta el UPS USB conectado y activa el apagado automático.
pub fn configure_usb(runner: &Runner, confirm: &str, min_battery_pct: u8) -> NasResult<String> {
    if confirm != "SI-CONFIGURAR" && confirm != "SI-FORMATEAR" && confirm != "SI-DESTRUIR-DATOS" {
        return Err(NasError::Denied(
            "confirmación requerida (SI-CONFIGURAR)".into(),
        ));
    }
    if !(15..=80).contains(&min_battery_pct) {
        return Err(NasError::Invalid(
            "el umbral de batería debe estar entre 15% y 80%".into(),
        ));
    }

    crate::deploy::deps::ensure_packages(runner, crate::deploy::deps::UPS_PACKAGES, false)?;

    let scan_out = runner.run(
        &CommandSpec::new("nut-scanner")
            .args(["-U", "-q"])
            .timeout(Duration::from_secs(20)),
    );
    let detected_driver = scan_out
        .ok()
        .filter(|o| o.is_success())
        .and_then(|o| parse_nut_scanner_driver(&o.stdout));

    let driver = detected_driver.ok_or_else(|| {
        NasError::NotFound(
            "no se detectó ningún UPS USB compatible mediante nut-scanner; verifica el cable USB"
                .into(),
        )
    })?;

    let secret = generate_random_hex_secret()?;
    let cfgs = render_nut_configs(&driver, &secret, min_battery_pct);

    std::fs::create_dir_all("/etc/nut").map_err(|e| NasError::io(Path::new("/etc/nut"), e))?;
    write_nut_file("/etc/nut/nut.conf", &cfgs.nut_conf, 0o644)?;
    write_nut_file("/etc/nut/ups.conf", &cfgs.ups_conf, 0o644)?;
    write_nut_file("/etc/nut/upsd.conf", &cfgs.upsd_conf, 0o640)?;
    write_nut_file("/etc/nut/upsd.users", &cfgs.upsd_users, 0o640)?;
    write_nut_file("/etc/nut/upsmon.conf", &cfgs.upsmon_conf, 0o640)?;

    let _ = runner.run(&CommandSpec::new("chown").args([
        "root:nut",
        "/etc/nut/upsd.conf",
        "/etc/nut/upsd.users",
        "/etc/nut/upsmon.conf",
    ]));

    let driver_unit_ok = runner
        .run(&CommandSpec::new("systemctl").args(["restart", "nut-driver@nasups.service"]))
        .is_ok_and(|o| o.is_success());
    if !driver_unit_ok {
        runner
            .run(
                &CommandSpec::new("upsdrvctl")
                    .args(["start"])
                    .timeout(Duration::from_secs(20)),
            )?
            .ensure_success()?;
    }
    for svc in ["nut-server", "nut-monitor"] {
        let _ = runner.run(&CommandSpec::new("systemctl").args(["enable", svc]));
        runner
            .run(&CommandSpec::new("systemctl").args(["restart", svc]))?
            .ensure_success()?;
    }

    Ok(format!(
        "UPS USB configurado con driver '{driver}' (apagado automático al {min_battery_pct}% de batería)"
    ))
}

/// Desactiva el monitoreo UPS (`nut-monitor` y `nut-server`) dejando `MODE=none`.
pub fn disable(runner: &Runner, confirm: &str) -> NasResult<String> {
    if confirm != "SI-CONFIGURAR" && confirm != "SI-FORMATEAR" && confirm != "SI-DESTRUIR-DATOS" {
        return Err(NasError::Denied("confirmación requerida".into()));
    }
    for svc in ["nut-monitor", "nut-server"] {
        let _ = runner.run(&CommandSpec::new("systemctl").args(["disable", "--now", svc]));
    }
    if Path::new("/etc/nut/nut.conf").exists() {
        write_nut_file(
            "/etc/nut/nut.conf",
            "# Generado por nas_debian\nMODE=none\n",
            0o644,
        )?;
    }
    Ok("monitoreo UPS deshabilitado".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_salida_de_upsc_y_nut_scanner() {
        let scanner = "\
[nutdev1]
\tdriver = \"usbhid-ups\"
\tport = \"auto\"
\tvendorid = \"051D\"
";
        assert_eq!(
            parse_nut_scanner_driver(scanner).as_deref(),
            Some("usbhid-ups")
        );

        let upsc = "\
battery.charge: 100
battery.runtime: 2450
ups.load: 18
ups.model: Back-UPS BX1100
ups.status: OL
";
        let st = parse_upsc_output(upsc, Some("usbhid-ups".into()), 35);
        assert!(st.online);
        assert!(!st.on_battery);
        assert_eq!(st.battery_charge, Some(100));
        assert_eq!(st.runtime_seconds, Some(2450));
        assert_eq!(st.load_percent, Some(18));
        assert_eq!(st.model.as_deref(), Some("Back-UPS BX1100"));
    }

    #[test]
    fn renderiza_configuracion_nut_sin_credenciales_fijas() {
        let cfgs = render_nut_configs("usbhid-ups", "secreto_rand_123", 40);
        assert!(cfgs.nut_conf.contains("MODE=standalone"));
        assert!(cfgs.ups_conf.contains("driver = usbhid-ups"));
        assert!(cfgs.ups_conf.contains("override.battery.charge.low = 40"));
        assert!(cfgs.upsd_conf.contains("LISTEN 127.0.0.1 3493"));
        assert!(cfgs
            .upsmon_conf
            .contains("SHUTDOWNCMD \"/sbin/shutdown -h +0\""));
        assert!(cfgs.upsd_users.contains("secreto_rand_123"));
    }

    #[test]
    fn parse_upsc_output_ignora_nan_e_infinito() {
        let raw = "battery.charge: NaN\nbattery.runtime: inf\nups.load: -inf\nups.status: OL\n";
        let st = parse_upsc_output(raw, Some("usbhid-ups".into()), 35);
        assert_eq!(st.battery_charge, None);
        assert_eq!(st.runtime_seconds, None);
        assert_eq!(st.load_percent, None);
    }
}
