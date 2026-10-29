use crate::pigment::Pigment;
use crate::{Color, Rgba8};

/// パレット上の 1 マスに載っている絵の具。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Paint {
    pigment: Pigment,
    /// 絵の具の量（厚み）。0 なら何もない。
    amount: f32,
}

/// 絵の具を出して混ぜる板。マス目ごとに絵の具の色と量を持つ。
pub struct Palette {
    width: u32,
    height: u32,
    cells: Vec<Option<Paint>>,
}

/// パレットの下地。絵の具が何も載っていない場所に見える色。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaletteBoard {
    /// 木の板（茶色）。
    #[default]
    Wood,
    /// 白い板（紙パレット・陶器の皿など）。
    White,
}

impl PaletteBoard {
    pub const ALL: [PaletteBoard; 2] = [PaletteBoard::Wood, PaletteBoard::White];

    pub fn name(self) -> &'static str {
        match self {
            PaletteBoard::Wood => "茶色（木）",
            PaletteBoard::White => "白",
        }
    }

    pub fn color(self) -> Rgba8 {
        match self {
            PaletteBoard::Wood => Rgba8 { r: 196, g: 160, b: 116, a: 255 },
            PaletteBoard::White => Rgba8 { r: 250, g: 250, b: 248, a: 255 },
        }
    }
}

impl Palette {
    pub fn new(width: u32, height: u32) -> Self {
        Palette { width, height, cells: vec![None; (width * height) as usize] }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// 保存したマスの並び（行優先、`(絵の具, 量)`）から作る。マスの数が合わなければ `None`。
    pub fn from_cells(width: u32, height: u32, cells: Vec<Option<(Pigment, f32)>>) -> Option<Self> {
        if cells.len() != (width as usize) * (height as usize) {
            return None;
        }
        let paints = cells
            .into_iter()
            .map(|cell| cell.map(|(pigment, amount)| Paint { pigment, amount: amount.clamp(0.0, 4.0) }))
            .collect();
        Some(Palette { width, height, cells: paints })
    }

    /// マスごとの絵の具（行優先、`(絵の具, 量)`）。保存に使う。
    pub fn cells(&self) -> impl Iterator<Item = Option<(Pigment, f32)>> + '_ {
        self.cells.iter().map(|cell| cell.map(|paint| (paint.pigment, paint.amount)))
    }

    /// (x, y) に半径 `radius` の `color` の絵の具をたっぷり置く。すでにある絵の具とは混ざる。
    pub fn put(&mut self, color: Color, x: f32, y: f32, radius: f32) {
        let fresh = Paint { pigment: Pigment::from_color(color), amount: 1.0 };
        for index in self.cells_within(x, y, radius) {
            self.cells[index] = Some(match self.cells[index] {
                Some(old) => combine(old, fresh),
                None => fresh,
            });
        }
    }

    /// (x, y) にある絵の具の色。何もなければ `None`。ペン先に絵の具を付けるときに使う。
    pub fn pick(&self, x: f32, y: f32) -> Option<Color> {
        let index = self.index_of(x.floor() as i32, y.floor() as i32)?;
        self.cells[index].map(|paint| paint.pigment.to_color())
    }

    /// 表示用に RGBA へ描き出す。`out` は幅 × 高さの行優先バッファ。何もない場所は `board` の色になる。
    pub fn render(&self, out: &mut [Rgba8], board: PaletteBoard) {
        let empty = board.color();
        for (px, cell) in out.iter_mut().zip(&self.cells) {
            *px = match cell {
                Some(paint) => {
                    let c = paint.pigment.to_color();
                    Rgba8 { r: to_u8(c.r), g: to_u8(c.g), b: to_u8(c.b), a: 255 }
                }
                None => empty,
            };
        }
    }

    fn index_of(&self, x: i32, y: i32) -> Option<usize> {
        let inside = x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height;
        inside.then(|| (y as u32 * self.width + x as u32) as usize)
    }

    fn cells_within(&self, x: f32, y: f32, radius: f32) -> Vec<usize> {
        let mut found = Vec::new();
        let (x0, x1) = ((x - radius).floor() as i32, (x + radius).ceil() as i32);
        let (y0, y1) = ((y - radius).floor() as i32, (y + radius).ceil() as i32);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let dx = cx as f32 + 0.5 - x;
                let dy = cy as f32 + 0.5 - y;
                if dx * dx + dy * dy <= radius * radius {
                    found.extend(self.index_of(cx, cy));
                }
            }
        }
        found
    }

    /// 範囲内の絵の具をまとめて混ぜた結果。何もなければ `None`。
    fn gather(&self, cells: &[usize]) -> Option<Paint> {
        let parts: Vec<(Pigment, f32)> =
            cells.iter().filter_map(|&i| self.cells[i]).map(|p| (p.pigment, p.amount)).collect();
        let amount = parts.iter().map(|(_, a)| a).sum::<f32>() / cells.len().max(1) as f32;
        Pigment::mix(&parts).map(|pigment| Paint { pigment, amount })
    }
}

/// パレットナイフ。パレット上をなぞって絵の具を混ぜ、伸ばす。
pub struct PaletteKnife {
    radius: f32,
    /// ナイフに付いている絵の具。
    load: Option<Paint>,
    last: Option<(f32, f32)>,
}

impl PaletteKnife {
    pub fn new(radius: f32) -> Self {
        PaletteKnife { radius, load: None, last: None }
    }

    /// なぞり始め。その場の絵の具をナイフに取る。
    pub fn begin(&mut self, palette: &Palette, x: f32, y: f32) {
        self.load = palette.gather(&palette.cells_within(x, y, self.radius));
        self.last = Some((x, y));
    }

    /// (x, y) までなぞる。通った場所の絵の具とナイフの絵の具が混ざり、ナイフの絵の具は伸びていく。
    pub fn drag(&mut self, palette: &mut Palette, x: f32, y: f32) {
        let Some((lx, ly)) = self.last else {
            self.begin(palette, x, y);
            return;
        };
        for (sx, sy) in steps((lx, ly), (x, y), self.radius) {
            self.smear(palette, sx, sy);
        }
        self.last = Some((x, y));
    }

    pub fn end(&mut self) {
        self.load = None;
        self.last = None;
    }

    fn smear(&mut self, palette: &mut Palette, x: f32, y: f32) {
        let cells = palette.cells_within(x, y, self.radius);
        let blended = match (self.load, palette.gather(&cells)) {
            (Some(load), Some(area)) => combine(load, area),
            (Some(load), None) => load,
            (None, Some(area)) => area,
            (None, None) => return,
        };
        for &index in &cells {
            palette.cells[index] = Some(match palette.cells[index] {
                Some(cell) => Paint { pigment: half_mix(cell.pigment, blended.pigment), amount: cell.amount },
                // 何もない場所には、ナイフの絵の具が薄く伸びる
                None => Paint { amount: blended.amount * 0.5, ..blended },
            });
        }
        self.load = Some(blended);
    }
}

/// ペンでパレットの上に描くときの筆。パレットの上ではいつも混色で、通った所の絵の具と混ざる。
///
/// 筆に付いた絵の具は描くうちに通った所の色を拾い、少しずつペンの色に戻る（油彩ペンと同じ）。
/// ペンの色そのものは変えない。色をペン先に取るのはスポイト（`Palette::pick`）で行う。
pub struct PaletteBrush {
    radius: f32,
    /// ペンの色。`None` なら絵の具を拭き取る（消しゴム）。
    base: Option<Pigment>,
    load: Option<Pigment>,
    last: Option<(f32, f32)>,
}

/// 1 回なぞるごとに、筆が通った所から拾う絵の具の割合。
const BRUSH_PICKUP: f32 = 0.35;
/// 1 回なぞるごとに、筆の絵の具がペンの色へ戻る割合。
const BRUSH_REFILL: f32 = 0.08;

impl PaletteBrush {
    /// `color` の絵の具で描く筆。
    pub fn paint(color: Color, radius: f32) -> Self {
        let base = Pigment::from_color(color);
        PaletteBrush { radius, base: Some(base), load: Some(base), last: None }
    }

    /// 絵の具を拭き取る布。
    pub fn wipe(radius: f32) -> Self {
        PaletteBrush { radius, base: None, load: None, last: None }
    }

    /// (x, y) まで描く。最初の呼び出しはその場に 1 回だけ置く。
    pub fn stroke_to(&mut self, palette: &mut Palette, x: f32, y: f32) {
        let from = self.last.unwrap_or((x, y));
        if self.last.is_none() {
            self.touch(palette, x, y);
        } else {
            for (sx, sy) in steps(from, (x, y), self.radius) {
                self.touch(palette, sx, sy);
            }
        }
        self.last = Some((x, y));
    }

    fn touch(&mut self, palette: &mut Palette, x: f32, y: f32) {
        let cells = palette.cells_within(x, y, self.radius);
        let (Some(base), Some(carried)) = (self.base, self.load) else {
            for &index in &cells {
                palette.cells[index] = None;
            }
            return;
        };
        let brushed = match palette.gather(&cells) {
            Some(area) => {
                Pigment::mix(&[(carried, 1.0 - BRUSH_PICKUP), (area.pigment, BRUSH_PICKUP)]).unwrap_or(carried)
            }
            None => carried,
        };
        for &index in &cells {
            palette.cells[index] = Some(match palette.cells[index] {
                Some(cell) => Paint { pigment: half_mix(cell.pigment, brushed), amount: cell.amount.max(0.5) },
                None => Paint { pigment: brushed, amount: 0.6 },
            });
        }
        self.load = Pigment::mix(&[(brushed, 1.0 - BRUSH_REFILL), (base, BRUSH_REFILL)]);
    }
}

/// `from` から `to` までを、半径 `radius` の筆で隙間なく埋める点の列（`from` は含まない）。
fn steps(from: (f32, f32), to: (f32, f32), radius: f32) -> Vec<(f32, f32)> {
    let distance = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
    let step = (radius / 3.0).max(0.5);
    let count = (distance / step).ceil().max(1.0) as usize;
    (1..=count)
        .map(|i| {
            let t = i as f32 / count as f32;
            (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
        })
        .collect()
}

fn combine(a: Paint, b: Paint) -> Paint {
    let pigment = Pigment::mix(&[(a.pigment, a.amount), (b.pigment, b.amount)]).unwrap_or(a.pigment);
    Paint { pigment, amount: (a.amount + b.amount).min(4.0) }
}

fn half_mix(a: Pigment, b: Pigment) -> Pigment {
    Pigment::mix(&[(a, 1.0), (b, 1.0)]).unwrap_or(a)
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Color = Color { r: 0.97, g: 0.97, b: 0.95, a: 1.0 };
    const RED: Color = Color { r: 0.82, g: 0.12, b: 0.1, a: 1.0 };
    const YELLOW: Color = Color { r: 0.98, g: 0.82, b: 0.05, a: 1.0 };
    const BLUE: Color = Color { r: 0.12, g: 0.18, b: 0.62, a: 1.0 };

    #[test]
    fn put_paint_can_be_picked() {
        let mut palette = Palette::new(100, 100);
        palette.put(RED, 20.0, 20.0, 5.0);
        let picked = palette.pick(20.0, 20.0).unwrap();
        assert!((picked.r - RED.r).abs() < 0.01);
        assert!(palette.pick(60.0, 60.0).is_none());
        assert!(palette.pick(-1.0, 5.0).is_none());
    }

    #[test]
    fn knife_mixes_blue_into_yellow_to_make_green() {
        let mut palette = Palette::new(120, 60);
        palette.put(BLUE, 30.0, 30.0, 8.0);
        palette.put(YELLOW, 70.0, 30.0, 8.0);
        let mut knife = PaletteKnife::new(6.0);
        knife.begin(&palette, 30.0, 30.0);
        knife.drag(&mut palette, 70.0, 30.0);
        knife.drag(&mut palette, 30.0, 30.0);
        knife.drag(&mut palette, 70.0, 30.0);
        knife.end();
        let mixed = palette.pick(70.0, 30.0).unwrap();
        assert!(mixed.g > mixed.r && mixed.g > mixed.b, "{mixed:?}");
    }

    #[test]
    fn knife_on_empty_board_does_nothing() {
        let mut palette = Palette::new(50, 50);
        let mut knife = PaletteKnife::new(5.0);
        knife.begin(&palette, 10.0, 10.0);
        knife.drag(&mut palette, 40.0, 40.0);
        assert!(palette.pick(25.0, 25.0).is_none());
    }

    #[test]
    fn brush_paints_on_empty_board_in_its_color() {
        let mut palette = Palette::new(100, 50);
        let red = RED;
        let mut brush = PaletteBrush::paint(red, 4.0);
        brush.stroke_to(&mut palette, 10.0, 25.0);
        brush.stroke_to(&mut palette, 60.0, 25.0);
        let painted = palette.pick(35.0, 25.0).unwrap();
        assert!((painted.r - red.r).abs() < 0.02 && painted.g < 0.2, "{painted:?}");
        assert!(palette.pick(35.0, 40.0).is_none());
    }

    #[test]
    fn brush_always_mixes_with_paint_on_the_board() {
        let mut palette = Palette::new(100, 50);
        palette.put(YELLOW, 50.0, 25.0, 10.0);
        let mut brush = PaletteBrush::paint(BLUE, 4.0);
        brush.stroke_to(&mut palette, 40.0, 25.0);
        brush.stroke_to(&mut palette, 60.0, 25.0);
        let mixed = palette.pick(50.0, 25.0).unwrap();
        assert!(mixed.g > mixed.r && mixed.g > mixed.b, "{mixed:?}");
    }

    #[test]
    fn wiping_removes_paint() {
        let mut palette = Palette::new(100, 50);
        palette.put(YELLOW, 50.0, 25.0, 10.0);
        let mut cloth = PaletteBrush::wipe(6.0);
        cloth.stroke_to(&mut palette, 40.0, 25.0);
        cloth.stroke_to(&mut palette, 60.0, 25.0);
        assert!(palette.pick(50.0, 25.0).is_none());
        assert!(palette.pick(50.0, 17.0).is_some(), "布が通らなかった所は残る");
    }

    #[test]
    fn render_shows_board_and_paint() {
        let mut palette = Palette::new(10, 10);
        palette.put(WHITE, 2.5, 2.5, 1.0);
        let mut out = vec![Rgba8::default(); 100];
        palette.render(&mut out, PaletteBoard::Wood);
        assert_eq!(out[99], PaletteBoard::Wood.color());
        assert!(out[2 * 10 + 2].r > 240);
    }

    #[test]
    fn white_board_shows_white_where_empty() {
        let mut palette = Palette::new(10, 10);
        palette.put(BLUE, 2.5, 2.5, 1.0);
        let mut out = vec![Rgba8::default(); 100];
        palette.render(&mut out, PaletteBoard::White);
        assert_eq!(out[99], PaletteBoard::White.color());
        assert_ne!(out[2 * 10 + 2], PaletteBoard::White.color());
    }
}
