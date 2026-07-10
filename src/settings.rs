use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub recent: Vec<String>,
    pub dark_theme: bool,
    pub word_wrap: bool,
    pub font_face: String,
    pub font_size: i32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            dark_theme: false,
            word_wrap: false,
            font_face: "Consolas".into(),
            font_size: 16,
        }
    }
}

impl AppSettings {
    pub fn settings_path() -> PathBuf {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("FastNotepad").join("settings.json")
    }

    pub fn load() -> Self {
        let path = Self::settings_path();
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::settings_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, text);
        }
    }

    pub fn add_recent(&mut self, path: &Path) {
        let s = path.to_string_lossy().to_string();
        self.recent.retain(|p| p != &s);
        self.recent.insert(0, s);
        self.recent.truncate(10);
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_defaults() {
        let s = AppSettings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.font_face, "Consolas");
        assert!(!back.dark_theme);
    }
}
