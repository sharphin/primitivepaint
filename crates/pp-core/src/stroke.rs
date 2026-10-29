use crate::Rect;
use crate::layer::Layer;
use crate::pen::{InputSample, Pen};

/// 入力サンプルの列を一定間隔のダブ（スタンプ）に変換してレイヤーへ描く。
#[derive(Default)]
pub struct StrokeEngine {
    last: Option<InputSample>,
    /// 前回のダブから進んだ距離の繰り越し。
    carry: f32,
}

impl StrokeEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.last.is_some()
    }

    /// ストローク開始。最初のダブを打ち、変更範囲を返す。
    pub fn begin(&mut self, layer: &mut Layer, pen: &mut dyn Pen, s: InputSample, clip: Rect) -> Rect {
        self.last = Some(s);
        self.carry = 0.0;
        pen.begin_stroke(layer);
        pen.dab(layer, s, clip)
    }

    /// 前回のサンプルから `s` まで線形補間してダブを打つ。
    pub fn extend(&mut self, layer: &mut Layer, pen: &mut dyn Pen, s: InputSample, clip: Rect) -> Rect {
        let Some(prev) = self.last else {
            return self.begin(layer, pen, s, clip);
        };
        let dx = s.x - prev.x;
        let dy = s.y - prev.y;
        let dist = (dx * dx + dy * dy).sqrt();
        let mut dirty = Rect::EMPTY;
        if dist > 0.0 {
            let mut t = self.carry;
            loop {
                let step = pen.spacing(lerp(prev, s, t / dist).pressure);
                t += step;
                if t > dist {
                    self.carry = t - step - dist;
                    break;
                }
                dirty = dirty.union(&pen.dab(layer, lerp(prev, s, t / dist), clip));
            }
        }
        self.last = Some(s);
        dirty
    }

    pub fn end(&mut self) {
        self.last = None;
        self.carry = 0.0;
    }
}

fn lerp(a: InputSample, b: InputSample, t: f32) -> InputSample {
    InputSample {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        pressure: a.pressure + (b.pressure - a.pressure) * t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pen::{InkPen, RoundTip};

    fn sample(x: f32, y: f32) -> InputSample {
        InputSample { x, y, pressure: 1.0 }
    }

    fn hard_pen(size: f32) -> InkPen {
        InkPen { tip: RoundTip { size, hardness: 1.0, ..Default::default() } }
    }

    #[test]
    fn dab_paints_center_and_reports_dirty_rect() {
        let mut layer = Layer::new("t");
        let clip = Rect::from_size(100, 100);
        let dirty = StrokeEngine::new().begin(&mut layer, &mut hard_pen(10.0), sample(50.0, 50.0), clip);
        assert_eq!(layer.pixel(50, 50).a, 255);
        assert_eq!(layer.pixel(70, 50).a, 0);
        assert!(dirty.x0 <= 45 && dirty.x1 >= 55);
    }

    #[test]
    fn stroke_is_continuous_across_tile_boundary() {
        let mut layer = Layer::new("t");
        let mut pen = hard_pen(6.0);
        let clip = Rect::from_size(1000, 1000);
        let mut engine = StrokeEngine::new();
        engine.begin(&mut layer, &mut pen, sample(200.0, 100.5), clip);
        engine.extend(&mut layer, &mut pen, sample(320.0, 100.5), clip);
        engine.end();
        for x in 200..=320 {
            assert_eq!(layer.pixel(x, 100).a, 255, "gap at x={x}");
        }
        assert_eq!(layer.tile_count(), 2);
    }

    #[test]
    fn clip_prevents_painting_outside_document() {
        let mut layer = Layer::new("t");
        let dirty = StrokeEngine::new().begin(
            &mut layer,
            &mut InkPen::default(),
            sample(-50.0, -50.0),
            Rect::from_size(10, 10),
        );
        assert!(dirty.is_empty());
        assert_eq!(layer.tile_count(), 0);
    }
}
