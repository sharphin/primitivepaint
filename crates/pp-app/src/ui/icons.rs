//! パネルのボタンに使う絵柄。文字の代わりに置き、名前はポインタを載せたときに出す。

use std::f32::consts::{FRAC_PI_2, PI};

use pp_core::{Rect, Rgba8};

use super::Pixmap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    RotateLeft,
    RotateRight,
    ResetRotation,
    ZoomOut,
    ZoomIn,
    Fit,
    /// トレーシングペーパーを重ねる。
    AddTracingPaper,
    /// 透明フィルムを重ねる。
    AddClearFilm,
    MoveUp,
    MoveDown,
    /// 紙を外す。
    RemoveSheet,
    /// 下の絵の具と混ぜて描く。
    MixPaint,
    /// 下の絵の具を覆って描く。
    OverwritePaint,
    /// ブラシサイズ。
    BrushSize,
    /// 絵の具の量。
    PaintAmount,
    /// 不透明度。
    Opacity,
    /// まっすぐな線を引く。
    StraightLine,
    /// 押した所を軸に、滑らかに回す。
    SmoothRotate,
    /// 筆の形：丸筆・平筆・フィルバート。
    RoundTip,
    FlatTip,
    FilbertTip,
}

/// `r` の中にコマンドの絵柄を描く。座標は `r` の中の 0〜1。
pub fn draw(pixmap: &mut Pixmap, r: Rect, icon: Icon, color: Rgba8) {
    let size = r.width() as f32;
    let map = |(u, v): (f32, f32)| (r.x0 as f32 + u * size, r.y0 as f32 + v * size);
    let stroke = (size * 0.08).max(1.5);
    let px = |t: f32| r.x0 + (t * size).round() as i32;
    let py = |t: f32| r.y0 + (t * size).round() as i32;
    match icon {
        Icon::RotateLeft | Icon::RotateRight => {
            // 4 分の 3 周の円と、その端の矢じり。右回転は左右反転
            let flip = |(u, v): (f32, f32)| if icon == Icon::RotateRight { (1.0 - u, v) } else { (u, v) };
            let points: Vec<(f32, f32)> =
                arc_points((0.5, 0.55), 0.26, -FRAC_PI_2, PI).into_iter().map(|p| map(flip(p))).collect();
            polyline(pixmap, &points, stroke, color);
            let head = [(0.5, 0.15), (0.5, 0.43), (0.32, 0.29)].map(|p| map(flip(p)));
            pixmap.fill_polygon(&head, color);
        }
        Icon::ResetRotation => {
            // まっすぐ立てた紙と、上向きの矢印
            let paper = [(0.3, 0.18), (0.7, 0.18), (0.7, 0.84), (0.3, 0.84), (0.3, 0.18)].map(map);
            polyline(pixmap, &paper, stroke, color);
            pixmap.line(map((0.5, 0.36)), map((0.5, 0.7)), stroke, color);
            let head = [(0.38, 0.42), (0.62, 0.42), (0.5, 0.28)].map(map);
            pixmap.fill_polygon(&head, color);
        }
        Icon::ZoomOut | Icon::ZoomIn => {
            // 虫めがねの中に − か +
            let lens: Vec<(f32, f32)> = arc_points((0.42, 0.42), 0.24, 0.0, 2.0 * PI).into_iter().map(map).collect();
            polyline(pixmap, &lens, stroke, color);
            pixmap.line(map((0.6, 0.6)), map((0.84, 0.84)), stroke * 1.6, color);
            pixmap.line(map((0.3, 0.42)), map((0.54, 0.42)), stroke, color);
            if icon == Icon::ZoomIn {
                pixmap.line(map((0.42, 0.3)), map((0.42, 0.54)), stroke, color);
            }
        }
        Icon::Fit => {
            // 四隅のかぎ括弧（画面いっぱいに合わせる）
            for (cx, cy, dx, dy) in
                [(0.2, 0.2, 1.0, 1.0), (0.8, 0.2, -1.0, 1.0), (0.2, 0.8, 1.0, -1.0), (0.8, 0.8, -1.0, -1.0)]
            {
                let corner = [(cx + dx * 0.22, cy), (cx, cy), (cx, cy + dy * 0.22)].map(map);
                polyline(pixmap, &corner, stroke, color);
            }
            let inner = [(0.38, 0.38), (0.62, 0.38), (0.62, 0.62), (0.38, 0.62), (0.38, 0.38)].map(map);
            polyline(pixmap, &inner, stroke * 0.7, color);
        }
        Icon::AddTracingPaper | Icon::AddClearFilm => {
            // 少しずらして重ねた 2 枚の紙と、右下の +
            let back = Rect::new(px(0.16), py(0.14), px(0.66), py(0.7));
            pixmap.frame(back, color);
            let front = Rect::new(px(0.3), py(0.28), px(0.8), py(0.84));
            let fill = match icon {
                Icon::AddTracingPaper => Rgba8 { r: 236, g: 236, b: 226, a: 255 },
                _ => Rgba8 { r: 214, g: 228, b: 238, a: 120 },
            };
            pixmap.fill_rect(front, fill);
            pixmap.frame(front, color);
            pixmap.line(map((0.55, 0.56)), map((0.55, 0.84)), stroke, color);
            pixmap.line(map((0.41, 0.7)), map((0.69, 0.7)), stroke, color);
        }
        Icon::MoveUp | Icon::MoveDown => {
            let flip = |(u, v): (f32, f32)| if icon == Icon::MoveDown { (u, 1.0 - v) } else { (u, v) };
            pixmap.line(map(flip((0.5, 0.36))), map(flip((0.5, 0.82))), stroke * 1.2, color);
            let head = [(0.24, 0.46), (0.76, 0.46), (0.5, 0.16)].map(|p| map(flip(p)));
            pixmap.fill_polygon(&head, color);
        }
        Icon::RemoveSheet => {
            // ごみ箱
            let lid = [(0.2, 0.26), (0.8, 0.26)].map(map);
            pixmap.line(lid[0], lid[1], stroke, color);
            pixmap.line(map((0.4, 0.18)), map((0.6, 0.18)), stroke, color);
            let can = [(0.28, 0.32), (0.34, 0.84), (0.66, 0.84), (0.72, 0.32)].map(map);
            polyline(pixmap, &can, stroke, color);
            for u in [0.43, 0.57] {
                pixmap.line(map((u, 0.42)), map((u, 0.74)), stroke * 0.7, color);
            }
        }
        Icon::MixPaint => {
            // 赤と青の丸が重なって、重なりが紫になる
            pixmap.fill_circle(px(0.38) as f32, py(0.5) as f32, size * 0.24, Rgba8 { r: 210, g: 60, b: 50, a: 255 });
            pixmap.fill_circle(px(0.62) as f32, py(0.5) as f32, size * 0.24, Rgba8 { r: 50, g: 90, b: 200, a: 200 });
        }
        Icon::OverwritePaint => {
            // 赤い丸の上を、青い四角が覆う
            pixmap.fill_circle(px(0.36) as f32, py(0.5) as f32, size * 0.24, Rgba8 { r: 210, g: 60, b: 50, a: 255 });
            pixmap.fill_rect(Rect::new(px(0.44), py(0.24), px(0.84), py(0.76)), Rgba8 { r: 50, g: 90, b: 200, a: 255 });
        }
        Icon::BrushSize => {
            // 大きさの違う 3 つの点
            for (u, radius) in [(0.2, 0.06), (0.42, 0.1), (0.72, 0.17)] {
                pixmap.fill_circle(map((u, 0.5)).0, map((u, 0.5)).1, size * radius, color);
            }
        }
        Icon::PaintAmount => {
            // しずく
            let drop: Vec<(f32, f32)> =
                std::iter::once((0.5, 0.12)).chain(arc_points((0.5, 0.6), 0.25, -0.35, PI + 0.35)).map(map).collect();
            pixmap.fill_polygon(&drop, color);
        }
        Icon::Opacity => {
            // 市松模様の上に、半分透けた四角
            let cell = (size * 0.18).max(1.0);
            for row in 0..4 {
                for column in 0..4 {
                    if (row + column) % 2 == 0 {
                        let x = r.x0 as f32 + size * 0.14 + column as f32 * cell;
                        let y = r.y0 as f32 + size * 0.14 + row as f32 * cell;
                        let square = Rect::new(x as i32, y as i32, (x + cell) as i32, (y + cell) as i32);
                        pixmap.fill_rect(square, Rgba8 { r: 150, g: 150, b: 150, a: 255 });
                    }
                }
            }
            let glass = Rgba8 { r: color.r / 2, g: color.g / 2, b: color.b / 2, a: color.a / 2 };
            pixmap.fill_rect(Rect::new(px(0.36), py(0.36), px(0.86), py(0.86)), glass);
            pixmap.frame(Rect::new(px(0.36), py(0.36), px(0.86), py(0.86)), color);
        }
        Icon::StraightLine => {
            // 定規の上を通る線と、両端の点
            pixmap.line(map((0.18, 0.78)), map((0.82, 0.22)), stroke, color);
            pixmap.fill_circle(map((0.18, 0.78)).0, map((0.18, 0.78)).1, size * 0.08, color);
            pixmap.fill_circle(map((0.82, 0.22)).0, map((0.82, 0.22)).1, size * 0.08, color);
        }
        Icon::SmoothRotate => {
            // 軸の点と、そのまわりを回る矢印
            pixmap.fill_circle(map((0.5, 0.5)).0, map((0.5, 0.5)).1, size * 0.08, color);
            let points: Vec<(f32, f32)> =
                arc_points((0.5, 0.5), 0.3, -PI * 0.9, PI * 0.4).into_iter().map(map).collect();
            polyline(pixmap, &points, stroke, color);
            let head = [(0.62, 0.86), (0.78, 0.66), (0.56, 0.64)].map(map);
            pixmap.fill_polygon(&head, color);
        }
        Icon::RoundTip => {
            pixmap.fill_circle(map((0.5, 0.5)).0, map((0.5, 0.5)).1, size * 0.3, color);
        }
        Icon::FlatTip | Icon::FilbertTip => {
            // 筆と同じく、右上がりに傾けた平たい跡
            let (half_long, half_thick) = if icon == Icon::FlatTip { (0.36, 0.11) } else { (0.36, 0.18) };
            let angle = -70.0f32.to_radians();
            let (c, s) = (angle.cos(), angle.sin());
            let points: Vec<(f32, f32)> = (0..32)
                .map(|i| {
                    let t = i as f32 / 32.0 * 2.0 * PI;
                    let (u, v) = if icon == Icon::FlatTip {
                        // 角の丸い長方形
                        let (ct, st) = (t.cos(), t.sin());
                        (ct.signum() * ct.abs().powf(1.0 / 3.0), st.signum() * st.abs().powf(1.0 / 3.0))
                    } else {
                        (t.cos(), t.sin())
                    };
                    let (along, across) = (u * half_long, v * half_thick);
                    map((0.5 + along * c - across * s, 0.5 + along * s + across * c))
                })
                .collect();
            pixmap.fill_polygon(&points, color);
        }
    }
}

/// 中心 `c`・半径 `radius` の円弧を、角度 `from` から `to` まで（画面座標なので正は時計回り）点に分ける。
fn arc_points(c: (f32, f32), radius: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    let steps = 24;
    (0..=steps)
        .map(|i| {
            let t = from + (to - from) * i as f32 / steps as f32;
            (c.0 + radius * t.cos(), c.1 + radius * t.sin())
        })
        .collect()
}

fn polyline(pixmap: &mut Pixmap, points: &[(f32, f32)], width: f32, color: Rgba8) {
    for pair in points.windows(2) {
        pixmap.line(pair[0], pair[1], width, color);
    }
    // 継ぎ目が欠けないよう、点ごとに丸を置く
    for &(x, y) in points {
        pixmap.fill_circle(x, y, width * 0.5, color);
    }
}
