use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hotkey {
    #[default]
    CtrlShiftO,
    CtrlAltO,
    CtrlShiftSpace,
}
impl Hotkey {
    pub fn label(self) -> &'static str {
        match self {
            Self::CtrlShiftO => "Ctrl+Shift+O",
            Self::CtrlAltO => "Ctrl+Alt+O",
            Self::CtrlShiftSpace => "Ctrl+Shift+Space",
        }
    }
    pub fn key(self) -> u32 {
        match self {
            Self::CtrlShiftSpace => 0x20,
            _ => u32::from(b'O'),
        }
    }
    pub fn modifier_bits(self) -> u32 {
        match self {
            Self::CtrlAltO => 0x0001 | 0x0002,
            _ => 0x0002 | 0x0004,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityMode {
    #[default]
    Fast,
    Balanced,
    Quality,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub quality_mode: QualityMode,
    pub quick_copy: bool,
    pub cpu_only: bool,
    pub hotkey: Hotkey,
}
impl Settings {
    pub fn path() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("ObjectLift/settings.json")
    }
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        serde_json::from_slice(&std::fs::read(path)?).context("Invalid ObjectLift settings JSON")
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}
