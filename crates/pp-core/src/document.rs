use crate::layer::{BlendMode, Layer, SheetMaterial};
use crate::mode::CanvasMode;
use crate::tile::{TILE_SIZE, split_coord};
use crate::{Rect, Rgba8};

/// 用紙（描く面）の位置。0 番は用紙の白地や油彩の下地で、利用者には見せない。
/// 紙モードで重ねた紙は、これより上に並ぶ。
const PAPER_LAYER: usize = 1;

/// 1 枚の絵。レイヤーは下から上の順に並ぶ。
pub struct Document {
    pub width: u32,
    pub height: u32,
    pub mode: CanvasMode,
    /// 解像度（dpi）。画素の数は変えず、紙の上での大きさ（cm など）との換算と、書き出す画像に記録する値に使う。
    pub dpi: u32,
    pub layers: Vec<Layer>,
    pub active_layer: usize,
}

impl Document {
    /// 記録できる解像度（dpi）の範囲。
    pub const MIN_DPI: u32 = 1;
    pub const MAX_DPI: u32 = 10000;

    /// 紙モードは白い用紙と描画用の紙 1 枚、油彩モードは下地を塗ったキャンバス 1 枚で始める。
    pub fn new(width: u32, height: u32, mode: CanvasMode) -> Self {
        let bounds = Rect::from_size(width, height);
        let layers = match mode {
            CanvasMode::Paper => {
                let mut paper = Layer::new("白地");
                paper.fill(bounds, Rgba8 { r: 255, g: 255, b: 255, a: 255 });
                vec![paper, Layer::new("用紙")]
            }
            CanvasMode::OilCanvas => {
                // 利用者から見るとキャンバス 1 枚に直接描くが、内部では乾いた下地（ジェッソ）と
                // 絵の具の層を分けておく。油彩ペンが下地を絵の具として拾わないようにするため。
                let mut ground = Layer::new("下地");
                ground.fill(bounds, Rgba8 { r: 244, g: 240, b: 230, a: 255 });
                vec![ground, Layer::new("絵の具")]
            }
        };
        Document { width, height, mode, dpi: mode.default_dpi(), active_layer: PAPER_LAYER, layers }
    }

    /// 保存しておいたレイヤーから組み立てる。レイヤーの数や選んでいる紙がモードに合わなければ `None`。
    /// 解像度はモードの既定の値になるので、ファイルに記録があれば後から入れる。
    /// 0 番は用紙の白地（油彩では下地）、1 番は用紙で、紙モードではその上に重ねた紙が並ぶ。
    pub fn from_layers(
        width: u32,
        height: u32,
        mode: CanvasMode,
        layers: Vec<Layer>,
        active_layer: usize,
    ) -> Option<Self> {
        let count_ok = if mode.has_layers() { layers.len() > PAPER_LAYER } else { layers.len() == PAPER_LAYER + 1 };
        if width == 0 || height == 0 || !count_ok || active_layer < PAPER_LAYER || active_layer >= layers.len() {
            return None;
        }
        Some(Document { width, height, mode, dpi: mode.default_dpi(), layers, active_layer })
    }

    /// 利用者から見た紙の一覧（下から順に、用紙と重ねた紙）。`(レイヤー番号, レイヤー)` を返す。
    pub fn sheets(&self) -> impl Iterator<Item = (usize, &Layer)> {
        self.layers.iter().enumerate().skip(PAPER_LAYER)
    }

    fn is_movable_sheet(&self, index: usize) -> bool {
        index > PAPER_LAYER && index < self.layers.len()
    }

    /// 選んでいる紙のすぐ上に紙を重ね、それを選ぶ。紙モード以外では何もしない。
    pub fn add_sheet(&mut self, material: SheetMaterial) -> bool {
        if !self.mode.has_layers() {
            return false;
        }
        let count = self.layers.iter().filter(|l| l.material == material).count() + 1;
        let index = self.active_layer + 1;
        self.layers.insert(index, Layer::with_material(format!("{} {count}", material.name()), material));
        self.active_layer = index;
        true
    }

    /// 選んでいる紙を外す。用紙は外せない。
    pub fn remove_active_sheet(&mut self) -> bool {
        if !self.is_movable_sheet(self.active_layer) {
            return false;
        }
        self.layers.remove(self.active_layer);
        self.active_layer -= 1;
        true
    }

    /// 選んでいる紙を 1 枚上（`up`）か下へ入れ替える。用紙より下には行けない。
    pub fn move_active_sheet(&mut self, up: bool) -> bool {
        let from = self.active_layer;
        let to = if up { from + 1 } else { from.wrapping_sub(1) };
        if !self.is_movable_sheet(from) || !self.is_movable_sheet(to) {
            return false;
        }
        self.layers.swap(from, to);
        self.active_layer = to;
        true
    }

    /// 上（`up`）か下の紙を選ぶ。
    pub fn select_sheet(&mut self, up: bool) -> bool {
        let to = if up { self.active_layer + 1 } else { self.active_layer.saturating_sub(1) };
        if to < PAPER_LAYER || to >= self.layers.len() || to == self.active_layer {
            return false;
        }
        self.active_layer = to;
        true
    }

    /// `index` 番のレイヤー（`sheets` が返す番号）の紙を選ぶ。
    pub fn select_sheet_at(&mut self, index: usize) -> bool {
        if index < PAPER_LAYER || index >= self.layers.len() || index == self.active_layer {
            return false;
        }
        self.active_layer = index;
        true
    }

    /// 選んでいる紙をめくる（隠す）か戻す。用紙はめくれない。
    pub fn flip_active_sheet(&mut self) -> bool {
        self.flip_sheet(self.active_layer)
    }

    /// `index` 番のレイヤーの紙をめくるか戻す。用紙はめくれない。
    pub fn flip_sheet(&mut self, index: usize) -> bool {
        if !self.is_movable_sheet(index) {
            return false;
        }
        let layer = &mut self.layers[index];
        layer.visible = !layer.visible;
        true
    }

    pub fn active_layer(&self) -> &Layer {
        &self.layers[self.active_layer]
    }

    pub fn layer_index(&self, id: u64) -> Option<usize> {
        self.layers.iter().position(|layer| layer.id() == id)
    }

    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.width, self.height)
    }

    pub fn active_layer_mut(&mut self) -> &mut Layer {
        &mut self.layers[self.active_layer]
    }

    /// `region` を合成して `out` に書き出す。`out` は region の幅 × 高さの行優先バッファ。
    pub fn composite(&self, requested: Rect, out: &mut [Rgba8]) {
        let region = requested.intersect(&self.bounds());
        let w = region.width() as usize;
        assert!(out.len() >= w * region.height() as usize);
        out[..w * region.height() as usize].fill(Rgba8::default());

        let ts = TILE_SIZE as i32;
        let (tx0, _) = split_coord(region.x0);
        let (ty0, _) = split_coord(region.y0);
        let (tx1, _) = split_coord(region.x1 - 1);
        let (ty1, _) = split_coord(region.y1 - 1);

        for layer in self.layers.iter().filter(|l| l.visible && l.opacity > 0) {
            if let Some(veil) = layer.material.veil() {
                // トレーシングペーパーは、下にあるもの全体を白っぽく覆う
                for px in out[..w * region.height() as usize].iter_mut() {
                    *px = src_over(*px, veil);
                }
            }
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    let Some(tile) = layer.tile(tx, ty) else {
                        continue;
                    };
                    let r = region.intersect(&Rect::new(tx * ts, ty * ts, (tx + 1) * ts, (ty + 1) * ts));
                    for y in r.y0..r.y1 {
                        let src = &tile.row((y - ty * ts) as u32)[(r.x0 - tx * ts) as usize..(r.x1 - tx * ts) as usize];
                        let start = (y - region.y0) as usize * w + (r.x0 - region.x0) as usize;
                        let dst = &mut out[start..start + src.len()];
                        blend_row(dst, src, layer.opacity, layer.blend);
                    }
                }
            }
        }
    }
}

fn blend_row(dst: &mut [Rgba8], src: &[Rgba8], opacity: u8, mode: BlendMode) {
    match mode {
        BlendMode::Normal => {
            for (d, s) in dst.iter_mut().zip(src) {
                if s.a == 0 {
                    continue;
                }
                *d = src_over(*d, scale(*s, opacity));
            }
        }
    }
}

/// a * b / 255 を丸めて計算する。
#[inline]
pub(crate) fn mul255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}

#[inline]
fn scale(p: Rgba8, k: u8) -> Rgba8 {
    if k == 255 {
        return p;
    }
    let k = k as u32;
    Rgba8 {
        r: mul255(p.r as u32, k) as u8,
        g: mul255(p.g as u32, k) as u8,
        b: mul255(p.b as u32, k) as u8,
        a: mul255(p.a as u32, k) as u8,
    }
}

/// premultiplied の destination-out 合成（消しゴム）。`a` の分だけ下地を消す。
#[inline]
pub(crate) fn dst_out(d: Rgba8, a: u32) -> Rgba8 {
    let keep = 255 - a;
    Rgba8 {
        r: mul255(d.r as u32, keep) as u8,
        g: mul255(d.g as u32, keep) as u8,
        b: mul255(d.b as u32, keep) as u8,
        a: mul255(d.a as u32, keep) as u8,
    }
}

/// premultiplied の src-over 合成。
#[inline]
pub(crate) fn src_over(d: Rgba8, s: Rgba8) -> Rgba8 {
    let inv = 255 - s.a as u32;
    Rgba8 {
        r: (s.r as u32 + mul255(d.r as u32, inv)) as u8,
        g: (s.g as u32 + mul255(d.g as u32, inv)) as u8,
        b: (s.b as u32 + mul255(d.b as u32, inv)) as u8,
        a: (s.a as u32 + mul255(d.a as u32, inv)) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul255_matches_rounded_division() {
        for a in 0..=255u32 {
            for b in 0..=255u32 {
                let exact = ((a * b) as f32 / 255.0).round() as u32;
                assert_eq!(mul255(a, b), exact, "{a} * {b}");
            }
        }
    }

    #[test]
    fn new_document_composites_to_white() {
        let doc = Document::new(300, 300, CanvasMode::Paper);
        let region = Rect::new(250, 250, 270, 260);
        let mut out = vec![Rgba8::default(); 20 * 10];
        doc.composite(region, &mut out);
        assert!(out.iter().all(|p| *p == Rgba8 { r: 255, g: 255, b: 255, a: 255 }));
    }

    #[test]
    fn oil_canvas_paints_above_the_ground() {
        let doc = Document::new(64, 64, CanvasMode::OilCanvas);
        assert_eq!(doc.layers.len(), 2);
        assert_eq!(doc.active_layer, 1);
        assert_eq!(doc.layers[0].pixel(10, 10).a, 255);
        assert_eq!(doc.layers[1].pixel(10, 10).a, 0);
    }

    #[test]
    fn sheets_can_be_added_moved_flipped_and_removed() {
        let mut doc = Document::new(64, 64, CanvasMode::Paper);
        assert!(doc.add_sheet(SheetMaterial::TracingPaper));
        assert!(doc.add_sheet(SheetMaterial::ClearFilm));
        assert_eq!(doc.active_layer().material, SheetMaterial::ClearFilm);
        assert_eq!(doc.sheets().count(), 3);

        assert!(doc.move_active_sheet(false));
        assert_eq!(doc.layers[2].material, SheetMaterial::ClearFilm);
        // 用紙より下には動かせない
        assert!(!doc.move_active_sheet(false));

        assert!(doc.flip_active_sheet());
        assert!(!doc.active_layer().visible);

        assert!(doc.remove_active_sheet());
        assert_eq!(doc.active_layer, PAPER_LAYER);
        assert!(!doc.remove_active_sheet(), "用紙は外せない");
        assert_eq!(doc.sheets().count(), 2);
    }

    #[test]
    fn from_layers_rejects_layers_that_do_not_fit_the_mode() {
        let layers = || vec![Layer::new("白地"), Layer::new("用紙"), Layer::new("紙")];
        assert!(Document::from_layers(64, 64, CanvasMode::Paper, layers(), 2).is_some());
        assert!(Document::from_layers(64, 64, CanvasMode::Paper, layers(), 0).is_none(), "白地は選べない");
        assert!(Document::from_layers(64, 64, CanvasMode::Paper, layers(), 3).is_none());
        assert!(Document::from_layers(64, 64, CanvasMode::OilCanvas, layers(), 1).is_none(), "油彩は 2 層だけ");
        assert!(Document::from_layers(64, 64, CanvasMode::Paper, vec![Layer::new("白地")], 1).is_none());
    }

    #[test]
    fn oil_canvas_has_no_sheets_to_add() {
        let mut doc = Document::new(64, 64, CanvasMode::OilCanvas);
        assert!(!doc.add_sheet(SheetMaterial::ClearFilm));
    }

    #[test]
    fn tracing_paper_veils_what_is_below_and_film_does_not() {
        let mut doc = Document::new(64, 64, CanvasMode::Paper);
        doc.layers[PAPER_LAYER].fill(Rect::new(0, 0, 8, 8), Rgba8 { r: 0, g: 0, b: 0, a: 255 });
        let mut out = vec![Rgba8::default(); 1];
        doc.add_sheet(SheetMaterial::ClearFilm);
        doc.composite(Rect::new(1, 1, 2, 2), &mut out);
        assert_eq!(out[0].r, 0);

        doc.add_sheet(SheetMaterial::TracingPaper);
        doc.composite(Rect::new(1, 1, 2, 2), &mut out);
        assert!(out[0].r > 100 && out[0].r < 200, "{:?}", out[0]);

        doc.flip_active_sheet();
        doc.composite(Rect::new(1, 1, 2, 2), &mut out);
        assert_eq!(out[0].r, 0, "めくった紙は見えない");
    }

    #[test]
    fn hidden_layer_is_skipped() {
        let mut doc = Document::new(64, 64, CanvasMode::Paper);
        doc.layers[0].visible = false;
        let mut out = vec![Rgba8::default(); 4];
        doc.composite(Rect::new(0, 0, 2, 2), &mut out);
        assert!(out.iter().all(|p| p.a == 0));
    }
}
