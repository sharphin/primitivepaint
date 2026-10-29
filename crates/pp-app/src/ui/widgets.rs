use pp_core::Rect;

use super::icons::{self, Icon};
use super::pixmap::{Align, Pixmap};
use super::{Frame, theme};

/// パネルの見出し帯。
pub fn header(pixmap: &mut Pixmap, frame: &Frame, title: &str) {
    let r = frame.rect;
    let band = Rect::new(r.x0, r.y0, r.x1, r.y0 + frame.px(theme::HEADER_HEIGHT));
    pixmap.fill_rect(band, theme::colors().header);
    pixmap.fill_rect(Rect::new(r.x0, band.y1 - 1, r.x1, band.y1), theme::colors().border);
    let text = Rect::new(band.x0 + frame.px(8.0), band.y0, band.x1, band.y1);
    pixmap.text(text, title, frame.font_px(theme::HEADER_TEXT), theme::colors().text, Align::Left);
}

/// パネルの背景と、右端・下端の区切り線。
pub fn background(pixmap: &mut Pixmap, frame: &Frame) {
    let r = frame.rect;
    pixmap.fill_rect(r, theme::colors().panel);
    pixmap.fill_rect(Rect::new(r.x1 - 1, r.y0, r.x1, r.y1), theme::colors().border);
    pixmap.fill_rect(Rect::new(r.x0, r.y1 - 1, r.x1, r.y1), theme::colors().border);
}

/// 文字の付いた押しボタン。
pub struct Button {
    pub rect: Rect,
    pub label: String,
    pub enabled: bool,
    pub selected: bool,
}

impl Button {
    pub fn new(rect: Rect, label: impl Into<String>) -> Self {
        Button { rect, label: label.into(), enabled: true, selected: false }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn hit(&self, x: f32, y: f32) -> bool {
        self.enabled && self.rect.contains(x, y)
    }

    pub fn render(&self, pixmap: &mut Pixmap, frame: &Frame) {
        let (fill, border, text) = match (self.enabled, self.selected) {
            (false, _) => (theme::colors().button_disabled, theme::colors().border, theme::colors().text_disabled),
            (true, true) => (theme::colors().selected, theme::colors().selected_border, theme::colors().text),
            (true, false) => (theme::colors().button, theme::colors().border, theme::colors().text),
        };
        pixmap.fill_rect(self.rect, fill);
        pixmap.frame(self.rect, border);
        pixmap.text(self.rect, &self.label, frame.font_px(theme::BODY_TEXT), text, Align::Center);
    }
}

/// 絵柄の押しボタン。名前はポインタを載せたときに説明として出す。
pub struct IconButton {
    pub rect: Rect,
    pub icon: Icon,
    pub tooltip: String,
    pub enabled: bool,
    pub selected: bool,
}

impl IconButton {
    pub fn new(rect: Rect, icon: Icon, tooltip: impl Into<String>) -> Self {
        IconButton { rect, icon, tooltip: tooltip.into(), enabled: true, selected: false }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn hit(&self, x: f32, y: f32) -> bool {
        self.enabled && self.rect.contains(x, y)
    }

    pub fn render(&self, pixmap: &mut Pixmap) {
        let (fill, border, color) = match (self.enabled, self.selected) {
            (false, _) => (theme::colors().button_disabled, theme::colors().border, theme::colors().text_disabled),
            (true, true) => (theme::colors().selected, theme::colors().selected_border, theme::colors().text),
            (true, false) => (theme::colors().button, theme::colors().border, theme::colors().text),
        };
        pixmap.fill_rect(self.rect, fill);
        pixmap.frame(self.rect, border);
        // 絵柄は正方形で、ボタンの中央に置く
        let side = self.rect.width().min(self.rect.height()) as i32;
        let inset = side / 8;
        let x0 = self.rect.x0 + (self.rect.width() as i32 - side) / 2 + inset;
        let y0 = self.rect.y0 + (self.rect.height() as i32 - side) / 2 + inset;
        let icon_rect = Rect::new(x0, y0, x0 + side - inset * 2, y0 + side - inset * 2);
        icons::draw(pixmap, icon_rect, self.icon, color);
        if !self.enabled {
            // 使えないボタンの地の色を 55% で重ねる
            pixmap.fill_rect(self.rect, theme::veil(theme::colors().button_disabled, 140));
        }
    }
}

/// 0〜1 の値を左右に動かして選ぶつまみ。
pub struct Slider {
    pub rect: Rect,
    /// 0..=1
    pub value: f32,
}

impl Slider {
    /// 押せる範囲（つまみの幅だけ左右に広い）。
    pub fn grab_rect(&self) -> Rect {
        Rect::new(self.rect.x0 - 6, self.rect.y0 - 4, self.rect.x1 + 6, self.rect.y1 + 4)
    }

    /// 画面の x 座標を値（0..=1）にする。
    pub fn value_at(&self, x: f32) -> f32 {
        ((x - self.rect.x0 as f32) / self.rect.width().max(1) as f32).clamp(0.0, 1.0)
    }

    pub fn render(&self, pixmap: &mut Pixmap, frame: &Frame) {
        let track = self.rect;
        let mid = (track.y0 + track.y1) / 2;
        pixmap.fill_rect(Rect::new(track.x0, mid - 2, track.x1, mid + 2), theme::colors().border);
        let knob_x = track.x0 as f32 + self.value.clamp(0.0, 1.0) * track.width() as f32;
        let knob_w = frame.px(4.0);
        let knob = Rect::new(knob_x as i32 - knob_w, track.y0, knob_x as i32 + knob_w, track.y1);
        pixmap.fill_rect(knob, theme::colors().button);
        pixmap.frame(knob, theme::colors().selected_border);
    }
}
