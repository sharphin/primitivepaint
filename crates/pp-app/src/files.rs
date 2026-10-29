//! ファイルを選ぶダイアログと確認・エラーの表示。Windows ではエクスプローラーと同じ OS 標準のダイアログを使う。
//! それ以外の OS は開発中の動作確認用で、ダイアログを出さずに作業フォルダの決まった名前を使う。

use std::path::PathBuf;

/// 保存しないまま閉じたり開いたりするときに、どうするか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

pub struct FileFilter {
    pub name: &'static str,
    /// 拡張子。保存するときは先頭のものを付ける。
    pub extensions: &'static [&'static str],
}

impl FileFilter {
    pub fn extension(&self) -> &'static str {
        self.extensions[0]
    }
}

pub const WORK_FILTER: FileFilter =
    FileFilter { name: "primitivepaint の作品", extensions: &[pp_file::plim::EXTENSION] };

/// 読み込めるほかのソフトの形式。開くと新しい作品になり、保存は primitivepaint の形式になる。
pub const IMPORT_FILTER: FileFilter =
    FileFilter { name: "Photoshop / CLIP STUDIO の PSD", extensions: &pp_file::psd::EXTENSIONS };

#[cfg(windows)]
mod native {
    use super::*;
    use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

    /// 開くファイルを選ぶ。最初の欄は、`filters` のどれでも選べる「開けるファイルすべて」。
    pub fn pick_open(filters: &[&FileFilter]) -> Option<PathBuf> {
        let all: Vec<&str> = filters.iter().flat_map(|f| f.extensions.iter().copied()).collect();
        let mut dialog = FileDialog::new().set_title("開く").add_filter("開けるファイルすべて", &all);
        for filter in filters {
            dialog = dialog.add_filter(filter.name, filter.extensions);
        }
        dialog.pick_file()
    }

    pub fn pick_save(title: &str, filter: &FileFilter, file_name: &str) -> Option<PathBuf> {
        FileDialog::new()
            .set_title(title)
            .add_filter(filter.name, filter.extensions)
            .set_file_name(file_name)
            .save_file()
    }

    pub fn ask_unsaved(file_name: &str) -> UnsavedChoice {
        let result = MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("primitivepaint")
            .set_description(format!("「{file_name}」への変更を保存しますか？"))
            .set_buttons(MessageButtons::YesNoCancelCustom(
                "保存する".to_string(),
                "保存しない".to_string(),
                "キャンセル".to_string(),
            ))
            .show();
        match result {
            MessageDialogResult::Custom(label) if label == "保存する" => UnsavedChoice::Save,
            MessageDialogResult::Custom(label) if label == "保存しない" => UnsavedChoice::Discard,
            MessageDialogResult::Yes => UnsavedChoice::Save,
            MessageDialogResult::No => UnsavedChoice::Discard,
            _ => UnsavedChoice::Cancel,
        }
    }

    pub fn show_info(message: &str) {
        MessageDialog::new()
            .set_level(MessageLevel::Info)
            .set_title("primitivepaint")
            .set_description(message)
            .set_buttons(MessageButtons::Ok)
            .show();
    }

    pub fn show_error(message: &str) {
        MessageDialog::new()
            .set_level(MessageLevel::Error)
            .set_title("primitivepaint")
            .set_description(message)
            .set_buttons(MessageButtons::Ok)
            .show();
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;

    pub fn pick_open(filters: &[&FileFilter]) -> Option<PathBuf> {
        let filter = filters[0];
        log::info!("{}を作業フォルダの untitled.{} から開きます", filter.name, filter.extension());
        Some(PathBuf::from(format!("untitled.{}", filter.extension())))
    }

    pub fn pick_save(_title: &str, filter: &FileFilter, file_name: &str) -> Option<PathBuf> {
        let path = PathBuf::from(file_name);
        Some(path.with_extension(filter.extension()))
    }

    pub fn ask_unsaved(_file_name: &str) -> UnsavedChoice {
        UnsavedChoice::Discard
    }

    pub fn show_info(message: &str) {
        log::info!("{message}");
    }

    pub fn show_error(message: &str) {
        log::error!("{message}");
    }
}

pub use native::{ask_unsaved, pick_open, pick_save, show_error, show_info};
