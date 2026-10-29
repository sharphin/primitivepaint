use pp_core::{Rect, Rgba8, SheetMaterial};

use super::icons::Icon;
use super::pixmap::Align;
use super::widgets::{self, IconButton};
use super::{Action, Frame, Panel, Pixmap, PressKind, Response, SheetInfo, UiState, theme};

const PAD: f32 = 6.0;
const ROW_HEIGHT: f32 = 28.0;
const BUTTON_SIZE: f32 = 26.0;
/// 見出しの下、ボタンの段の下から一覧が始まる。
const LIST_TOP: f32 = theme::HEADER_HEIGHT + PAD + BUTTON_SIZE + 6.0;
const EYE_SIZE: f32 = 22.0;

/// 右下の「紙」パネル。重ねた紙を上から順に並べる（CLIP STUDIO のレイヤーパネルの位置）。
#[derive(Default)]
pub struct SheetPanel {
    frame: Frame,
}

impl SheetPanel {
    fn inner_width(&self) -> f32 {
        self.frame.rect.width() as f32 / self.frame.scale - PAD * 2.0
    }

    fn buttons(&self, state: &UiState) -> Vec<(IconButton, Action)> {
        let can_add = state.mode.has_layers();
        let first = state.sheets.first().map(|s| s.index);
        let last = state.sheets.last().map(|s| s.index);
        let active = state.sheets.iter().find(|s| s.active).map(|s| s.index);
        // 用紙（一番下）は動かせない。用紙のすぐ上の紙は下へ動かせない。
        let movable = active.is_some() && active != first;
        let can_up = movable && active != last;
        let can_down = movable && active.zip(first).is_some_and(|(a, f)| a > f + 1);

        let add_note = if can_add { "" } else { "（油彩には重ねられません）" };
        let row = theme::HEADER_HEIGHT + PAD;
        let rect = |i: usize, gap: f32| {
            self.frame.sub(PAD + i as f32 * (BUTTON_SIZE + 4.0) + gap, row, BUTTON_SIZE, BUTTON_SIZE)
        };
        vec![
            (
                IconButton::new(rect(0, 0.0), Icon::AddTracingPaper, format!("トレーシングペーパーを重ねる{add_note}"))
                    .enabled(can_add),
                Action::AddSheet(SheetMaterial::TracingPaper),
            ),
            (
                IconButton::new(rect(1, 0.0), Icon::AddClearFilm, format!("透明フィルムを重ねる{add_note}"))
                    .enabled(can_add),
                Action::AddSheet(SheetMaterial::ClearFilm),
            ),
            (IconButton::new(rect(2, 10.0), Icon::MoveUp, "上へ入れ替える").enabled(can_up), Action::MoveSheet(true)),
            (
                IconButton::new(rect(3, 10.0), Icon::MoveDown, "下へ入れ替える").enabled(can_down),
                Action::MoveSheet(false),
            ),
            (
                IconButton::new(rect(4, 20.0), Icon::RemoveSheet, "紙を外す（元に戻せません）").enabled(movable),
                Action::RemoveSheet,
            ),
        ]
    }

    /// 上から `row` 行目の矩形。
    fn row_rect(&self, row: usize) -> Rect {
        self.frame.sub(PAD, LIST_TOP + row as f32 * ROW_HEIGHT, self.inner_width(), ROW_HEIGHT)
    }

    fn eye_rect(&self, row: Rect) -> Rect {
        let size = self.frame.px(EYE_SIZE);
        let y0 = row.y0 + (row.height() as i32 - size) / 2;
        let x0 = row.x0 + self.frame.px(4.0);
        Rect::new(x0, y0, x0 + size, y0 + size)
    }

    /// 上から順に (行, 紙)。
    fn rows<'s>(&self, state: &'s UiState) -> impl Iterator<Item = (Rect, &'s SheetInfo)> {
        let rows: Vec<Rect> = (0..state.sheets.len()).map(|i| self.row_rect(i)).collect();
        rows.into_iter().zip(state.sheets.iter().rev())
    }

    fn render_row(&self, pixmap: &mut Pixmap, row: Rect, sheet: &SheetInfo, is_base: bool) {
        if sheet.active {
            pixmap.fill_rect(row, theme::colors().selected);
        }
        pixmap.fill_rect(Rect::new(row.x0, row.y1 - 1, row.x1, row.y1), theme::colors().border);

        let eye = self.eye_rect(row);
        if !is_base {
            pixmap.fill_rect(eye, theme::colors().button);
            pixmap.frame(eye, theme::colors().border);
            if sheet.visible {
                let cx = (eye.x0 + eye.x1) as f32 * 0.5;
                let cy = (eye.y0 + eye.y1) as f32 * 0.5;
                let w = eye.width() as f32 * 0.36;
                let eye_shape = [(cx - w, cy), (cx, cy - w * 0.6), (cx + w, cy), (cx, cy + w * 0.6)];
                pixmap.fill_polygon(&eye_shape, theme::colors().text);
                pixmap.fill_circle(cx, cy, w * 0.32, theme::colors().button);
            }
        }

        // 素材の見本：用紙は白、トレペは乳白、フィルムは枠だけ
        let gap = self.frame.px(6.0);
        let swatch = Rect::new(eye.x1 + gap, eye.y0, eye.x1 + gap + self.frame.px(30.0), eye.y1);
        match sheet.material {
            SheetMaterial::Paper => pixmap.fill_rect(swatch, Rgba8 { r: 255, g: 255, b: 255, a: 255 }),
            SheetMaterial::TracingPaper => pixmap.fill_rect(swatch, Rgba8 { r: 236, g: 236, b: 228, a: 255 }),
            SheetMaterial::ClearFilm => {
                let mid = (swatch.y0 + swatch.y1) / 2;
                pixmap.fill_rect(
                    Rect::new(swatch.x0, mid, swatch.x1, swatch.y1),
                    Rgba8 { r: 210, g: 222, b: 230, a: 255 },
                );
            }
        }
        pixmap.frame(swatch, theme::colors().border);

        let text = Rect::new(swatch.x1 + gap, row.y0, row.x1 - gap, row.y1);
        let color = if sheet.visible { theme::colors().text } else { theme::colors().text_disabled };
        pixmap.text(text, &sheet.name, self.frame.font_px(theme::BODY_TEXT), color, Align::Left);
    }
}

impl Panel for SheetPanel {
    fn place(&mut self, frame: Frame) {
        self.frame = frame;
    }

    fn frame(&self) -> Frame {
        self.frame
    }

    fn render(&self, state: &UiState, pixmap: &mut Pixmap) {
        widgets::background(pixmap, &self.frame);
        widgets::header(pixmap, &self.frame, "紙");
        for (button, _) in self.buttons(state) {
            button.render(pixmap);
        }
        let base = state.sheets.first().map(|s| s.index);
        for (row, sheet) in self.rows(state) {
            self.render_row(pixmap, row, sheet, Some(sheet.index) == base);
        }
    }

    fn tooltip(&self, x: f32, y: f32, state: &UiState) -> Option<String> {
        if let Some((button, _)) = self.buttons(state).into_iter().find(|(b, _)| b.rect.contains(x, y)) {
            return Some(button.tooltip);
        }
        let base = state.sheets.first().map(|s| s.index);
        let (row, sheet) = self.rows(state).find(|(row, _)| row.contains(x, y))?;
        if Some(sheet.index) != base && self.eye_rect(row).contains(x, y) {
            return Some(if sheet.visible { "めくる".into() } else { "戻す（めくり中）".into() });
        }
        None
    }

    fn press(&mut self, x: f32, y: f32, _kind: PressKind, state: &UiState) -> Response {
        if let Some((_, action)) = self.buttons(state).into_iter().find(|(b, _)| b.hit(x, y)) {
            return Response::action(action);
        }
        let base = state.sheets.first().map(|s| s.index);
        let hit = self.rows(state).find(|(row, _)| row.contains(x, y));
        let Some((row, sheet)) = hit else {
            return Response::default();
        };
        if Some(sheet.index) != base && self.eye_rect(row).contains(x, y) {
            return Response::action(Action::FlipSheet(sheet.index));
        }
        Response::action(Action::SelectSheet(sheet.index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::state;

    fn panel() -> SheetPanel {
        let mut panel = SheetPanel::default();
        panel.place(Frame { rect: Rect::new(980, 348, 1280, 800), scale: 1.0 });
        panel
    }

    fn center(r: Rect) -> (f32, f32) {
        ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5)
    }

    #[test]
    fn top_row_is_the_top_sheet_and_eye_flips_it() {
        let mut panel = panel();
        let state = state();
        let top = panel.row_rect(0);
        let (x, y) = center(top);
        assert_eq!(panel.press(x, y, PressKind::Primary, &state).action, Some(Action::SelectSheet(2)));
        let (ex, ey) = center(panel.eye_rect(top));
        assert_eq!(panel.press(ex, ey, PressKind::Primary, &state).action, Some(Action::FlipSheet(2)));
        // 用紙はめくれない
        let (bx, by) = center(panel.eye_rect(panel.row_rect(1)));
        assert_eq!(panel.press(bx, by, PressKind::Primary, &state).action, Some(Action::SelectSheet(1)));
    }

    #[test]
    fn buttons_follow_what_the_active_sheet_can_do() {
        let panel = panel();
        let state = state();
        let enabled: Vec<(Action, bool)> = panel.buttons(&state).into_iter().map(|(b, a)| (a, b.enabled)).collect();
        assert!(enabled.contains(&(Action::RemoveSheet, true)));
        // 一番上で用紙のすぐ上なので、上にも下にも動かせない
        assert!(enabled.contains(&(Action::MoveSheet(true), false)));
        assert!(enabled.contains(&(Action::MoveSheet(false), false)));
    }
}
