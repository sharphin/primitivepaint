//! 作品ファイルの読み書き。
//!
//! - `plim` … このアプリ独自の形式（`.plim`）。キャンバスモード・紙の素材・パレットの絵の具まで、
//!   作業を続けるのに必要なものをすべて残す。
//! - `png` / `jpeg` … 見た目どおりに合成した 1 枚の画像の書き出し。
//! - `psd` … Photoshop の形式（`.psd` / `.psb`）の読み込みと、`.psd` の書き出し（「複製を保存」）。
//!   CLIP STUDIO などとのやりとりに使う。読み込んだレイヤーは `layered` で紙に変換する
//!   （紙が持たない不透明度・合成モードなどは画素に焼き込む）。書き出すと紙はそれぞれラスターレイヤーになる。
//! - CLIP STUDIO 形式（`.clip`）の読み込み・書き出しは、ここに別のモジュールとして追加する予定。

mod blend;
pub mod jpeg;
pub mod layered;
pub mod plim;
pub mod png;
pub mod psd;
pub mod scaled;

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::Path;

use pp_core::{Document, Palette};

pub use scaled::OutputSize;

/// 見た目どおりに合成した 1 枚の画像として書き出す形式。形式を増やすときはここに足す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    pub const ALL: [ImageFormat; 2] = [ImageFormat::Png, ImageFormat::Jpeg];

    pub fn name(&self) -> &'static str {
        match self {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
        }
    }

    pub fn extension(&self) -> &'static str {
        self.extensions()[0]
    }

    /// 拡張子の一覧（先頭が書き出すときに付けるもの）。
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            ImageFormat::Png => &[png::EXTENSION],
            ImageFormat::Jpeg => &jpeg::EXTENSIONS,
        }
    }

    /// 書き出すときに品質（圧縮の強さ）を選ぶか。
    pub fn has_quality(&self) -> bool {
        matches!(self, ImageFormat::Jpeg)
    }

    /// `settings` の大きさ・品質で書き出す。
    pub fn export_to_path(&self, path: &Path, document: &Document, settings: ExportSettings) -> io::Result<()> {
        match self {
            ImageFormat::Png => png::export_to_path(path, document, settings.size, settings.dpi),
            ImageFormat::Jpeg => jpeg::export_to_path(path, document, settings.size, settings.quality, settings.dpi),
        }
    }
}

/// 画像を書き出すときの設定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportSettings {
    pub size: OutputSize,
    /// 品質（1〜100）。品質を選ぶ形式（`ImageFormat::has_quality`）だけが使う。
    pub quality: u8,
    /// 画像に記録する解像度（dpi）。
    pub dpi: u32,
}

/// 作品をそのままほかのソフトの形式で保存する「複製を保存」の形式。形式を増やすときはここに足す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyFormat {
    Psd,
}

impl CopyFormat {
    pub const ALL: [CopyFormat; 1] = [CopyFormat::Psd];

    /// メニューやファイルを選ぶ画面に出す名前。
    pub fn name(&self) -> &'static str {
        match self {
            CopyFormat::Psd => "Photoshop ドキュメント",
        }
    }

    pub fn extension(&self) -> &'static str {
        self.extensions()[0]
    }

    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            CopyFormat::Psd => &[psd::SAVE_EXTENSION],
        }
    }

    pub fn save_to_path(&self, path: &Path, document: &Document) -> io::Result<()> {
        match self {
            CopyFormat::Psd => psd::save_to_path(path, document),
        }
    }
}

/// `path` に書く。いったん別名で書き終えてから置き換えるので、途中で失敗しても元のファイルは壊れない。
pub(crate) fn write_replacing(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> io::Result<()> {
    let extension = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = path.with_extension(format!("{extension}.tmp"));
    let written = File::create(&temp).and_then(|file| {
        let mut out = BufWriter::new(file);
        write(&mut out)?;
        out.into_inner().map_err(|err| err.into_error())?.sync_all()
    });
    match written {
        Ok(()) => fs::rename(&temp, path),
        Err(err) => {
            let _ = fs::remove_file(&temp);
            Err(err)
        }
    }
}

/// ファイルから読み込んだ作業の状態。
pub struct Project {
    pub document: Document,
    pub palette: Palette,
}

/// 読み込みに失敗した理由。
#[derive(Debug)]
pub enum FileError {
    Io(io::Error),
    /// このアプリのファイルではない。
    NotOurFile,
    /// 新しい版のアプリで保存されたファイル。
    NewerVersion(u32),
    /// 中身が壊れている。どこがおかしいか。
    Corrupt(&'static str),
    /// 読めない種類のファイル（色の持ち方など）。利用者に見せる説明。
    Unsupported(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::Io(err) => write!(f, "ファイルを読み書きできません: {err}"),
            FileError::NotOurFile => write!(f, "primitivepaint のファイルではありません"),
            FileError::NewerVersion(v) => {
                write!(f, "新しい版（形式 {v}）で保存されたファイルです。アプリを更新してください")
            }
            FileError::Corrupt(what) => write!(f, "ファイルが壊れています（{what}）"),
            FileError::Unsupported(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for FileError {}

impl From<io::Error> for FileError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::UnexpectedEof => FileError::Corrupt("途中で終わっている"),
            _ => FileError::Io(err),
        }
    }
}
