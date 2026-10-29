use crate::layer::Layer;
use crate::mode::CanvasMode;
use crate::paint_supply::PaintSupply;
use crate::pen::{InputSample, MIN_PAINT_OPACITY, Pen, RoundTip, TipShape};
use crate::stroke_buffer::{StrokeBuffer, ceiling};
use crate::{Color, Rect, Rgba8};

/// 水彩筆。透明な絵の具なので下の色が透け、重ねると色が掛け合わさって濃くなる（黄の上に青で緑）。
///
/// 1 ストロークの中では濃さが一定で、乾いてから重ねたように、ストロークごとに重なる。
/// 絵の具の量が少ないと、描くうちにかすれていく。不透明度は 1 ストロークで付く濃さ。
/// 紙モードでだけ使える。
pub struct Watercolor {
    pub tip: RoundTip,
    supply: PaintSupply,
    stroke: Option<StrokeBuffer>,
}

impl Default for Watercolor {
    fn default() -> Self {
        Watercolor {
            tip: RoundTip {
                size: 36.0,
                hardness: 0.3,
                color: Color { r: 0.12, g: 0.18, b: 0.62, a: 1.0 },
                opacity: 0.45,
                ..RoundTip::default()
            },
            supply: PaintSupply::default(),
            stroke: None,
        }
    }
}

impl Pen for Watercolor {
    fn name(&self) -> &str {
        "水彩筆"
    }

    fn usable_on(&self, mode: CanvasMode) -> bool {
        mode == CanvasMode::Paper
    }

    fn size(&self) -> f32 {
        self.tip.size
    }

    fn set_size(&mut self, size: f32) {
        self.tip.size = size;
    }

    fn set_color(&mut self, color: Color) {
        self.tip.color = color;
    }

    fn color(&self) -> Option<Color> {
        Some(self.tip.color)
    }

    fn spacing(&self, pressure: f32) -> f32 {
        self.tip.spacing_px(pressure)
    }

    fn takes_paint(&self) -> bool {
        true
    }

    fn begin_stroke(&mut self, layer: &Layer) {
        self.supply.refill();
        self.stroke = Some(StrokeBuffer::new(layer));
    }

    fn dab(&mut self, layer: &mut Layer, sample: InputSample, clip: Rect) -> Rect {
        let Watercolor { tip, supply, stroke } = self;
        let buffer = stroke.get_or_insert_with(|| StrokeBuffer::new(layer));
        let opacity = tip.opacity * tip.color.a * supply.use_paint(sample);
        if opacity <= 0.0 {
            return Rect::EMPTY;
        }
        let color = [tip.color.r, tip.color.g, tip.color.b];
        let amount = |_x: i32, _y: i32, coverage: f32| (coverage * opacity * 255.0 + 0.5) as u32;
        buffer.cover_dab(layer, tip, sample, clip, ceiling(opacity), amount, |below, a| {
            glaze(below, color, a as f32 / 255.0)
        })
    }

    fn paint_amount(&self) -> Option<f32> {
        Some(self.supply.amount())
    }

    fn set_paint_amount(&mut self, amount: f32) {
        self.supply.set_amount(amount);
    }

    fn opacity(&self) -> Option<f32> {
        Some(self.tip.opacity)
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.tip.opacity = opacity.clamp(MIN_PAINT_OPACITY, 1.0);
    }

    fn tip_shape(&self) -> Option<TipShape> {
        Some(self.tip.shape)
    }

    fn set_tip_shape(&mut self, shape: TipShape) {
        self.tip.shape = shape;
    }
}

/// 透明な絵の具を `strength`（0..=1）の濃さで重ねる。
///
/// 白い紙の上で見たときに「下の見た目 × 絵の具の透過色」になるよう、色と不透明度を決める。
fn glaze(below: Rgba8, color: [f32; 3], strength: f32) -> Rgba8 {
    let below_alpha = below.a as f32 / 255.0;
    let under = [below.r, below.g, below.b].map(|c| c as f32 / 255.0 + (1.0 - below_alpha));
    let target: [f32; 3] = std::array::from_fn(|i| under[i] * (1.0 - strength * (1.0 - color[i])));
    let darkest = target.iter().copied().fold(1.0f32, f32::min);
    let alpha = (1.0 - (1.0 - below_alpha) * (1.0 - strength)).max(1.0 - darkest).clamp(0.0, 1.0);
    // 0 以上なので、round の代わりに 0.5 を足して切り捨てる（round は遅い）
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Rgba8 {
        r: to_u8(target[0] - (1.0 - alpha)),
        g: to_u8(target[1] - (1.0 - alpha)),
        b: to_u8(target[2] - (1.0 - alpha)),
        a: to_u8(alpha),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::StrokeEngine;

    /// 白い紙の上に置いたときの見た目。
    fn over_white(px: Rgba8) -> [f32; 3] {
        let a = px.a as f32 / 255.0;
        [px.r, px.g, px.b].map(|c| c as f32 / 255.0 + 1.0 - a)
    }

    fn stroke(layer: &mut Layer, brush: &mut Watercolor, color: Color, from: (f32, f32), to: (f32, f32)) {
        let clip = Rect::from_size(200, 200);
        brush.set_color(color);
        let mut engine = StrokeEngine::new();
        engine.begin(layer, brush, InputSample { x: from.0, y: from.1, pressure: 1.0 }, clip);
        engine.extend(layer, brush, InputSample { x: to.0, y: to.1, pressure: 1.0 }, clip);
        engine.end();
    }

    #[test]
    fn single_stroke_is_transparent_and_even() {
        let mut layer = Layer::new("t");
        let mut brush = Watercolor::default();
        stroke(&mut layer, &mut brush, Color { r: 0.1, g: 0.2, b: 0.8, a: 1.0 }, (20.0, 100.0), (180.0, 100.0));
        let center = layer.pixel(100, 100);
        assert!(center.a > 0 && center.a < 255, "{center:?}");
        assert_eq!(layer.pixel(60, 100), layer.pixel(140, 100));
    }

    #[test]
    fn yellow_glazed_with_blue_looks_green() {
        let mut layer = Layer::new("t");
        let mut brush = Watercolor::default();
        brush.tip.opacity = 0.8;
        let yellow = Color { r: 0.98, g: 0.85, b: 0.1, a: 1.0 };
        let blue = Color { r: 0.1, g: 0.35, b: 0.85, a: 1.0 };
        stroke(&mut layer, &mut brush, yellow, (20.0, 100.0), (180.0, 100.0));
        stroke(&mut layer, &mut brush, blue, (100.0, 20.0), (100.0, 180.0));
        let [r, g, b] = over_white(layer.pixel(100, 100));
        assert!(g > r && g > b, "{r} {g} {b}");
    }

    #[test]
    fn little_paint_fades_along_the_stroke() {
        let mut layer = Layer::new("t");
        let mut brush = Watercolor::default();
        brush.tip.size = 10.0;
        brush.set_paint_amount(0.0);
        assert_eq!(brush.paint_amount(), Some(0.0));
        let clip = Rect::from_size(1200, 200);
        brush.set_color(Color { r: 0.1, g: 0.2, b: 0.8, a: 1.0 });
        let mut engine = StrokeEngine::new();
        engine.begin(&mut layer, &mut brush, InputSample { x: 10.0, y: 100.0, pressure: 1.0 }, clip);
        engine.extend(&mut layer, &mut brush, InputSample { x: 1000.0, y: 100.0, pressure: 1.0 }, clip);
        engine.end();
        let start = layer.pixel(12, 100).a;
        let middle = layer.pixel(110, 100).a;
        assert!(start > 0 && middle < start, "{start} {middle}");
        assert_eq!(layer.pixel(900, 100).a, 0);
    }

    #[test]
    fn glaze_on_white_matches_transmittance() {
        let px = glaze(Rgba8::default(), [1.0, 0.5, 0.0], 1.0);
        let seen = over_white(px);
        assert!((seen[0] - 1.0).abs() < 0.01 && (seen[1] - 0.5).abs() < 0.01 && seen[2] < 0.01, "{seen:?}");
    }
}
