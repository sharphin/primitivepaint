//! ペンの入力イベント 1 回ごとの重さを、画面に出す合成まで含めて測る。
//! `cargo run --release -p pp-core --example event_bench`

use std::time::Instant;

use pp_core::{CanvasMode, Color, Document, InputSample, OilPen, PaintMode, Pen, Rgba8, StrokeEngine};

fn main() {
    for size in [100.0, 300.0, 600.0] {
        for mode in PaintMode::ALL {
            let mut doc = Document::new(4000, 3500, CanvasMode::OilCanvas);
            let clip = doc.bounds();
            let mut pen = OilPen::default();
            pen.set_size(size);
            pen.set_paint_mode(mode);
            pen.set_paint_amount(1.0);
            pen.set_color(Color { r: 0.7, g: 0.2, b: 0.1, a: 1.0 });
            let mut out = Vec::new();
            let (mut paint, mut show, mut worst) = (0.0f64, 0.0f64, 0.0f64);
            // 2 本引く。2 本目は 1 本目の絵の具の上を通る
            for pass in 0..2 {
                let mut engine = StrokeEngine::new();
                let y = 1500.0 + pass as f32 * 100.0;
                for i in 0..500 {
                    let s = InputSample { x: 500.0 + i as f32 * 6.0, y, pressure: 1.0 };
                    let start = Instant::now();
                    let layer = doc.active_layer_mut();
                    let dirty = if i == 0 {
                        engine.begin(layer, &mut pen, s, clip)
                    } else {
                        engine.extend(layer, &mut pen, s, clip)
                    };
                    let painted = start.elapsed().as_secs_f64();
                    let region = dirty.intersect(&clip);
                    out.resize((region.width() * region.height()) as usize, Rgba8::default());
                    if !region.is_empty() {
                        doc.composite(region, &mut out);
                    }
                    let total = start.elapsed().as_secs_f64();
                    paint += painted;
                    show += total - painted;
                    worst = worst.max(total);
                }
                engine.end();
            }
            println!(
                "太さ {size:>4} {:　<3}: 描く {:>7.1} ms  合成 {:>6.1} ms  1 回の最大 {:>5.2} ms",
                mode.name(),
                paint * 1000.0,
                show * 1000.0,
                worst * 1000.0
            );
        }
    }
}
