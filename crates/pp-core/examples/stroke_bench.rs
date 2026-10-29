//! 筆ごとに、太い線を 1 本引く速さを測る。`cargo run --release -p pp-core --example stroke_bench`

use std::time::Instant;

use pp_core::{
    Color, Eraser, InkPen, InputSample, KneadedEraser, Layer, OilPen, PaintMode, Pen, Pencil, Rect, Rgba8,
    StrokeEngine, Watercolor,
};

fn pen(name: &str) -> Box<dyn Pen> {
    match name {
        "インクペン" => Box::new(InkPen::default()),
        "鉛筆" => Box::new(Pencil::default()),
        "油彩" => Box::new(OilPen::default()),
        "油彩上書き" => {
            let mut pen = OilPen::default();
            pen.set_paint_mode(PaintMode::Overwrite);
            Box::new(pen)
        }
        "水彩筆" => Box::new(Watercolor::default()),
        "消しゴム" => Box::new(Eraser::default()),
        _ => Box::new(KneadedEraser::default()),
    }
}

fn main() {
    let clip = Rect::from_size(3000, 2000);
    for size in [30.0, 300.0] {
        for name in ["インクペン", "鉛筆", "油彩", "油彩上書き", "水彩筆", "消しゴム", "練り消し"]
        {
            let mut pen = pen(name);
            pen.set_size(size);
            pen.set_color(Color { r: 0.7, g: 0.2, b: 0.1, a: 1.0 });
            // 消す道具のために、先に塗っておく
            let mut layer = Layer::new("bench");
            layer.fill(clip, Rgba8 { r: 40, g: 80, b: 160, a: 255 });
            let start = Instant::now();
            let mut engine = StrokeEngine::new();
            engine.begin(&mut layer, pen.as_mut(), InputSample { x: 200.0, y: 300.0, pressure: 1.0 }, clip);
            engine.extend(&mut layer, pen.as_mut(), InputSample { x: 2600.0, y: 1500.0, pressure: 1.0 }, clip);
            engine.end();
            println!("{name:　<6} 太さ {size:>5}: {:>8.1} ms", start.elapsed().as_secs_f64() * 1000.0);
        }
    }
}
