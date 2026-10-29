//! primitivepaint のドキュメントモデルと描画処理。
//!
//! GPU やウィンドウに依存しない部分をここに集め、単体テストできるようにしている。
//! ピクセルはすべて premultiplied alpha の RGBA8 で持つ。

pub mod color_chart;
pub mod document;
pub mod geom;
pub mod kneaded_eraser;
pub mod layer;
pub mod mode;
pub mod paint_supply;
pub mod palette;
pub mod pen;
pub mod pencil;
pub mod pigment;
pub mod pressure;
pub mod straight_line;
pub mod stroke;
pub mod stroke_buffer;
pub mod tile;
pub mod watercolor;

pub use color_chart::ColorChart;
pub use document::Document;
pub use geom::Rect;
pub use kneaded_eraser::KneadedEraser;
pub use layer::{BlendMode, Layer, LayerSnapshot, SheetMaterial};
pub use mode::CanvasMode;
pub use paint_supply::PaintSupply;
pub use palette::{Palette, PaletteBoard, PaletteBrush, PaletteKnife};
pub use pen::{DabMode, Eraser, InkPen, InputSample, OilPen, PaintMode, Pen, RoundTip, TipShape};
pub use pencil::Pencil;
pub use pigment::Pigment;
pub use pressure::PressureCurve;
pub use straight_line::StraightLine;
pub use stroke::StrokeEngine;
pub use tile::{TILE_SIZE, Tile};
pub use watercolor::Watercolor;

/// premultiplied alpha の RGBA8 ピクセル。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// ブラシ色などに使う直線（非 premultiplied）の色。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
}
