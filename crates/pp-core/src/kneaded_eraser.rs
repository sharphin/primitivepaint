use crate::document::dst_out;
use crate::layer::Layer;
use crate::mode::CanvasMode;
use crate::pen::{InputSample, Pen, RoundTip};
use crate::stroke_buffer::{StrokeBuffer, ceiling};
use crate::{Color, Rect};

/// 1 回こするごとに取れる割合（筆圧 1 のとき）。
const LIFT: f32 = 0.4;

/// 練り消し。1 回こするごとに少しずつ取れるので、下描きを薄くしたり、明るいところを抜いたりできる。
///
/// 1 ストロークの中では何度往復しても取れる量は同じで、ストロークを重ねるごとに薄くなる。
/// 強く押すほどよく取れる。
pub struct KneadedEraser {
    pub tip: RoundTip,
    stroke: Option<StrokeBuffer>,
}

impl Default for KneadedEraser {
    fn default() -> Self {
        KneadedEraser { tip: RoundTip { size: 30.0, hardness: 0.2, ..RoundTip::default() }, stroke: None }
    }
}

impl Pen for KneadedEraser {
    fn name(&self) -> &str {
        "練り消し"
    }

    /// 油彩の絵の具は練り消しでは取れない。
    fn usable_on(&self, mode: CanvasMode) -> bool {
        mode == CanvasMode::Paper
    }

    fn size(&self) -> f32 {
        self.tip.size
    }

    fn set_size(&mut self, size: f32) {
        self.tip.size = size;
    }

    fn set_color(&mut self, _color: Color) {}

    fn color(&self) -> Option<Color> {
        None
    }

    fn spacing(&self, pressure: f32) -> f32 {
        self.tip.spacing_px(pressure)
    }

    fn begin_stroke(&mut self, layer: &Layer) {
        self.stroke = Some(StrokeBuffer::new(layer));
    }

    fn dab(&mut self, layer: &mut Layer, sample: InputSample, clip: Rect) -> Rect {
        let KneadedEraser { tip, stroke } = self;
        let buffer = stroke.get_or_insert_with(|| StrokeBuffer::new(layer));
        let strength = LIFT * sample.pressure.clamp(0.0, 1.0);
        let amount = |_x: i32, _y: i32, coverage: f32| (coverage * strength * 255.0 + 0.5) as u32;
        buffer.cover_dab(layer, tip, sample, clip, ceiling(strength), amount, |below, a| dst_out(below, a as u32))
    }

    fn erases(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pen::InkPen;
    use crate::stroke::StrokeEngine;

    fn rub(layer: &mut Layer, eraser: &mut KneadedEraser) {
        let clip = Rect::from_size(200, 200);
        let mut engine = StrokeEngine::new();
        engine.begin(layer, eraser, InputSample { x: 60.0, y: 100.0, pressure: 1.0 }, clip);
        engine.extend(layer, eraser, InputSample { x: 140.0, y: 100.0, pressure: 1.0 }, clip);
        // 同じストロークの中で戻っても、それ以上は取れない
        engine.extend(layer, eraser, InputSample { x: 60.0, y: 100.0, pressure: 1.0 }, clip);
        engine.end();
    }

    #[test]
    fn each_stroke_lifts_a_little_more() {
        let mut layer = Layer::new("t");
        let clip = Rect::from_size(200, 200);
        let mut ink = InkPen { tip: RoundTip { size: 120.0, hardness: 1.0, ..RoundTip::default() } };
        ink.dab(&mut layer, InputSample { x: 100.0, y: 100.0, pressure: 1.0 }, clip);

        let mut eraser = KneadedEraser::default();
        eraser.tip.hardness = 1.0;
        rub(&mut layer, &mut eraser);
        let once = layer.pixel(100, 100).a;
        assert!((140..=165).contains(&once), "1 回で 4 割ほど取れる: {once}");
        rub(&mut layer, &mut eraser);
        let twice = layer.pixel(100, 100).a;
        assert!(twice < once - 40 && twice > 0, "重ねるとさらに薄くなる: {twice}");
    }
}
