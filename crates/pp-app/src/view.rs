use pp_core::Rect;

/// スクロールバーの片方向の位置。`start` と `size` は、スクロールできる範囲全体に対する割合（0..=1）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollSpan {
    pub start: f32,
    pub size: f32,
}

/// スクロールしきったときにも画面に残す、キャンバスの幅（物理ピクセル）。
const SCROLL_KEEP: f32 = 64.0;

/// キャンバスの表示位置・倍率・回転。
/// `screen = R(rotation) * canvas * zoom + pan`（物理ピクセル、回転は時計回りが正）。
#[derive(Clone, Copy, Debug)]
pub struct ViewTransform {
    pub zoom: f32,
    /// ラジアン。
    pub rotation: f32,
    pub pan: [f32; 2],
}

impl ViewTransform {
    /// キャンバス全体が画面に収まるように、回転なしで配置する。
    pub fn fit(canvas: (u32, u32), screen: (u32, u32)) -> Self {
        let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
        let (sw, sh) = (screen.0 as f32, screen.1 as f32);
        let zoom = (sw / cw).min(sh / ch) * 0.9;
        ViewTransform { zoom, rotation: 0.0, pan: [(sw - cw * zoom) * 0.5, (sh - ch * zoom) * 0.5] }
    }

    /// キャンバス座標から画面座標への 2x2 行列 [a, b, c, d]（screen.x = a*x + c*y, screen.y = b*x + d*y）。
    pub fn linear(&self) -> [f32; 4] {
        let (sin, cos) = self.rotation.sin_cos();
        [cos * self.zoom, sin * self.zoom, -sin * self.zoom, cos * self.zoom]
    }

    pub fn canvas_to_screen(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d] = self.linear();
        (a * x + c * y + self.pan[0], b * x + d * y + self.pan[1])
    }

    pub fn screen_to_canvas(&self, x: f64, y: f64) -> (f32, f32) {
        let dx = x as f32 - self.pan[0];
        let dy = y as f32 - self.pan[1];
        let (sin, cos) = self.rotation.sin_cos();
        // 逆回転してから倍率で割る
        ((cos * dx + sin * dy) / self.zoom, (-sin * dx + cos * dy) / self.zoom)
    }

    /// 画面上の点 (x, y) を固定したまま倍率を `factor` 倍する。
    pub fn zoom_at(&mut self, x: f32, y: f32, factor: f32) {
        let new_zoom = (self.zoom * factor).clamp(0.02, 64.0);
        let k = new_zoom / self.zoom;
        self.pan[0] = x - (x - self.pan[0]) * k;
        self.pan[1] = y - (y - self.pan[1]) * k;
        self.zoom = new_zoom;
    }

    /// 回転・拡大したキャンバスを囲む、画面上の範囲（`vertical` なら縦、そうでなければ横の最小と最大）。
    fn screen_extent(&self, canvas: (u32, u32), vertical: bool) -> (f32, f32) {
        let [a, b, c, d] = self.linear();
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)];
        let along = |&(x, y): &(f32, f32)| {
            if vertical { b * x + d * y + self.pan[1] } else { a * x + c * y + self.pan[0] }
        };
        let values = corners.iter().map(along);
        values.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
    }

    /// スクロールできる範囲（画面座標）と、`viewport` の端。キャンバスの端が反対側の端に
    /// `SCROLL_KEEP` だけ残るところまで動かせる。範囲は表示位置によらず、倍率と回転だけで決まる。
    fn scroll_range(&self, canvas: (u32, u32), viewport: Rect, vertical: bool) -> (f32, f32, f32) {
        let (lo, hi) = self.screen_extent(canvas, vertical);
        let (view_lo, view_len) = if vertical {
            (viewport.y0 as f32, viewport.height() as f32)
        } else {
            (viewport.x0 as f32, viewport.width() as f32)
        };
        let reach = (view_len - SCROLL_KEEP).max(0.0);
        (lo - reach, hi + reach, view_lo)
    }

    /// スクロールバーに出す位置。
    pub fn scroll_span(&self, canvas: (u32, u32), viewport: Rect, vertical: bool) -> ScrollSpan {
        let (lo, hi, view_lo) = self.scroll_range(canvas, viewport, vertical);
        let total = (hi - lo).max(1.0);
        let view_len = if vertical { viewport.height() } else { viewport.width() } as f32;
        let size = (view_len / total).clamp(0.0, 1.0);
        ScrollSpan { start: ((view_lo - lo) / total).clamp(0.0, 1.0 - size), size }
    }

    /// スクロールバーのつまみを `start`（割合）の位置にしたときの表示位置にする。
    pub fn scroll_to(&mut self, canvas: (u32, u32), viewport: Rect, vertical: bool, start: f32) {
        let (lo, hi, view_lo) = self.scroll_range(canvas, viewport, vertical);
        let total = (hi - lo).max(1.0);
        let view_len = if vertical { viewport.height() } else { viewport.width() } as f32;
        let wanted = start.clamp(0.0, (1.0 - view_len / total).max(0.0));
        let current = (view_lo - lo) / total;
        let shift = (current - wanted) * total;
        self.pan[if vertical { 1 } else { 0 }] += shift;
    }

    /// 画面上の点 (x, y) を中心に、回転角を `angle` にする。
    pub fn rotate_to(&mut self, x: f32, y: f32, angle: f32) {
        let (sin, cos) = (angle - self.rotation).sin_cos();
        let dx = self.pan[0] - x;
        let dy = self.pan[1] - y;
        self.pan = [x + cos * dx - sin * dy, y + sin * dx + cos * dy];
        self.rotation = angle;
    }
}

/// 軸のまわりの角度を測りはじめる、軸からの距離（論理ピクセル）。近すぎると角度が暴れる。
const PIVOT_DEAD_ZONE: f32 = 12.0;

/// 滑らかな回転で、押した所を軸に回している途中の状態。紙を指で押さえて回すのと同じく、
/// ポインタが軸のまわりを回った角度だけ表示を回す。
pub struct PivotDrag {
    /// 軸にする画面の位置（押した所）。
    pivot: (f32, f32),
    /// ポインタが軸から離れたときの (ポインタの角度, 表示の回転角)。離れるまでは向きが決まらないので `None`。
    start: Option<(f32, f32)>,
}

impl PivotDrag {
    pub fn new(x: f32, y: f32) -> Self {
        PivotDrag { pivot: (x, y), start: None }
    }

    pub fn pivot(&self) -> (f32, f32) {
        self.pivot
    }

    /// ポインタが (x, y) に来たときの、表示の回転角。`rotation` は今の回転角、`scale` は画面の倍率。
    /// 軸に近すぎて向きが決まらないうちは `None`。
    pub fn turn(&mut self, x: f32, y: f32, rotation: f32, scale: f32) -> Option<f32> {
        let (dx, dy) = (x - self.pivot.0, y - self.pivot.1);
        if (dx * dx + dy * dy).sqrt() < PIVOT_DEAD_ZONE * scale {
            return None;
        }
        let angle = dy.atan2(dx);
        let (start_angle, start_rotation) = *self.start.get_or_insert((angle, rotation));
        Some(start_rotation + angle - start_angle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas_to_screen(view: &ViewTransform, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d] = view.linear();
        (a * x + c * y + view.pan[0], b * x + d * y + view.pan[1])
    }

    #[test]
    fn pivot_drag_turns_by_the_angle_around_the_pressed_point() {
        let mut drag = PivotDrag::new(100.0, 100.0);
        assert_eq!(drag.turn(105.0, 100.0, 0.3, 1.0), None, "軸に近いうちは回さない");
        assert_eq!(drag.turn(150.0, 100.0, 0.3, 1.0), Some(0.3), "離れた所を起点にする");
        let quarter = drag.turn(100.0, 150.0, 0.3, 1.0).unwrap();
        assert!((quarter - 0.3 - std::f32::consts::FRAC_PI_2).abs() < 1e-5, "{quarter}");
    }

    #[test]
    fn screen_to_canvas_inverts_transform() {
        let view = ViewTransform { zoom: 1.7, rotation: 0.6, pan: [120.0, -40.0] };
        let (sx, sy) = canvas_to_screen(&view, 300.0, 200.0);
        let (cx, cy) = view.screen_to_canvas(sx as f64, sy as f64);
        assert!((cx - 300.0).abs() < 1e-3 && (cy - 200.0).abs() < 1e-3);
    }

    #[test]
    fn scrolling_moves_the_canvas_and_reports_the_new_position() {
        let viewport = Rect::new(200, 80, 1000, 700);
        let canvas = (3000, 2000);
        let mut view = ViewTransform { zoom: 1.0, rotation: 0.0, pan: [200.0, 80.0] };
        let start = view.scroll_span(canvas, viewport, false);
        assert!(start.size > 0.0 && start.size < 1.0);
        view.scroll_to(canvas, viewport, false, start.start + 0.1);
        assert!(view.pan[0] < 200.0, "右へスクロールするとキャンバスは左へ動く");
        let moved = view.scroll_span(canvas, viewport, false);
        assert!((moved.start - (start.start + 0.1)).abs() < 1e-4);
        assert_eq!(moved.size, start.size, "動かしても全体の長さは変わらない");

        // 端まで動かしても、キャンバスは少し画面に残る
        view.scroll_to(canvas, viewport, true, 1.0);
        let (_, hi) = view.screen_extent(canvas, true);
        assert!((hi - viewport.y0 as f32 - SCROLL_KEEP).abs() < 1e-2, "{hi}");
    }

    #[test]
    fn rotated_canvas_scrolls_over_its_bounding_box() {
        let viewport = Rect::new(0, 0, 800, 600);
        let view = ViewTransform { zoom: 0.2, rotation: 0.7, pan: [400.0, 100.0] };
        let span = view.scroll_span((3000, 2000), viewport, false);
        assert!(span.start >= 0.0 && span.start + span.size <= 1.0 + 1e-6);
    }

    #[test]
    fn rotate_to_keeps_pivot_fixed() {
        let mut view = ViewTransform { zoom: 0.5, rotation: 0.2, pan: [10.0, 20.0] };
        let before = view.screen_to_canvas(400.0, 300.0);
        view.rotate_to(400.0, 300.0, 1.3);
        let after = view.screen_to_canvas(400.0, 300.0);
        assert!((before.0 - after.0).abs() < 1e-3 && (before.1 - after.1).abs() < 1e-3);
        assert_eq!(view.rotation, 1.3);
    }
}
