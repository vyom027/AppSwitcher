use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub animation: String,
    pub layout: String,
    pub duration: f64,
    pub dock_mag: f64,
    pub sensitivity: i32,
    pub accent: [u8; 3],
    pub warn_dismissed: bool,
    pub alt_tab: bool,
    pub blocklist: Vec<String>,
    pub hotkey_mod: String,
    pub hotkey_key: u32,
    pub pinch_enabled: bool,
    pub pinch_mods: Vec<u32>,
    pub pinch_vk: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            animation: "slide".to_string(),
            layout: "dock".to_string(),
            duration: 0.22,
            dock_mag: 1.95,
            sensitivity: 210,
            accent: [150, 205, 255],
            warn_dismissed: false,
            alt_tab: true,
            blocklist: Vec::new(),
            hotkey_mod: "alt".to_string(),
            hotkey_key: 0x09, // VK_TAB
            pinch_enabled: true,
            pinch_mods: vec![0x11], // VK_CONTROL
            pinch_vk: 0x09,         // VK_TAB
        }
    }
}

impl Settings {
    pub fn config_path() -> Option<PathBuf> {
        let appdata = std::env::var_os("APPDATA")?;
        let mut path = PathBuf::from(appdata);
        path.push("AppSwitcher");
        path.push("settings.json");
        Some(path)
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(data) = fs::read_to_string(&path) {
                    if let Ok(settings) = serde_json::from_str::<Settings>(&data) {
                        return settings;
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let data = serde_json::to_string_pretty(self)?;
            fs::write(path, data)?;
        }
        Ok(())
    }
}
