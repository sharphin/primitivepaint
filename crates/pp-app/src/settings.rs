//! アプリの設定（配色など）。次に起動したときにも同じ設定で始められるよう、ファイルに残す。
//!
//! 置き場所は Windows では `%APPDATA%\primitivepaint\settings.txt`、それ以外では
//! `$XDG_CONFIG_HOME`（なければ `~/.config`）の下。中身は `キー=値` を 1 行ずつ並べた文字列。

use std::io;
use std::path::PathBuf;

use crate::ui::theme::UiTheme;

const APP_DIR: &str = "primitivepaint";
const FILE_NAME: &str = "settings.txt";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub theme: UiTheme,
}

impl Settings {
    /// 設定ファイルを読む。ファイルがない・読めないときは初期設定にする。
    pub fn load() -> Settings {
        let Some(path) = settings_path() else { return Settings::default() };
        match std::fs::read_to_string(&path) {
            Ok(text) => Settings::parse(&text),
            Err(err) => {
                if err.kind() != io::ErrorKind::NotFound {
                    log::warn!("設定を読めません（{}）: {err}", path.display());
                }
                Settings::default()
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let Some(path) = settings_path() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }

    /// 知らないキーや値は読み飛ばし、その項目は初期値のままにする。
    fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            if let ("theme", Some(theme)) = (key.trim(), UiTheme::from_key(value.trim())) {
                settings.theme = theme;
            }
        }
        settings
    }

    fn to_text(self) -> String {
        format!("theme={}\n", self.theme.key())
    }
}

fn settings_path() -> Option<PathBuf> {
    let env_dir = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env_dir("APPDATA")
    } else {
        env_dir("XDG_CONFIG_HOME").or_else(|| env_dir("HOME").map(|home| home.join(".config")))
    }?;
    Some(base.join(APP_DIR).join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips() {
        let settings = Settings { theme: UiTheme::Dark };
        assert_eq!(Settings::parse(&settings.to_text()), settings);
    }

    #[test]
    fn unknown_lines_and_values_fall_back_to_defaults() {
        assert_eq!(Settings::parse("theme=purple\nzoom=3\n# memo"), Settings::default());
        assert_eq!(Settings::parse(" theme = dark \r\n").theme, UiTheme::Dark);
    }
}
