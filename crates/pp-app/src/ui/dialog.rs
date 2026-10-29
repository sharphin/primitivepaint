//! 画面の中央に開くダイアログ（「新規」「書き出し」など）の共通のインターフェース。
//! 開いている間は、ほかのパネルもキャンバスも操作できない。

use pp_core::Rect;

use super::number_field::FieldKey;
use super::{Action, Pixmap};

/// ダイアログを操作した結果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DialogResult {
    /// まだ開いたまま（`true` なら描き直す）。
    Open(bool),
    Cancel,
    /// 決まった内容で閉じる。
    Done(Action),
}

/// ダイアログで受け付けるキー。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKey {
    /// 選んでいる欄へのキー。
    Field(FieldKey),
    /// 欄を行き来する。
    Tab,
}

/// ホイールの細かさ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelStep {
    Normal,
    /// Shift を押している。
    Fine,
    /// Ctrl を押している。
    Coarse,
}

pub trait Dialog {
    /// 画面 `screen` の中央に置く。
    fn place(&mut self, screen: Rect, scale: f32);

    fn rect(&self) -> Rect;

    /// 選んでいる欄へのキー入力。表示が変わったら true。
    fn key(&mut self, key: DialogKey) -> bool;

    /// `select` なら Shift を押していて、欄の中では今の位置から選択を広げる。
    fn press(&mut self, x: f32, y: f32, select: bool) -> DialogResult;

    /// マウスで欄の文字を選んでいる途中か。
    fn is_selecting(&self) -> bool;

    /// 押したまま動かした。表示が変わったら true。
    fn drag(&mut self, x: f32) -> bool;

    fn release(&mut self);

    /// ポインタの下の欄の値を、ホイールで `steps` 段（上が正）上下する。表示が変わったら true。
    fn wheel(&mut self, x: f32, y: f32, steps: i32, step: WheelStep) -> bool;

    /// Enter キーか決定のボタン。内容が正しくなければ開いたまま。
    fn confirm(&self) -> DialogResult;

    fn render(&self, pixmap: &mut Pixmap);
}

/// 画面 `screen` の中央に置いた、論理ピクセルで `width`×`height` の範囲。
pub fn centered(screen: Rect, scale: f32, width: f32, height: f32) -> Rect {
    let (w, h) = ((width * scale).round() as i32, (height * scale).round() as i32);
    let x0 = screen.x0 + (screen.width() as i32 - w).max(0) / 2;
    let y0 = screen.y0 + (screen.height() as i32 - h).max(0) / 2;
    Rect::new(x0, y0, x0 + w, y0 + h)
}
