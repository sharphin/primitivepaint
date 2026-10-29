//! タブ 1 つ分の作品。絵・表示位置・保存先・パレットをまとめて持つ。

use std::path::{Path, PathBuf};

use pp_core::{Document, Palette};

use crate::view::ViewTransform;

/// タブで開いている作品。
pub struct Work {
    pub doc: Document,
    pub view: ViewTransform,
    /// 表に出ていない間のパレット。表に出ている間は混色パネルが持つので `None`。
    parked_palette: Option<Palette>,
    /// 保存先。まだ一度も保存していなければ `None`。
    file: Option<PathBuf>,
    /// 保存していない名前（「無題 2」の 2）。
    untitled_number: usize,
    /// ほかの形式から読み込んだときの元のファイル名（拡張子なし）。保存するまでの名前に使う。
    source_stem: Option<String>,
    /// 最後に保存して（または開いて）から、絵・紙・パレットを変えたか。
    modified: bool,
}

impl Work {
    pub fn new(doc: Document, palette: Palette, untitled_number: usize) -> Self {
        Work {
            doc,
            view: ViewTransform { zoom: 1.0, rotation: 0.0, pan: [0.0, 0.0] },
            parked_palette: Some(palette),
            file: None,
            untitled_number,
            source_stem: None,
            modified: false,
        }
    }

    /// ほかの形式から読み込んだ作品。保存先はまだなく、保存していない変更がある扱いにする。
    pub fn imported(doc: Document, palette: Palette, source_stem: String) -> Self {
        Work { source_stem: Some(source_stem), modified: true, ..Work::new(doc, palette, 0) }
    }

    /// ファイルから開いた作品。
    pub fn opened(doc: Document, palette: Palette, path: PathBuf) -> Self {
        Work { file: Some(path), ..Work::new(doc, palette, 0) }
    }

    /// タブやタイトルに出す名前（拡張子付き）。
    pub fn name(&self) -> String {
        match self.file.as_ref().and_then(|path| path.file_name()) {
            Some(name) => name.to_string_lossy().into_owned(),
            None => self.untitled_name(),
        }
    }

    /// 保存ダイアログに最初に入れておく名前（拡張子なし）。
    pub fn stem(&self) -> String {
        match self.file.as_ref().and_then(|path| path.file_stem()) {
            Some(stem) => stem.to_string_lossy().into_owned(),
            None => self.untitled_name(),
        }
    }

    fn untitled_name(&self) -> String {
        if let Some(stem) = &self.source_stem {
            return stem.clone();
        }
        match self.untitled_number {
            0 | 1 => "無題".to_string(),
            n => format!("無題 {n}"),
        }
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn mark_modified(&mut self) {
        self.modified = true;
    }

    /// 保存できたら呼ぶ。
    pub fn mark_saved(&mut self, path: PathBuf) {
        self.file = Some(path);
        self.modified = false;
    }

    /// まだ何も描いておらず、保存先もない（開いたファイルで置き換えてよい）か。
    pub fn is_blank(&self) -> bool {
        self.file.is_none() && !self.modified
    }

    /// 表に出すときに、しまっておいたパレットを取り出す。
    pub fn unpark_palette(&mut self) -> Option<Palette> {
        self.parked_palette.take()
    }

    /// 裏に回すときに、混色パネルから外したパレットをしまう。
    pub fn park_palette(&mut self, palette: Palette) {
        self.parked_palette = Some(palette);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pp_core::CanvasMode;

    fn work(number: usize) -> Work {
        Work::new(Document::new(8, 8, CanvasMode::Paper), Palette::new(2, 2), number)
    }

    #[test]
    fn untitled_works_are_numbered_from_the_second() {
        assert_eq!(work(1).name(), "無題");
        assert_eq!(work(3).name(), "無題 3");
    }

    #[test]
    fn saving_names_the_work_after_its_file() {
        let mut w = work(2);
        w.mark_modified();
        assert!(!w.is_blank());
        w.mark_saved(PathBuf::from("dir/花.plim"));
        assert_eq!(w.name(), "花.plim");
        assert_eq!(w.stem(), "花");
        assert!(!w.is_modified());
    }
}
