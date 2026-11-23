use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

pub const DEFAULT_ADDRESS: &str = "127.0.0.1:9090";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Portuguese,
    English,
}

#[derive(Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub soloist_address: String,
    pub soloist_executable: String,
    pub client_id: String,
    pub remember_soloist_key: bool,
    pub auto_start_soloist: bool,
    pub language: Language,
}

pub fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("settings.json"))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn config_dir() -> Option<PathBuf> {
    Some(PathBuf::from(env::var_os("LOCALAPPDATA")?).join("SpotifyRS"))
}

#[cfg(target_os = "linux")]
pub(crate) fn config_dir() -> Option<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".config"))
        })?;
    Some(base.join("spotifyrs"))
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut settings: Self = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        settings.normalize();
        settings
    }

    pub fn normalize(&mut self) {
        self.soloist_address = crate::soloist::endpoint(&self.soloist_address)
            .map(|address| address.to_string())
            .unwrap_or_else(|_| DEFAULT_ADDRESS.into());
        self.soloist_executable = self.soloist_executable.trim().trim_matches('"').to_owned();
        self.client_id = self.client_id.trim().to_owned();
        if !crate::auth::valid_client_id(&self.client_id) {
            self.client_id.clear();
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let pending = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = fs::write(&pending, serde_json::to_vec_pretty(self)?)
            .and_then(|()| fs::rename(&pending, path));
        if result.is_err() {
            let _ = fs::remove_file(&pending);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_preferences_across_loads_and_replaces_the_previous_file() {
        let path =
            env::temp_dir().join(format!("spotifyrs-settings-{}.json", rand::random::<u64>()));
        let mut settings = Settings::load(&path);
        assert_eq!(settings.soloist_address, DEFAULT_ADDRESS);
        settings.soloist_address = "127.0.0.1:9191".into();
        settings.soloist_executable = "C:/SpotifyRS/soloist.exe".into();
        settings.client_id = "0123456789abcdef0123456789abcdef".into();
        settings.remember_soloist_key = true;
        settings.auto_start_soloist = true;
        settings.language = Language::English;
        settings.save(&path).unwrap();
        assert!(Settings::load(&path) == settings);
        settings.auto_start_soloist = false;
        settings.save(&path).unwrap();
        assert!(Settings::load(&path) == settings);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn falls_back_for_invalid_address_or_client_id() {
        let mut settings = Settings {
            soloist_address: "192.168.1.5:9090".into(),
            client_id: "invalid".into(),
            ..Settings::default()
        };
        settings.normalize();
        assert_eq!(settings.soloist_address, DEFAULT_ADDRESS);
        assert!(settings.client_id.is_empty());
    }
}
