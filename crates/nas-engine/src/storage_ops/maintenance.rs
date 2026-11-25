use nas_exec::Runner;

use super::{confirm_ok, fail, helper, valid_device, valid_name};

/// Inicia `btrfs scrub start` de una unidad `/srv/nas/unidades/<id>`.
pub fn unit_scrub_start(runner: &Runner, unit_id: &str) -> Result<String, String> {
    if !valid_name(unit_id) {
        return Err(fail("identificador de unidad inválido"));
    }
    helper(runner, &["scrub-unit-start", unit_id])
}

/// Ejecuta `fstrim -v` en una unidad `/srv/nas/unidades/<id>`.
pub fn unit_trim(runner: &Runner, unit_id: &str) -> Result<String, String> {
    if !valid_name(unit_id) {
        return Err(fail("identificador de unidad inválido"));
    }
    helper(runner, &["trim-unit", unit_id])
}

/// Configura `hdparm -W0`/`-W1` y su regla udev persistente en un disco físico.
pub fn set_write_cache(
    runner: &Runner,
    device: &str,
    enabled: bool,
    confirm: &str,
) -> Result<String, String> {
    if !confirm_ok(confirm) && confirm != "SI-APLICAR" {
        return Err(fail("confirmación requerida (SI-APLICAR o SI-FORMATEAR)"));
    }
    let device = device.trim();
    if !valid_device(device) {
        return Err(fail("dispositivo inválido"));
    }
    let mode = if enabled { "on" } else { "off" };
    helper(runner, &["set-write-cache", confirm, device, mode])
}

/// Auto-instala `nut` si falta, detecta el UPS USB y habilita apagado automático.
pub fn ups_configure(
    runner: &Runner,
    min_battery_pct: u8,
    confirm: &str,
) -> Result<String, String> {
    if confirm != "SI-CONFIGURAR" && !confirm_ok(confirm) {
        return Err(fail("confirmación requerida (SI-CONFIGURAR)"));
    }
    if !(15..=80).contains(&min_battery_pct) {
        return Err(fail("el umbral de batería debe estar entre 15% y 80%"));
    }
    let pct_str = min_battery_pct.to_string();
    helper(runner, &["ups-configure", confirm, &pct_str])
}

/// Deshabilita el servicio NUT local de monitoreo UPS.
pub fn ups_disable(runner: &Runner, confirm: &str) -> Result<String, String> {
    if confirm != "SI-CONFIGURAR" && !confirm_ok(confirm) {
        return Err(fail("confirmación requerida (SI-CONFIGURAR)"));
    }
    helper(runner, &["ups-disable", confirm])
}
