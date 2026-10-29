use std::cell::Cell;

use pp_core::Rect;

use super::pixmap::Align;
use super::{Action, Frame, Panel, Pixmap, PressKind, Response, UiState, theme};

const TAB_MAX_WIDTH: f32 = 130.0;
const TAB_MIN_WIDTH: f32 = 80.0;
const CLOSE_SIZE: f32 = 12.0;
const NEW_TAB_WIDTH: f32 = 20.0;

/// キャンバスの上に並ぶ作品のタブ（CLIP STUDIO と同じ位置）。押すと切り替わり、× で閉じる。
#[derive(Default)]
pub struct TabBar {
    frame: Frame,
    /// ポインタが載っている閉じるボタンのタブ。
    hovered_close: Option<usize>,
    /// 最後に描いたときのタブの数。`hover` は状態を受け取らないので、ここで覚えておく。
    drawn_count: Cell<usize>,
}

impl TabBar {
    fn tab_width(&self, count: usize) -> f32 {
        let available = self.frame.rect.width() as f32 / self.frame.scale - NEW_TAB_WIDTH - 8.0;
        (available / count.max(1) as f32).clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH)
    }

    fn height(&self) -> f32 {
        self.frame.rect.height() as f32 / self.frame.scale
    }

    fn tab_rect(&self, index: usize, count: usize) -> Rect {
        let width = self.tab_width(count);
        self.frame.sub(4.0 + index as f32 * width, 2.0, width - 2.0, self.height() - 2.0)
    }

    fn close_rect(&self, tab: Rect) -> Rect {
        let size = self.frame.px(CLOSE_SIZE);
        let x1 = tab.x1 - self.frame.px(6.0);
        let y0 = tab.y0 + (tab.height() as i32 - size) / 2;
        Rect::new(x1 - size, y0, x1, y0 + size)
    }

    fn new_tab_rect(&self, count: usize) -> Rect {
        let x = 4.0 + count as f32 * self.tab_width(count) + 2.0;
        self.frame.sub(x, 3.0, NEW_TAB_WIDTH - 4.0, self.height() - 6.0)
    }

    /// (タブの番号, 閉じるボタンの上か)。
    fn hit(&self, x: f32, y: f32, count: usize) -> Option<(usize, bool)> {
        (0..count).map(|i| (i, self.tab_rect(i, count))).find(|(_, r)| r.contains(x, y)).map(|(i, r)| {
            let on_close = self.close_rect(r).contains(x, y);
            (i, on_close)
        })
    }
}

impl Panel for TabBar {
    fn place(&mut self, frame: Frame) {
        self.frame = frame;
    }

    fn frame(&self) -> Frame {
        self.frame
    }

    fn render(&self, state: &UiState, pixmap: &mut Pixmap) {
        let r = self.frame.rect;
        pixmap.fill_rect(r, theme::colors().header);
        pixmap.fill_rect(Rect::new(r.x0, r.y1 - 1, r.x1, r.y1), theme::colors().border);
        let count = state.tabs.len();
        self.drawn_count.set(count);
        let font = self.frame.font_px(theme::BODY_TEXT);
        for (i, tab) in state.tabs.iter().enumerate() {
            let rect = self.tab_rect(i, count);
            let current = i == state.current_tab;
            // 選んでいるタブは下の区切り線を消して、キャンバスとつながって見えるようにする
            let fill = if current { theme::colors().panel } else { theme::colors().button_disabled };
            pixmap.fill_rect(Rect::new(rect.x0, rect.y0, rect.x1, r.y1 - if current { 0 } else { 1 }), fill);
            pixmap.frame(Rect::new(rect.x0, rect.y0, rect.x1, rect.y1 + 1), theme::colors().border);
            if current {
                pixmap.fill_rect(Rect::new(rect.x0 + 1, r.y1 - 1, rect.x1 - 1, r.y1), theme::colors().panel);
            }

            let close = self.close_rect(rect);
            let label = if tab.modified { format!("{} *", tab.name) } else { tab.name.clone() };
            let text = Rect::new(rect.x0 + self.frame.px(8.0), rect.y0, close.x0 - self.frame.px(4.0), rect.y1);
            let color = if current { theme::colors().text } else { theme::colors().text_disabled };
            pixmap.text(text, &label, font, color, Align::Left);

            if self.hovered_close == Some(i) {
                pixmap.fill_rect(close, theme::colors().selected);
            }
            let inset = close.width() as f32 * 0.28;
            let (x0, y0) = (close.x0 as f32 + inset, close.y0 as f32 + inset);
            let (x1, y1) = (close.x1 as f32 - inset, close.y1 as f32 - inset);
            let stroke = (self.frame.scale * 1.3).max(1.0);
            pixmap.line((x0, y0), (x1, y1), stroke, theme::colors().text);
            pixmap.line((x0, y1), (x1, y0), stroke, theme::colors().text);
        }

        // 新しい作品を作る「＋」
        let plus = self.new_tab_rect(count);
        let (cx, cy) = ((plus.x0 + plus.x1) as f32 * 0.5, (plus.y0 + plus.y1) as f32 * 0.5);
        let arm = plus.height() as f32 * 0.3;
        let stroke = (self.frame.scale * 1.5).max(1.0);
        pixmap.line((cx - arm, cy), (cx + arm, cy), stroke, theme::colors().text);
        pixmap.line((cx, cy - arm), (cx, cy + arm), stroke, theme::colors().text);
    }

    fn press(&mut self, x: f32, y: f32, _kind: PressKind, state: &UiState) -> Response {
        let count = state.tabs.len();
        if self.new_tab_rect(count).contains(x, y) {
            return Response::action(Action::ShowNewDocument);
        }
        match self.hit(x, y, count) {
            Some((index, true)) => Response::action(Action::CloseTab(index)),
            Some((index, false)) if index != state.current_tab => Response::action(Action::SelectTab(index)),
            _ => Response::default(),
        }
    }

    fn hover(&mut self, x: f32, y: f32) -> bool {
        let hovered = self.hit(x, y, self.drawn_count.get()).and_then(|(i, on_close)| on_close.then_some(i));
        let changed = hovered != self.hovered_close;
        self.hovered_close = hovered;
        changed
    }
}
