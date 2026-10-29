use pp_core::{CanvasMode, Rect};

use super::dialog::{self, Dialog, DialogKey, DialogResult, WheelStep};
use super::number_field::NumberField;
use super::pixmap::Align;
use super::widgets::{self, Button};
use super::{Action, Frame, Pixmap, theme};

/// 紙モードですぐ入れられる用紙の大きさ（ピクセル）。A4・B5 は 350dpi。
const PAPER_SIZES: [(&str, u32, u32); 6] = [
    ("横長 3000×2000", 3000, 2000),
    ("縦長 2000×3000", 2000, 3000),
    ("正方形 2400×2400", 2400, 2400),
    ("A4 縦 2894×4093", 2894, 4093),
    ("A4 横 4093×2894", 4093, 2894),
    ("B5 縦 2508×3541", 2508, 3541),
];

/// 油彩キャンバスの号数（F は人物、P は風景、M は海景）。縦長で、200dpi。
/// 油彩は描くたびに混色の計算をするので、紙より粗くして軽くしている。
const OIL_SIZES: [(&str, u32, u32); 6] = [
    ("F6 2504×3228", 2504, 3228),
    ("F10 3583×4173", 3583, 4173),
    ("P6 2150×3228", 2150, 3228),
    ("P10 3228×4173", 3228, 4173),
    ("M6 1906×3228", 1906, 3228),
    ("M10 2622×4173", 2622, 4173),
];

fn sizes_for(mode: CanvasMode) -> &'static [(&'static str, u32, u32); 6] {
    match mode {
        CanvasMode::Paper => &PAPER_SIZES,
        CanvasMode::OilCanvas => &OIL_SIZES,
    }
}

/// ホイール 1 段で動く量。Shift で細かく、Ctrl で大きく。
const WHEEL_STEP: u32 = 10;
const WHEEL_STEP_FINE: u32 = 1;
const WHEEL_STEP_COARSE: u32 = 100;

const MODES: [CanvasMode; 2] = [CanvasMode::Paper, CanvasMode::OilCanvas];

/// 入れられる大きさの範囲（ピクセル）。
pub const MIN_SIDE: u32 = 16;
pub const MAX_SIDE: u32 = 10000;

const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 300.0;
const FIELD_WIDTH: f32 = 100.0;
const PAD: f32 = 14.0;
const BUTTON_HEIGHT: f32 = 26.0;
const LABEL_HEIGHT: f32 = 20.0;

/// 大きさを入れる欄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Width,
    Height,
}

/// 「新規」のダイアログ。キャンバスモードと用紙の大きさを決める。大きさは数字で入れるか、候補から選ぶ。
pub struct NewDocumentDialog {
    frame: Frame,
    mode: CanvasMode,
    width: NumberField,
    height: NumberField,
    focus: Field,
}

impl NewDocumentDialog {
    pub fn new(mode: CanvasMode) -> Self {
        let (_, width, height) = sizes_for(mode)[0];
        NewDocumentDialog {
            frame: Frame::default(),
            mode,
            width: NumberField::new(width, MIN_SIDE, MAX_SIDE),
            height: NumberField::new(height, MIN_SIDE, MAX_SIDE),
            focus: Field::Width,
        }
    }

    /// 入っている大きさ。範囲外や空なら `None`。
    fn size(&self) -> Option<(u32, u32)> {
        Some((self.width.value()?, self.height.value()?))
    }

    fn field_mut(&mut self, field: Field) -> &mut NumberField {
        match field {
            Field::Width => &mut self.width,
            Field::Height => &mut self.height,
        }
    }

    fn set_size(&mut self, width: u32, height: u32) {
        self.width.set_value(width);
        self.height.set_value(height);
    }

    fn field_at(&self, x: f32, y: f32) -> Option<Field> {
        [Field::Width, Field::Height].into_iter().find(|&f| self.field_rect(f).contains(x, y))
    }

    fn mode_top() -> f32 {
        theme::HEADER_HEIGHT + PAD + LABEL_HEIGHT
    }

    fn size_top() -> f32 {
        Self::mode_top() + BUTTON_HEIGHT + PAD * 0.6 + LABEL_HEIGHT
    }

    fn presets_top() -> f32 {
        Self::size_top() + BUTTON_HEIGHT + PAD * 0.6 + LABEL_HEIGHT
    }

    fn field_rect(&self, field: Field) -> Rect {
        let x = match field {
            Field::Width => PAD + 30.0,
            Field::Height => PAD + 30.0 + FIELD_WIDTH + 64.0,
        };
        self.frame.sub(x, Self::size_top(), FIELD_WIDTH, BUTTON_HEIGHT)
    }

    fn mode_buttons(&self) -> Vec<(Button, CanvasMode)> {
        let width = (WIDTH - PAD * 2.0 - 6.0) / 2.0;
        MODES
            .iter()
            .enumerate()
            .map(|(i, &mode)| {
                let rect = self.frame.sub(PAD + i as f32 * (width + 6.0), Self::mode_top(), width, BUTTON_HEIGHT);
                (Button::new(rect, mode.name()).selected(mode == self.mode), mode)
            })
            .collect()
    }

    fn size_buttons(&self) -> Vec<(Button, usize)> {
        let width = (WIDTH - PAD * 2.0 - 6.0) / 2.0;
        let current = self.size();
        sizes_for(self.mode)
            .iter()
            .enumerate()
            .map(|(i, &(label, w, h))| {
                let (column, row) = ((i % 2) as f32, (i / 2) as f32);
                let x = PAD + column * (width + 6.0);
                let y = Self::presets_top() + row * (BUTTON_HEIGHT + 4.0);
                let button = Button::new(self.frame.sub(x, y, width, BUTTON_HEIGHT), label);
                (button.selected(current == Some((w, h))), i)
            })
            .collect()
    }

    fn create_button(&self) -> Button {
        Button::new(
            self.frame.sub(WIDTH - PAD - 180.0 - 6.0, HEIGHT - PAD - BUTTON_HEIGHT, 90.0, BUTTON_HEIGHT),
            "作成",
        )
        .enabled(self.size().is_some())
    }

    fn cancel_button(&self) -> Button {
        Button::new(self.frame.sub(WIDTH - PAD - 90.0, HEIGHT - PAD - BUTTON_HEIGHT, 90.0, BUTTON_HEIGHT), "キャンセル")
    }
}

impl Dialog for NewDocumentDialog {
    fn key(&mut self, key: DialogKey) -> bool {
        match key {
            DialogKey::Field(key) => self.field_mut(self.focus).key(key),
            DialogKey::Tab => {
                self.focus = match self.focus {
                    Field::Width => Field::Height,
                    Field::Height => Field::Width,
                };
                // Windows と同じく、移った先の欄はすべて選んだ状態にする
                self.field_mut(self.focus).select_all();
                true
            }
        }
    }

    fn wheel(&mut self, x: f32, y: f32, steps: i32, step: WheelStep) -> bool {
        let Some(field) = self.field_at(x, y) else { return false };
        let amount = match step {
            WheelStep::Normal => WHEEL_STEP,
            WheelStep::Fine => WHEEL_STEP_FINE,
            WheelStep::Coarse => WHEEL_STEP_COARSE,
        };
        let changed = self.field_mut(field).wheel(steps, amount);
        let moved = self.focus != field;
        self.focus = field;
        changed || moved
    }

    fn is_selecting(&self) -> bool {
        self.width.is_selecting() || self.height.is_selecting()
    }

    fn drag(&mut self, x: f32) -> bool {
        self.field_mut(self.focus).drag(x)
    }

    fn release(&mut self) {
        self.width.release();
        self.height.release();
    }

    fn place(&mut self, screen: Rect, scale: f32) {
        self.frame = Frame { rect: dialog::centered(screen, scale, WIDTH, HEIGHT), scale };
    }

    fn rect(&self) -> Rect {
        self.frame.rect
    }
    fn confirm(&self) -> DialogResult {
        match self.size() {
            Some((width, height)) => DialogResult::Done(Action::NewDocument { mode: self.mode, width, height }),
            None => DialogResult::Open(false),
        }
    }

    fn press(&mut self, x: f32, y: f32, select: bool) -> DialogResult {
        if let Some((_, mode)) = self.mode_buttons().into_iter().find(|(b, _)| b.hit(x, y)) {
            if mode != self.mode {
                // よく使う大きさがモードで変わるので、そのモードの最初の大きさにする
                self.mode = mode;
                let (_, width, height) = sizes_for(mode)[0];
                self.set_size(width, height);
            }
            return DialogResult::Open(true);
        }
        if let Some((_, index)) = self.size_buttons().into_iter().find(|(b, _)| b.hit(x, y)) {
            let (_, width, height) = sizes_for(self.mode)[index];
            self.set_size(width, height);
            return DialogResult::Open(true);
        }
        if let Some(field) = self.field_at(x, y) {
            let extend = select && field == self.focus;
            self.focus = field;
            self.field_mut(field).press(x, extend);
            return DialogResult::Open(true);
        }
        if self.create_button().hit(x, y) {
            return self.confirm();
        }
        if self.cancel_button().hit(x, y) {
            return DialogResult::Cancel;
        }
        DialogResult::Open(false)
    }

    fn render(&self, pixmap: &mut Pixmap) {
        let f = &self.frame;
        widgets::background(pixmap, f);
        pixmap.frame(f.rect, theme::colors().border);
        widgets::header(pixmap, f, "新規");
        let font = f.font_px(theme::BODY_TEXT);
        let label = |pixmap: &mut Pixmap, top: f32, text: &str| {
            let rect = f.sub(PAD, top - LABEL_HEIGHT, WIDTH - PAD * 2.0, LABEL_HEIGHT);
            pixmap.text(rect, text, font, theme::colors().text, Align::Left);
        };
        label(pixmap, Self::mode_top(), "キャンバス");
        for (button, _) in self.mode_buttons() {
            button.render(pixmap, f);
        }
        label(pixmap, Self::size_top(), "大きさ（ピクセル）");
        let row = |x: f32, w: f32| f.sub(x, Self::size_top(), w, BUTTON_HEIGHT);
        pixmap.text(row(PAD, 30.0), "幅", font, theme::colors().text, Align::Left);
        pixmap.text(row(PAD + 30.0 + FIELD_WIDTH, 64.0), "× 高さ", font, theme::colors().text, Align::Center);
        for (field, number) in [(Field::Width, &self.width), (Field::Height, &self.height)] {
            number.render(pixmap, f, self.field_rect(field), field == self.focus);
        }
        if self.size().is_none() {
            let hint = format!("{MIN_SIDE}〜{MAX_SIDE} の数字を入れてください");
            // 「大きさ」の見出しと同じ行の右に出す
            let line = f.sub(PAD, Self::size_top() - LABEL_HEIGHT, WIDTH - PAD * 2.0, LABEL_HEIGHT);
            pixmap.text(line, &hint, font, theme::colors().warning, Align::Right);
        }
        label(pixmap, Self::presets_top(), "よく使う大きさ");
        for (button, _) in self.size_buttons() {
            button.render(pixmap, f);
        }
        let create = self.create_button();
        let enabled = create.enabled;
        create.selected(enabled).render(pixmap, f);
        self.cancel_button().render(pixmap, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::number_field::FieldKey;

    fn center(r: Rect) -> (f32, f32) {
        ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5)
    }

    #[test]
    fn chosen_mode_and_size_are_created() {
        let mut dialog = NewDocumentDialog::new(CanvasMode::Paper);
        dialog.place(Rect::new(0, 0, 1200, 800), 1.0);
        let (x, y) = center(dialog.mode_buttons()[1].0.rect);
        assert_eq!(dialog.press(x, y, false), DialogResult::Open(true));
        assert_eq!(dialog.size(), Some((2504, 3228)), "油彩キャンバスは号数の大きさになる");
        let (x, y) = center(dialog.size_buttons()[3].0.rect);
        dialog.press(x, y, false);
        let (x, y) = center(dialog.create_button().rect);
        assert_eq!(
            dialog.press(x, y, false),
            DialogResult::Done(Action::NewDocument { mode: CanvasMode::OilCanvas, width: 3228, height: 4173 })
        );
        let (x, y) = center(dialog.cancel_button().rect);
        assert_eq!(dialog.press(x, y, false), DialogResult::Cancel);
    }

    #[test]
    fn typed_size_is_created_and_out_of_range_is_refused() {
        let mut dialog = NewDocumentDialog::new(CanvasMode::Paper);
        dialog.place(Rect::new(0, 0, 1200, 800), 1.0);
        let key = |dialog: &mut NewDocumentDialog, key: FieldKey| dialog.key(DialogKey::Field(key));
        // 開いたときは幅がすべて選ばれているので、打つと置き換わる
        for c in "1234x".chars() {
            key(&mut dialog, FieldKey::Char(c));
        }
        dialog.key(DialogKey::Tab);
        key(&mut dialog, FieldKey::End { select: false });
        key(&mut dialog, FieldKey::Backspace);
        assert_eq!(
            dialog.confirm(),
            DialogResult::Done(Action::NewDocument { mode: CanvasMode::Paper, width: 1234, height: 200 })
        );
        key(&mut dialog, FieldKey::SelectAll);
        key(&mut dialog, FieldKey::Char('9'));
        assert_eq!(dialog.confirm(), DialogResult::Open(false), "16 より小さい");
        assert!(!dialog.create_button().enabled);
    }

    #[test]
    fn wheel_changes_the_field_under_the_pointer() {
        let mut dialog = NewDocumentDialog::new(CanvasMode::Paper);
        dialog.place(Rect::new(0, 0, 1200, 800), 1.0);
        let (x, y) = center(dialog.field_rect(Field::Height));
        assert!(dialog.wheel(x, y, 1, WheelStep::Normal));
        assert!(dialog.wheel(x, y, -1, WheelStep::Fine));
        assert!(dialog.wheel(x, y, 1, WheelStep::Coarse));
        assert_eq!(dialog.size(), Some((3000, 2109)));
        assert_eq!(dialog.focus, Field::Height);
        assert!(!dialog.wheel(0.0, 0.0, 1, WheelStep::Normal), "欄の外では何もしない");
    }

    #[test]
    fn buttons_fit_inside_the_dialog() {
        let mut dialog = NewDocumentDialog::new(CanvasMode::Paper);
        dialog.place(Rect::new(0, 0, 1200, 800), 1.5);
        let inside = |r: Rect| r.intersect(&dialog.rect()) == r;
        assert!(dialog.size_buttons().iter().all(|(b, _)| inside(b.rect)));
        assert!(inside(dialog.create_button().rect) && inside(dialog.cancel_button().rect));
        let last_size = dialog.size_buttons().last().unwrap().0.rect;
        assert!(last_size.y1 < dialog.create_button().rect.y0, "大きさのボタンと作成ボタンが重ならない");
    }
}
