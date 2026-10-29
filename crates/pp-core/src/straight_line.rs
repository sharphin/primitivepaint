use crate::Rect;
use crate::layer::Layer;
use crate::pen::{InputSample, Pen};
use crate::stroke::StrokeEngine;

/// 押した位置から離した位置までの、まっすぐな線。
///
/// 引いている間は端の位置だけを覚えておき、離したときに 1 本のストロークとして一度だけ描く。
/// 動かすたびに描き直すと、太い筆では重くなるため。
///
/// 筆圧は、引いている間で一番強かったものを線全体に使う。ペンは押し始めと離す瞬間に筆圧が弱くなるので、
/// 端の筆圧を使うと線が細くなってしまう。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StraightLine {
    start: InputSample,
    end: InputSample,
    pressure: f32,
}

impl StraightLine {
    pub fn new(start: InputSample) -> Self {
        StraightLine { start, end: start, pressure: start.pressure }
    }

    pub fn start(&self) -> InputSample {
        InputSample { pressure: self.pressure, ..self.start }
    }

    pub fn end(&self) -> InputSample {
        InputSample { pressure: self.pressure, ..self.end }
    }

    /// 引いている間で一番強かった筆圧。
    pub fn pressure(&self) -> f32 {
        self.pressure
    }

    pub fn set_end(&mut self, end: InputSample) {
        self.end = end;
        self.pressure = self.pressure.max(end.pressure);
    }

    /// `layer` に描き、変更した範囲を返す。
    pub fn draw(&self, layer: &mut Layer, pen: &mut dyn Pen, clip: Rect) -> Rect {
        let mut engine = StrokeEngine::new();
        let first = engine.begin(layer, pen, self.start(), clip);
        let dirty = first.union(&engine.extend(layer, pen, self.end(), clip));
        engine.end();
        dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pen::{InkPen, RoundTip};

    fn at(x: f32, y: f32) -> InputSample {
        InputSample { x, y, pressure: 1.0 }
    }

    #[test]
    fn draws_from_the_start_to_the_last_end() {
        let clip = Rect::from_size(200, 200);
        let mut layer = Layer::new("t");
        let mut pen = InkPen { tip: RoundTip { size: 4.0, hardness: 1.0, ..RoundTip::default() } };
        let mut line = StraightLine::new(at(10.0, 10.0));
        line.set_end(at(190.0, 10.0));
        line.set_end(at(10.0, 190.0));
        let dirty = line.draw(&mut layer, &mut pen, clip);
        assert_eq!(layer.pixel(100, 10).a, 0, "途中の位置には描かない");
        assert_eq!(layer.pixel(10, 100).a, 255, "最後の位置まで描く");
        assert!(dirty.contains(10.0, 100.0), "{dirty:?}");
    }

    #[test]
    fn uses_the_strongest_pressure_for_the_whole_line() {
        let clip = Rect::from_size(300, 100);
        let mut layer = Layer::new("t");
        let mut pen = InkPen { tip: RoundTip { size: 40.0, hardness: 1.0, ..RoundTip::default() } };
        let mut line = StraightLine::new(InputSample { x: 20.0, y: 50.0, pressure: 0.1 });
        line.set_end(InputSample { x: 150.0, y: 50.0, pressure: 1.0 });
        // 離す直前は筆圧が弱くなる
        line.set_end(InputSample { x: 280.0, y: 50.0, pressure: 0.05 });
        assert_eq!(line.pressure(), 1.0);
        line.draw(&mut layer, &mut pen, clip);
        assert_eq!(layer.pixel(25, 68).a, 255, "始点も筆の太さどおり");
        assert_eq!(layer.pixel(275, 68).a, 255, "終点も筆の太さどおり");
    }
}
