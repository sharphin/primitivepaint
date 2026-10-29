use pp_core::Rect;

use super::{Action, Frame, Panel, Pixmap, PressKind, Response, UiState, theme};
use crate::view::ScrollSpan;

/// つまみの最小の長さ（論理ピクセル）。拡大してキャンバスが大きくなっても掴めるようにする。
const MIN_THUMB: f32 = 24.0;

/// キャンバスの右（縦）と下（横）のスクロールバー。つまみをドラッグするか、溝を押すとそこへ動く。
pub struct ScrollBar {
    frame: Frame,
    vertical: bool,
    /// ドラッグ中なら、つまみの先頭からポインタまでの距離（割合）。
    grab: Option<f32>,
}

impl ScrollBar {
    pub fn new(vertical: bool) -> Self {
        ScrollBar { frame: Frame::default(), vertical, grab: None }
    }

    fn span(&self, state: &UiState) -> ScrollSpan {
        if self.vertical { state.scroll_v } else { state.scroll_h }
    }

    /// 溝（つまみが動く範囲）。縦のスクロールバーは右下の角まで伸びているので、角の分を除く。
    fn track(&self) -> Rect {
        let r = self.frame.rect;
        if self.vertical { Rect::new(r.x0, r.y0, r.x1, r.y1 - r.width() as i32) } else { r }
    }

    fn track_range(&self) -> (f32, f32) {
        let t = self.track();
        if self.vertical { (t.y0 as f32, t.height() as f32) } else { (t.x0 as f32, t.width() as f32) }
    }

    /// つまみの表示上の長さ（割合）。短すぎるときは `MIN_THUMB` まで伸ばす。
    fn thumb_size(&self, span: &ScrollSpan) -> f32 {
        let (_, length) = self.track_range();
        span.size.max(self.frame.px(MIN_THUMB) as f32 / length.max(1.0)).min(1.0)
    }

    /// 表示上のつまみの先頭（割合）。つまみを伸ばした分、動ける範囲を縮めて対応させる。
    fn thumb_start(&self, span: &ScrollSpan) -> f32 {
        let room = 1.0 - span.size;
        let shown_room = 1.0 - self.thumb_size(span);
        if room <= 0.0 { 0.0 } else { span.start / room * shown_room }
    }

    fn thumb(&self, span: &ScrollSpan) -> Rect {
        let (from, length) = self.track_range();
        let a = (from + self.thumb_start(span) * length) as i32;
        let b = (from + (self.thumb_start(span) + self.thumb_size(span)) * length) as i32;
        let t = self.track();
        let inset = self.frame.px(2.0);
        if self.vertical {
            Rect::new(t.x0 + inset, a, t.x1 - inset, b)
        } else {
            Rect::new(a, t.y0 + inset, b, t.y1 - inset)
        }
    }

    /// 画面の (x, y) を溝の中の割合にする。
    fn fraction_at(&self, x: f32, y: f32) -> f32 {
        let (from, length) = self.track_range();
        let along = if self.vertical { y } else { x };
        (along - from) / length.max(1.0)
    }

    /// つまみの先頭を表示上の割合 `shown` に置いたときの、本当の位置。
    fn action_for(&self, span: &ScrollSpan, shown: f32) -> Action {
        let shown_room = 1.0 - self.thumb_size(span);
        let room = 1.0 - span.size;
        let start = if shown_room <= 0.0 { 0.0 } else { (shown / shown_room).clamp(0.0, 1.0) * room };
        Action::ScrollTo { vertical: self.vertical, start }
    }
}

impl Panel for ScrollBar {
    fn place(&mut self, frame: Frame) {
        self.frame = frame;
    }

    fn frame(&self) -> Frame {
        self.frame
    }

    fn render(&self, state: &UiState, pixmap: &mut Pixmap) {
        pixmap.fill_rect(self.frame.rect, theme::colors().panel);
        let span = self.span(state);
        if span.size < 1.0 {
            let color = if self.grab.is_some() { theme::colors().selected_border } else { theme::colors().border };
            pixmap.fill_rect(self.thumb(&span), color);
        }
    }

    fn press(&mut self, x: f32, y: f32, _kind: PressKind, state: &UiState) -> Response {
        let span = self.span(state);
        if span.size >= 1.0 || !self.track().contains(x, y) {
            return Response::default();
        }
        let at = self.fraction_at(x, y);
        let start = self.thumb_start(&span);
        if self.thumb(&span).contains(x, y) {
            self.grab = Some(at - start);
            return Response { action: None, redraw: true };
        }
        // 溝を押したら、つまみの真ん中がそこに来るように動かし、そのままドラッグできるようにする
        let half = self.thumb_size(&span) * 0.5;
        self.grab = Some(half);
        Response::action(self.action_for(&span, at - half))
    }

    fn drag(&mut self, x: f32, y: f32, state: &UiState) -> Response {
        let Some(grab) = self.grab else { return Response::default() };
        let span = self.span(state);
        Response::action(self.action_for(&span, self.fraction_at(x, y) - grab))
    }

    fn release(&mut self) -> Response {
        self.grab = None;
        Response { action: None, redraw: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::state;

    fn bar() -> ScrollBar {
        let mut bar = ScrollBar::new(false);
        bar.place(Frame { rect: Rect::new(200, 700, 900, 712), scale: 1.0 });
        bar
    }

    fn started(action: Option<Action>) -> f32 {
        match action {
            Some(Action::ScrollTo { vertical: false, start }) => start,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn dragging_the_thumb_follows_the_pointer() {
        let mut bar = bar();
        let mut state = state();
        state.scroll_h = ScrollSpan { start: 0.2, size: 0.3 };
        let thumb = bar.thumb(&state.scroll_h);
        let y = 706.0;
        let grab_x = thumb.x0 as f32 + 5.0;
        assert_eq!(bar.press(grab_x, y, PressKind::Primary, &state).action, None);
        let moved = started(bar.drag(grab_x + 70.0, y, &state).action);
        assert!((moved - 0.3).abs() < 0.01, "溝の 1 割ぶん動く: {moved}");
        bar.release();
        assert_eq!(bar.drag(grab_x, y, &state).action, None);
    }

    #[test]
    fn pressing_the_track_centers_the_thumb_there() {
        let mut bar = bar();
        let mut state = state();
        state.scroll_h = ScrollSpan { start: 0.0, size: 0.2 };
        let start = started(bar.press(200.0 + 700.0 * 0.8, 706.0, PressKind::Primary, &state).action);
        assert!((start - 0.7).abs() < 0.01, "{start}");
    }

    #[test]
    fn whole_canvas_in_view_has_nothing_to_scroll() {
        let mut bar = bar();
        let mut state = state();
        state.scroll_h = ScrollSpan { start: 0.0, size: 1.0 };
        assert_eq!(bar.press(500.0, 706.0, PressKind::Primary, &state).action, None);
    }
}
