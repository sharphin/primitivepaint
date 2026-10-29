use crate::document::{mul255, src_over};
use crate::layer::Layer;
use crate::mode::CanvasMode;
use crate::pen::{InputSample, Pen, RoundTip};
use crate::stroke_buffer::{StrokeBuffer, ceiling};
use crate::{Color, Rect, Rgba8};

/// 鉛筆の芯の色（黒鉛）。鉛筆はパレットの絵の具を付けないので固定。
const GRAPHITE: Color = Color { r: 0.2, g: 0.2, b: 0.22, a: 1.0 };

/// 鉛筆。紙目（紙の表面の凹凸）の山にだけ芯が乗るので、軽く描くとざらつき、強く押すと埋まる。
///
/// 紙目は紙に固定した模様なので、同じ場所を何度もなぞると少しずつ濃くなる。
/// 1 ストロークの中ではダブが重なっても濃くならない。
pub struct Pencil {
    pub tip: RoundTip,
    stroke: Option<StrokeBuffer>,
}

impl Default for Pencil {
    fn default() -> Self {
        Pencil {
            tip: RoundTip { size: 5.0, hardness: 0.5, color: GRAPHITE, opacity: 0.9, ..RoundTip::default() },
            stroke: None,
        }
    }
}

impl Pen for Pencil {
    fn name(&self) -> &str {
        "鉛筆"
    }

    fn usable_on(&self, _mode: CanvasMode) -> bool {
        true
    }

    fn size(&self) -> f32 {
        self.tip.size
    }

    fn set_size(&mut self, size: f32) {
        self.tip.size = size;
    }

    fn set_color(&mut self, _color: Color) {}

    fn color(&self) -> Option<Color> {
        Some(self.tip.color)
    }

    fn spacing(&self, pressure: f32) -> f32 {
        self.tip.spacing_px(pressure)
    }

    fn begin_stroke(&mut self, layer: &Layer) {
        self.stroke = Some(StrokeBuffer::new(layer));
    }

    fn dab(&mut self, layer: &mut Layer, sample: InputSample, clip: Rect) -> Rect {
        let Pencil { tip, stroke } = self;
        let buffer = stroke.get_or_insert_with(|| StrokeBuffer::new(layer));
        let pressure = sample.pressure.clamp(0.0, 1.0);
        let opacity = tip.opacity;
        let (r, g, b) = (channel(tip.color.r), channel(tip.color.g), channel(tip.color.b));
        let amount = |x: i32, y: i32, coverage: f32| {
            // 筆圧が紙目の高さを超えた分だけ芯が乗る
            let bite = ((pressure - paper_grain(x, y)) * 3.0 + 0.35).clamp(0.0, 1.0);
            (coverage * bite * opacity * 255.0 + 0.5) as u32
        };
        buffer.cover_dab(layer, tip, sample, clip, ceiling(opacity), amount, |below, a| {
            let a = a as u32;
            let lead = Rgba8 { r: mul255(r, a) as u8, g: mul255(g, a) as u8, b: mul255(b, a) as u8, a: a as u8 };
            src_over(below, lead)
        })
    }
}

fn channel(v: f32) -> u32 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u32
}

/// 紙目の高さ（0..=1）。座標から決まるので、何度なぞっても同じ模様になる。
pub(crate) fn paper_grain(x: i32, y: i32) -> f32 {
    let fine = hash(x, y);
    // 4px 周期の粗い凹凸を双線形補間でなめらかにする
    let (cx, cy) = (x.div_euclid(4), y.div_euclid(4));
    let (fx, fy) = (x.rem_euclid(4) as f32 / 4.0, y.rem_euclid(4) as f32 / 4.0);
    let top = hash(cx, cy) * (1.0 - fx) + hash(cx + 1, cy) * fx;
    let bottom = hash(cx, cy + 1) * (1.0 - fx) + hash(cx + 1, cy + 1) * fx;
    let coarse = top * (1.0 - fy) + bottom * fy;
    0.35 * fine + 0.65 * coarse
}

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::StrokeEngine;

    fn draw_line(layer: &mut Layer, pencil: &mut Pencil, pressure: f32) {
        let clip = Rect::from_size(400, 100);
        let mut engine = StrokeEngine::new();
        engine.begin(layer, pencil, InputSample { x: 20.0, y: 50.0, pressure }, clip);
        engine.extend(layer, pencil, InputSample { x: 380.0, y: 50.0, pressure }, clip);
        engine.end();
    }

    fn mean_alpha(layer: &Layer) -> f32 {
        (40..360).map(|x| layer.pixel(x, 50).a as f32).sum::<f32>() / 320.0
    }

    #[test]
    fn light_pressure_is_grainy_and_heavy_pressure_fills() {
        let mut light = Layer::new("t");
        draw_line(&mut light, &mut Pencil::default(), 0.3);
        let mut heavy = Layer::new("t");
        draw_line(&mut heavy, &mut Pencil::default(), 1.0);
        assert!(mean_alpha(&heavy) > mean_alpha(&light) + 40.0);
        // 軽い線には芯が乗らない画素（紙目の谷）が残る
        assert!((40..360).any(|x| light.pixel(x, 50).a < 30));
    }

    #[test]
    fn going_over_again_darkens() {
        let mut layer = Layer::new("t");
        let mut pencil = Pencil::default();
        draw_line(&mut layer, &mut pencil, 0.5);
        let once = mean_alpha(&layer);
        draw_line(&mut layer, &mut pencil, 0.5);
        assert!(mean_alpha(&layer) > once + 10.0);
    }

    #[test]
    fn grain_is_stable_and_in_range() {
        for (x, y) in [(0, 0), (-5, 7), (1000, -3)] {
            let g = paper_grain(x, y);
            assert_eq!(g, paper_grain(x, y));
            assert!((0.0..=1.0).contains(&g));
        }
    }
}
