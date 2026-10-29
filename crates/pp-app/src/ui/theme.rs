//! パネルの配色。CLIP STUDIO PAINT と同じく、明るい灰色（ライト）と暗い灰色（ダーク）を選べる。

use std::sync::atomic::{AtomicU8, Ordering};

use pp_core::{Color, Rgba8};

const fn gray(v: u8) -> Rgba8 {
    Rgba8 { r: v, g: v, b: v, a: 255 }
}

const fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
    Rgba8 { r, g, b, a: 255 }
}

/// 画面全体の配色の種類。「オプション」メニューで切り替える。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiTheme {
    #[default]
    Light,
    Dark,
}

impl UiTheme {
    pub const ALL: [UiTheme; 2] = [UiTheme::Light, UiTheme::Dark];

    /// メニューに出す名前。
    pub fn name(self) -> &'static str {
        match self {
            UiTheme::Light => "ライト",
            UiTheme::Dark => "ダーク",
        }
    }

    /// 設定ファイルに書く名前。
    pub fn key(self) -> &'static str {
        match self {
            UiTheme::Light => "light",
            UiTheme::Dark => "dark",
        }
    }

    pub fn from_key(key: &str) -> Option<UiTheme> {
        UiTheme::ALL.into_iter().find(|theme| theme.key() == key)
    }

    pub fn colors(self) -> &'static Colors {
        match self {
            UiTheme::Light => &LIGHT,
            UiTheme::Dark => &DARK,
        }
    }
}

/// 1 つの配色に含まれる色。
pub struct Colors {
    pub panel: Rgba8,
    pub header: Rgba8,
    pub border: Rgba8,
    pub text: Rgba8,
    pub text_disabled: Rgba8,
    pub button: Rgba8,
    pub button_disabled: Rgba8,
    /// 選ばれているもの（ツール、紙、筆圧カーブ）の背景。
    pub selected: Rgba8,
    pub selected_border: Rgba8,
    pub warning: Rgba8,
    pub tooltip: Rgba8,
    /// キャンバスのまわり（作業領域）。
    pub workspace: Rgba8,
}

const LIGHT: Colors = Colors {
    panel: gray(232),
    header: gray(204),
    border: gray(150),
    text: gray(28),
    text_disabled: gray(150),
    button: gray(246),
    button_disabled: gray(226),
    selected: rgb(176, 204, 238),
    selected_border: rgb(74, 120, 190),
    warning: rgb(168, 60, 30),
    tooltip: rgb(255, 255, 236),
    // CLIP STUDIO と同じく中間の灰色
    workspace: gray(102),
};

const DARK: Colors = Colors {
    panel: gray(64),
    header: gray(50),
    border: gray(32),
    text: gray(226),
    text_disabled: gray(124),
    button: gray(82),
    button_disabled: gray(58),
    selected: rgb(52, 86, 132),
    selected_border: rgb(104, 156, 226),
    warning: rgb(240, 124, 92),
    tooltip: gray(44),
    workspace: gray(40),
};

/// いま使っている配色。描く処理のあちこちから読むので、引数で回さずここに置く。
static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_current(theme: UiTheme) {
    CURRENT.store(theme as u8, Ordering::Relaxed);
}

pub fn current() -> UiTheme {
    match CURRENT.load(Ordering::Relaxed) {
        1 => UiTheme::Dark,
        _ => UiTheme::Light,
    }
}

/// いまの配色の色。
pub fn colors() -> &'static Colors {
    current().colors()
}

/// `base` を不透明度 `alpha` で重ねるための、premultiplied の色。使えないものを薄く見せるのに使う。
pub fn veil(base: Rgba8, alpha: u8) -> Rgba8 {
    let channel = |v: u8| (v as u16 * alpha as u16 / 255) as u8;
    Rgba8 { r: channel(base.r), g: channel(base.g), b: channel(base.b), a: alpha }
}

/// 文字の大きさ（論理ピクセル）。Windows の標準の UI 文字（9 ポイント = 12 ピクセル）にそろえる。
pub const BODY_TEXT: f32 = 12.0;
pub const HEADER_TEXT: f32 = BODY_TEXT;
/// ブラシサイズの見本の数字など、小さな添え字。
pub const SMALL_TEXT: f32 = 10.0;
pub const HEADER_HEIGHT: f32 = 22.0;

pub fn opaque(c: Color) -> Rgba8 {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba8 { r: channel(c.r), g: channel(c.g), b: channel(c.b), a: 255 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for theme in UiTheme::ALL {
            assert_eq!(UiTheme::from_key(theme.key()), Some(theme));
        }
        assert_eq!(UiTheme::from_key("purple"), None);
    }

    #[test]
    fn text_stands_out_from_the_panel_in_both_themes() {
        let luma = |c: Rgba8| c.r as i32 + c.g as i32 + c.b as i32;
        for theme in UiTheme::ALL {
            let colors = theme.colors();
            assert!((luma(colors.text) - luma(colors.panel)).abs() > 400, "{theme:?}");
            assert!((luma(colors.text) - luma(colors.button)).abs() > 400, "{theme:?}");
        }
    }

    #[test]
    fn veil_premultiplies() {
        assert_eq!(veil(gray(226), 140), Rgba8 { r: 124, g: 124, b: 124, a: 140 });
    }
}
