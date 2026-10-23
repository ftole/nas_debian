//! Parser de `lsblk -J -o ...` (inventario de dispositivos de bloque).

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct RawLsblk {
    #[serde(default)]
    blockdevices: Vec<RawDevice>,
}

#[derive(Debug, Deserialize)]
struct RawDevice {
    #[serde(default)]
    name: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    rota: Option<RotaValue>,
    #[serde(default)]
    children: Vec<RawDevice>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RotaValue {
    Bool(bool),
    Num(u8),
    Text(String),
}

impl RotaValue {
    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::Num(value) => Some(*value != 0),
            Self::Text(value) => match value.trim().to_lowercase().as_str() {
                "1" | "true" | "yes" => Some(true),
                "0" | "false" | "no" => Some(false),
                _ => None,
            },
        }
    }
}

/// Dispositivo de bloque normalizado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockDevice {
    /// Nombre del dispositivo (sin `/dev/`).
    pub name: String,
    /// Tamaño legible.
    pub size: String,
    /// Tipo (`disk`, `part`, `lvm`, `crypt`, ...).
    pub kind: String,
    /// Punto de montaje, si está montado.
    pub mountpoint: Option<String>,
    /// Modelo del hardware, si aplica.
    pub model: Option<String>,
    /// Indica si el dispositivo es mecánico (`Some(true)`) o flash (`Some(false)`).
    pub rotational: Option<bool>,
    /// Dispositivos hijos (particiones, volúmenes).
    pub children: Vec<BlockDevice>,
}

impl BlockDevice {
    /// Recorre el árbol y devuelve todos los dispositivos en orden.
    pub fn flatten(&self) -> Vec<&Self> {
        let mut out = vec![self];
        for child in &self.children {
            out.extend(child.flatten());
        }
        out
    }

    /// Ruta completa del dispositivo.
    pub fn device_path(&self) -> String {
        format!("/dev/{}", self.name)
    }
}

/// Convierte la salida de `lsblk -J` en dispositivos normalizados.
pub fn parse(raw: &str) -> Result<Vec<BlockDevice>, serde_json::Error> {
    let parsed: RawLsblk = serde_json::from_str(raw)?;
    Ok(parsed.blockdevices.into_iter().map(convert).collect())
}

fn convert(raw: RawDevice) -> BlockDevice {
    BlockDevice {
        name: raw.name,
        size: raw.size.unwrap_or_else(|| "N/D".to_string()),
        kind: raw.kind.unwrap_or_else(|| "desconocido".to_string()),
        mountpoint: raw.mountpoint.filter(|m| !m.is_empty()),
        model: raw
            .model
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty()),
        rotational: raw.rota.and_then(|r| r.as_bool()),
        children: raw.children.into_iter().map(convert).collect(),
    }
}

/// Devuelve los discos físicos, excluyendo el disco del sistema operativo.
pub fn physical_disks<'a>(
    devices: &'a [BlockDevice],
    os_disk: Option<&str>,
) -> Vec<&'a BlockDevice> {
    devices
        .iter()
        .filter(|device| device.kind == "disk")
        .filter(|device| !device.name.starts_with("loop") && !device.name.starts_with("sr"))
        .filter(|device| os_disk.map_or(true, |os| !device.name.eq_ignore_ascii_case(os)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse, physical_disks};

    const LSBLK_JSON: &str = r#"{
      "blockdevices": [
        {"name":"sda","size":"1.8T","type":"disk","mountpoint":null,"model":"ST2000DM008-2FR102","rota":true,
         "children":[
           {"name":"sda1","size":"1.8T","type":"part","mountpoint":"/srv/nas","model":null,"rota":true,"children":[]}
         ]},
        {"name":"nvme0n1","size":"238.5G","type":"disk","mountpoint":null,"model":"Samsung SSD","rota":false,
         "children":[
           {"name":"nvme0n1p2","size":"237G","type":"part","mountpoint":"/","model":null,"rota":false,"children":[]}
         ]},
        {"name":"loop0","size":"64M","type":"loop","mountpoint":"/snap","model":null,"rota":false,"children":[]}
      ]
    }"#;

    #[test]
    fn parsea_arbol_de_dispositivos() {
        let devices = parse(LSBLK_JSON).expect("json válido");
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].name, "sda");
        assert_eq!(devices[0].rotational, Some(true));
        assert_eq!(devices[0].children.len(), 1);
        assert_eq!(
            devices[0].children[0].mountpoint.as_deref(),
            Some("/srv/nas")
        );
        assert_eq!(devices[1].rotational, Some(false));
    }

    #[test]
    fn excluye_loop_y_disco_del_so() {
        let devices = parse(LSBLK_JSON).expect("json válido");
        let disks = physical_disks(&devices, Some("nvme0n1"));
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].name, "sda");
        assert_eq!(disks[0].device_path(), "/dev/sda");
    }

    #[test]
    fn tolera_rota_como_texto() {
        let json = r#"{"blockdevices":[{"name":"sdb","size":"2T","type":"disk","rota":"1","children":[]}]}"#;
        let devices = parse(json).expect("json válido");
        assert_eq!(devices[0].rotational, Some(true));
    }

    #[test]
    fn json_invalido_es_error() {
        assert!(parse("no-json").is_err());
    }
}
