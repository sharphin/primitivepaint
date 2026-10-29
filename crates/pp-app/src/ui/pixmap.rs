use pp_core::{Rect, Rgba8};

use super::font::Font;

/// 文字を矩形のどこに置くか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// UI を描く CPU 側の画像（premultiplied RGBA8）。描画は `clip` の中だけに行う。
pub struct Pixmap {
    width: u32,
    height: u32,
    pixels: Vec<Rgba8>,
    clip: Rect,
    font: Option<Font>,
}

impl Pixmap {
    pub fn new(font: Option<Font>) -> Self {
        Pixmap { width: 0, height: 0, pixels: Vec::new(), clip: Rect::EMPTY, font }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels = vec![Rgba8::default(); (width * height) as usize];
        self.clip = self.bounds();
    }

    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.width, self.height)
    }

    /// 以降の描画を `r` の中（画像の範囲内）に限る。
    pub fn set_clip(&mut self, r: Rect) {
        self.clip = r.intersect(&self.bounds());
    }

    /// `region` の画素を行順に取り出す。
    pub fn copy_region(&self, region: Rect) -> Vec<Rgba8> {
        let r = region.intersect(&self.bounds());
        let mut out = Vec::with_capacity((r.width() * r.height()) as usize);
        for y in r.y0..r.y1 {
            let start = (y as u32 * self.width + r.x0 as u32) as usize;
            out.extend_from_slice(&self.pixels[start..start + r.width() as usize]);
        }
        out
    }

    #[cfg(test)]
    pub fn pixel(&self, x: i32, y: i32) -> Rgba8 {
        self.pixels[(y as u32 * self.width + x as u32) as usize]
    }

    /// 1 画素に `color` を被覆率 `coverage` で重ねる。
    fn blend(&mut self, x: i32, y: i32, color: Rgba8, coverage: u8) {
        if !self.clip.contains(x as f32, y as f32) || coverage == 0 {
            return;
        }
        let px = &mut self.pixels[(y as u32 * self.width + x as u32) as usize];
        let a = color.a as u32 * coverage as u32 / 255;
        if a == 255 {
            *px = color;
            return;
        }
        let scale = |c: u8| (c as u32 * coverage as u32 / 255) as u8;
        let inv = 255 - a;
        let mix = |dst: u8, src: u8| (src as u32 + dst as u32 * inv / 255).min(255) as u8;
        *px = Rgba8 {
            r: mix(px.r, scale(color.r)),
            g: mix(px.g, scale(color.g)),
            b: mix(px.b, scale(color.b)),
            a: mix(px.a, a as u8),
        };
    }

    /// 矩形を塗りつぶす。透明色（a = 0 の `Rgba8::default()`）で塗ると穴が空く。
    pub fn fill_rect(&mut self, r: Rect, color: Rgba8) {
        let area = r.intersect(&self.clip);
        if color.a == 255 || color.a == 0 {
            for y in area.y0..area.y1 {
                let start = (y as u32 * self.width + area.x0 as u32) as usize;
                self.pixels[start..start + area.width() as usize].fill(color);
            }
            return;
        }
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                self.blend(x, y, color, 255);
            }
        }
    }

    /// 矩形の内側に 1px の枠を描く。
    pub fn frame(&mut self, r: Rect, color: Rgba8) {
        if r.is_empty() {
            return;
        }
        self.fill_rect(Rect::new(r.x0, r.y0, r.x1, r.y0 + 1), color);
        self.fill_rect(Rect::new(r.x0, r.y1 - 1, r.x1, r.y1), color);
        self.fill_rect(Rect::new(r.x0, r.y0, r.x0 + 1, r.y1), color);
        self.fill_rect(Rect::new(r.x1 - 1, r.y0, r.x1, r.y1), color);
    }

    /// 多角形を塗る（偶奇規則、1 画素を 4×4 点で標本化してアンチエイリアス）。
    pub fn fill_polygon(&mut self, points: &[(f32, f32)], color: Rgba8) {
        if points.len() < 3 {
            return;
        }
        let x0 = points.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i32;
        let x1 = points.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i32;
        let y0 = points.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
        let y1 = points.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i32;
        let area = Rect::new(x0, y0, x1, y1).intersect(&self.clip);
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let mut hits = 0u32;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = x as f32 + (sx as f32 + 0.5) / 4.0;
                        let py = y as f32 + (sy as f32 + 0.5) / 4.0;
                        hits += inside(points, px, py) as u32;
                    }
                }
                self.blend(x, y, color, (hits * 255 / 16) as u8);
            }
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: Rgba8) {
        let points: Vec<(f32, f32)> = (0..32)
            .map(|i| {
                let t = i as f32 / 32.0 * std::f32::consts::TAU;
                (cx + radius * t.cos(), cy + radius * t.sin())
            })
            .collect();
        self.fill_polygon(&points, color);
    }

    /// 太さ `width` の線分を描く。
    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), width: f32, color: Rgba8) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (nx, ny) = (-dy / len * width * 0.5, dx / len * width * 0.5);
        self.fill_polygon(
            &[(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)],
            color,
        );
    }

    /// 縦方向の中央に揃えて、`r` の中に 1 行の文字を書く。書いた幅を返す。
    /// `text` を `px` の大きさで書いたときの幅（画面のピクセル）。フォントがなければ `None`。
    pub fn measure_text(&mut self, text: &str, px: f32) -> Option<f32> {
        self.font.as_mut().map(|font| font.measure(text, px))
    }

    pub fn text(&mut self, r: Rect, text: &str, px: f32, color: Rgba8, align: Align) -> f32 {
        let Some(mut font) = self.font.take() else {
            return 0.0;
        };
        let width = font.measure(text, px);
        let (ascent, descent) = font.line_metrics(px);
        let baseline = r.y0 as f32 + (r.height() as f32 - (ascent - descent)) * 0.5 + ascent;
        let mut pen_x = match align {
            Align::Left => r.x0 as f32,
            Align::Center => r.x0 as f32 + (r.width() as f32 - width) * 0.5,
            Align::Right => r.x1 as f32 - width,
        };
        let saved_clip = self.clip;
        self.set_clip(r.intersect(&saved_clip));
        for c in text.chars() {
            let glyph = font.glyph(c, px);
            let m = glyph.metrics;
            let left = (pen_x + m.xmin as f32).round() as i32;
            let top = (baseline - (m.ymin as f32 + m.height as f32)).round() as i32;
            for gy in 0..m.height {
                for gx in 0..m.width {
                    self.blend(left + gx as i32, top + gy as i32, color, glyph.coverage[gy * m.width + gx]);
                }
            }
            pen_x += m.advance_width;
        }
        self.clip = saved_clip;
        self.font = Some(font);
        width
    }

    /// `src`（`src_size` の画像）を `dst` に最近傍で拡大縮小して貼る。
    pub fn blit_scaled(&mut self, dst: Rect, src: &[Rgba8], src_size: (u32, u32)) {
        let area = dst.intersect(&self.clip);
        for y in area.y0..area.y1 {
            let sy = ((y - dst.y0) as u64 * src_size.1 as u64 / dst.height() as u64) as u32;
            for x in area.x0..area.x1 {
                let sx = ((x - dst.x0) as u64 * src_size.0 as u64 / dst.width() as u64) as u32;
                self.pixels[(y as u32 * self.width + x as u32) as usize] = src[(sy * src_size.0 + sx) as usize];
            }
        }
    }
}

/// 偶奇規則で点が多角形の内側にあるか。
fn inside(points: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut result = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            result = !result;
        }
        j = i;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba8 = Rgba8 { r: 255, g: 0, b: 0, a: 255 };

    #[test]
    fn drawing_stays_inside_clip() {
        let mut pixmap = Pixmap::new(None);
        pixmap.resize(20, 20);
        pixmap.set_clip(Rect::new(5, 5, 10, 10));
        pixmap.fill_rect(Rect::new(0, 0, 20, 20), RED);
        assert_eq!(pixmap.pixel(7, 7), RED);
        assert_eq!(pixmap.pixel(12, 7).a, 0);
    }

    #[test]
    fn polygon_is_filled_with_soft_edges() {
        let mut pixmap = Pixmap::new(None);
        pixmap.resize(20, 20);
        pixmap.fill_circle(10.0, 10.5, 5.5, RED);
        assert_eq!(pixmap.pixel(10, 10), RED);
        assert_eq!(pixmap.pixel(1, 1).a, 0);
        let edge = pixmap.pixel(15, 10).a;
        assert!(edge > 0 && edge < 255, "{edge}");
    }

    #[test]
    fn copy_region_returns_rows() {
        let mut pixmap = Pixmap::new(None);
        pixmap.resize(8, 8);
        pixmap.fill_rect(Rect::new(2, 2, 4, 4), RED);
        let copied = pixmap.copy_region(Rect::new(2, 2, 6, 4));
        assert_eq!(copied.len(), 8);
        assert_eq!(copied[0], RED);
        assert_eq!(copied[2].a, 0);
    }
}
