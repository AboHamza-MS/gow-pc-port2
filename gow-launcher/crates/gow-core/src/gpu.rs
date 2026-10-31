//! Graphics card list and selection.
//!
//! Windows chooses the GPU per program (Settings > Display > Graphics), so the launcher offers the two preferences
//! and writes `UserGpuPreferences`. Linux lists the PCI display controllers and steers the choice with PRIME variables.

use serde::Serialize;

use crate::launch::EnvironmentPlan;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsCard {
    /// "ahorro" / "rendimiento" on Windows (ids kept from the settings of the former launcher), PCI slot on Linux.
    pub id: String,
    /// "integrated", "dedicated" or empty (a single card).
    pub kind: String,
    pub display_name: String,
    pub manufacturer: String,
    pub driver: String,
    pub is_default: bool,
}

pub fn graphics_cards() -> Vec<GraphicsCard> {
    #[cfg(windows)]
    {
        vec![
            GraphicsCard { id: "ahorro".into(), kind: "integrated".into(), ..GraphicsCard::default() },
            GraphicsCard { id: "rendimiento".into(), kind: "dedicated".into(), ..GraphicsCard::default() },
        ]
    }
    #[cfg(not(windows))]
    {
        linux::list(std::path::Path::new("/sys/bus/pci/devices"))
    }
}

/// Adds the variables that make the runner use `id` on Linux. Windows does it through the registry
/// ([`apply_windows_preference`]); "auto" changes nothing.
pub fn linux_selection(cards: &[GraphicsCard], id: &str, plan: &mut EnvironmentPlan) {
    if id.is_empty() || id == "auto" {
        return;
    }
    let Some(card) = cards.iter().find(|card| card.id == id) else {
        return;
    };
    if card.driver == "nvidia" {
        // Proprietary NVIDIA driver on a hybrid laptop: PRIME render offload.
        plan.set.insert("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into());
        plan.set.insert("__GLX_VENDOR_LIBRARY_NAME".into(), "nvidia".into());
        plan.set.insert("__VK_LAYER_NV_optimus".into(), "NVIDIA_only".into());
    } else if !card.is_default {
        // Mesa: DRI_PRIME=pci-0000_03_00_0 chooses that card.
        plan.set.insert("DRI_PRIME".into(), format!("pci-{}", id.replace([':', '.'], "_")));
    }
}

/// Per-program preference of Windows 10/11 (what Settings > Graphics writes): "ahorro" = power saving,
/// "rendimiento" = high performance, anything else removes the entry.
#[cfg(windows)]
pub fn apply_windows_preference(id: &str, executable: &str) -> std::io::Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    let key = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(r"Software\Microsoft\DirectX\UserGpuPreferences", KEY_SET_VALUE)?
        .0;
    let name = executable.replace('/', "\\");
    match id {
        "ahorro" => key.set_value(&name, &"GpuPreference=1;"),
        "rendimiento" => key.set_value(&name, &"GpuPreference=2;"),
        _ => match key.delete_value(&name) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        },
    }
}

#[cfg(not(windows))]
mod linux {
    use std::fs;
    use std::path::Path;

    use super::GraphicsCard;

    fn read_text(path: &Path) -> String {
        fs::read_to_string(path).map(|text| text.trim().to_string()).unwrap_or_default()
    }

    /// Commercial name from pci.ids: "Rembrandt [Radeon 680M]" -> "Radeon 680M".
    fn pci_device_name(manufacturer: &str, device: &str) -> String {
        for base in ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids", "/usr/share/pci.ids"] {
            let Ok(text) = fs::read_to_string(base) else {
                continue;
            };
            let mut inside_vendor = false;
            for line in text.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if !line.starts_with('\t') {
                    if inside_vendor {
                        break;
                    }
                    inside_vendor = line.starts_with(&format!("{manufacturer}  "));
                    continue;
                }
                if inside_vendor && !line.starts_with("\t\t") && line.get(1..5) == Some(device) {
                    let name = line.get(7..).unwrap_or("").trim();
                    return match (name.find('['), name.rfind(']')) {
                        (Some(open), Some(close)) if close > open => name[open + 1..close].to_string(),
                        _ => name.to_string(),
                    };
                }
            }
        }
        String::new()
    }

    fn vendor_name(id: &str) -> String {
        match id {
            "10de" => "NVIDIA".into(),
            "1002" => "AMD".into(),
            "8086" => "Intel".into(),
            other => other.to_uppercase(),
        }
    }

    pub fn list(devices: &Path) -> Vec<GraphicsCard> {
        let mut slots: Vec<_> = fs::read_dir(devices).into_iter().flatten().filter_map(Result::ok).collect();
        slots.sort_by_key(|entry| entry.file_name());
        let mut cards = Vec::new();
        for entry in slots {
            let base = entry.path();
            let class = read_text(&base.join("class"));
            // 0x0300xx = VGA, 0x0302xx = 3D (the NVIDIA of hybrid laptops), 0x0380xx = another display controller.
            if !["0x0300", "0x0302", "0x0380"].iter().any(|prefix| class.starts_with(prefix)) {
                continue;
            }
            let manufacturer = read_text(&base.join("vendor")).get(2..).unwrap_or("").to_string();
            let device = read_text(&base.join("device")).get(2..).unwrap_or("").to_string();
            let driver = fs::read_link(base.join("driver"))
                .ok()
                .and_then(|target| target.file_name().map(|name| name.to_string_lossy().into_owned()))
                .unwrap_or_default();
            let mut name = pci_device_name(&manufacturer, &device);
            if name.is_empty() {
                name = format!("{manufacturer}:{device}");
            }
            cards.push(GraphicsCard {
                id: entry.file_name().to_string_lossy().into_owned(),
                display_name: name,
                manufacturer: vendor_name(&manufacturer),
                driver,
                is_default: read_text(&base.join("boot_vga")) == "1",
                kind: String::new(),
            });
        }
        // Integrated: the one the platform boots with (boot_vga) unless it is NVIDIA; the rest are dedicated.
        let single = cards.len() == 1;
        for card in &mut cards {
            card.kind = if single {
                String::new()
            } else if card.is_default && card.manufacturer != "NVIDIA" {
                "integrated".into()
            } else {
                "dedicated".into()
            };
        }
        cards
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, driver: &str, is_default: bool) -> GraphicsCard {
        GraphicsCard { id: id.into(), driver: driver.into(), is_default, ..GraphicsCard::default() }
    }

    #[test]
    fn nvidia_uses_prime_render_offload() {
        let cards = [card("0000:01:00.0", "nvidia", false)];
        let mut plan = EnvironmentPlan::default();
        linux_selection(&cards, "0000:01:00.0", &mut plan);
        assert_eq!(plan.set["__NV_PRIME_RENDER_OFFLOAD"], "1");
        assert_eq!(plan.set["__VK_LAYER_NV_optimus"], "NVIDIA_only");
    }

    #[test]
    fn mesa_non_default_card_uses_dri_prime() {
        let cards = [card("0000:03:00.0", "amdgpu", false), card("0000:00:02.0", "i915", true)];
        let mut plan = EnvironmentPlan::default();
        linux_selection(&cards, "0000:03:00.0", &mut plan);
        assert_eq!(plan.set["DRI_PRIME"], "pci-0000_03_00_0");
        let mut plan = EnvironmentPlan::default();
        linux_selection(&cards, "0000:00:02.0", &mut plan);
        assert!(plan.set.is_empty());
    }

    #[test]
    fn automatic_and_unknown_cards_change_nothing() {
        let cards = [card("a", "nvidia", false)];
        for id in ["", "auto", "missing"] {
            let mut plan = EnvironmentPlan::default();
            linux_selection(&cards, id, &mut plan);
            assert!(plan.set.is_empty());
        }
    }
}
