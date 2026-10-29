use crate::document::{dst_out, mul255, src_over};
use crate::layer::Layer;
use crate::mode::CanvasMode;
use crate::paint_supply::PaintSupply;
use crate::pigment::Pigment;
use crate::stroke_buffer::{StrokeBuffer, ceiling};
use crate::tile::{TILE_SIZE, split_coord};
use crate::{Color, Rect, Rgba8};

/// ペン・マウスからの 1 サンプル。座標はキャンバスのピクセル座標。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputSample {
    pub x: f32,
    pub y: f32,
    /// 0.0..=1.0。筆圧のないマウスは 1.0。
    pub pressure: f32,
}

/// ペンのインターフェース。ペンの種類はこれを実装した構造体として増やす。
pub trait Pen {
    fn name(&self) -> &str;

    /// このキャンバスモードで使えるか（油彩キャンバスでは消しゴムを使えない、など）。
    fn usable_on(&self, mode: CanvasMode) -> bool;

    fn size(&self) -> f32;

    fn set_size(&mut self, size: f32);

    /// パレットで絵の具を付けたときに呼ばれる。色を持たないペン（消しゴム）は何もしない。
    fn set_color(&mut self, color: Color);

    /// ペン先の色。消しゴムのように色を持たないペンは `None`。
    fn color(&self) -> Option<Color>;

    /// 筆圧 `pressure` のとき、次のダブまでの間隔（px）。
    fn spacing(&self, pressure: f32) -> f32;

    /// ストロークの描き始めに、描く先のレイヤーを渡して呼ばれる。
    /// ストローク中だけの状態を持つペンはここで初期化する。
    fn begin_stroke(&mut self, layer: &Layer);

    /// 1 つのダブをレイヤーに描き、変更した範囲を返す。
    fn dab(&mut self, layer: &mut Layer, sample: InputSample, clip: Rect) -> Rect;

    /// 描いたものを消す道具（消しゴムの仲間）か。ブラシサイズは、消す道具とそれ以外で別々に共通にする。
    fn erases(&self) -> bool {
        false
    }

    /// パレットの絵の具を付けて描くペンか。インクや鉛筆の芯のように色が決まっているペンは false。
    fn takes_paint(&self) -> bool {
        false
    }

    /// キャンバス上での描き方。選べないペンは `None`。
    fn paint_mode(&self) -> Option<PaintMode> {
        None
    }

    /// 描き方を変える。選べないペンは何もしない。
    fn set_paint_mode(&mut self, _mode: PaintMode) {}

    /// ペン先に含ませる絵の具の量（0..=1）。少ないほど、描くうちに絵の具が減ってかすれる。選べないペンは `None`。
    fn paint_amount(&self) -> Option<f32> {
        None
    }

    fn set_paint_amount(&mut self, _amount: f32) {}

    /// 絵の具の不透明度（0..=1）。低いほど下が透ける。選べないペンは `None`。
    fn opacity(&self) -> Option<f32> {
        None
    }

    fn set_opacity(&mut self, _opacity: f32) {}

    /// 筆の形。選べないペンは `None`。
    fn tip_shape(&self) -> Option<TipShape> {
        None
    }

    fn set_tip_shape(&mut self, _shape: TipShape) {}
}

/// 筆の形。筆先を紙に置いたときの跡の形で、平筆とフィルバートは平たい。
///
/// 傾きを読まないので、平たい筆は手で持ったときと同じ向き（`FLAT_TIP_ANGLE`）に固定する。
/// 横に引くと太く、その向きに沿って引くと細くなる。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TipShape {
    /// 丸筆。円い跡。
    #[default]
    Round,
    /// 平筆。角の丸い長方形の跡。
    Flat,
    /// フィルバート（平筆の先を丸くしたもの）。楕円の跡。
    Filbert,
}

/// 平たい筆の、毛の並ぶ向き（キャンバスの右を 0、上回りを正とする角度）。
const FLAT_TIP_ANGLE: f32 = 70.0 * std::f32::consts::PI / 180.0;

impl TipShape {
    pub const ALL: [TipShape; 3] = [TipShape::Round, TipShape::Flat, TipShape::Filbert];

    pub fn name(self) -> &'static str {
        match self {
            TipShape::Round => "丸筆",
            TipShape::Flat => "平筆",
            TipShape::Filbert => "フィルバート",
        }
    }

    /// 毛の並ぶ向きの幅に対する、厚みの割合。
    fn aspect(self) -> f32 {
        match self {
            TipShape::Round => 1.0,
            TipShape::Flat => 0.3,
            TipShape::Filbert => 0.5,
        }
    }
}

/// 1 つのダブの跡の形。中心からの位置ごとに、どれだけ覆うかを求める。
pub(crate) struct Footprint {
    shape: TipShape,
    /// 毛の並ぶ向きの半径と、厚みの向きの半径（px）。
    radius: f32,
    thickness: f32,
    /// 毛の並ぶ向き（画面の y は下向きなので、上回りの角度を y について反転して持つ）。
    cos: f32,
    sin: f32,
    /// ぼかしの幅（外周を 1 とした割合）。
    soft: f32,
}

impl Footprint {
    fn new(shape: TipShape, radius: f32, hardness: f32) -> Self {
        let thickness = (radius * shape.aspect()).max(0.5);
        // hardness=1 でもアンチエイリアス用に 1px は残す
        let soft = (1.0 - hardness.clamp(0.0, 1.0)).max(1.0 / thickness).min(1.0);
        let angle = if shape == TipShape::Round { 0.0 } else { FLAT_TIP_ANGLE };
        Footprint { shape, radius, thickness, cos: angle.cos(), sin: -angle.sin(), soft }
    }

    /// 中心から縦に `dy` 離れた行で、跡がかかりうる横の範囲（中心からの dx の最小と最大）。かからなければ `None`。
    ///
    /// 太い筆で、外接矩形の角の何もない所まで覆い具合を計算しないために使う。少し広めに返すことはあっても、狭くはしない。
    pub(crate) fn row_span(&self, dy: f32) -> Option<(f32, f32)> {
        let (c, s, a, b) = (self.cos, self.sin, self.radius, self.thickness);
        match self.shape {
            TipShape::Round | TipShape::Filbert => {
                // 傾いた楕円 ((dx c + dy s) / a)² + ((-dx s + dy c) / b)² = 1 を dx の 2 次式として解く
                let qa = c * c / (a * a) + s * s / (b * b);
                let qb = 2.0 * dy * c * s * (1.0 / (a * a) - 1.0 / (b * b));
                let qc = dy * dy * (s * s / (a * a) + c * c / (b * b)) - 1.0;
                let disc = qb * qb - 4.0 * qa * qc;
                if disc < 0.0 {
                    return None;
                }
                let root = disc.sqrt();
                Some(((-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)))
            }
            // 超楕円は、同じ向きの長方形 |along| <= a, |across| <= b の中に収まる
            TipShape::Flat => {
                let strip = |k: f32, offset: f32, half: f32| -> (f32, f32) {
                    // |dx k + offset| <= half を満たす dx
                    if k.abs() < 1e-6 {
                        return if offset.abs() <= half { (f32::NEG_INFINITY, f32::INFINITY) } else { (1.0, -1.0) };
                    }
                    let (p, q) = ((-half - offset) / k, (half - offset) / k);
                    (p.min(q), p.max(q))
                };
                let (l0, h0) = strip(c, dy * s, a);
                let (l1, h1) = strip(-s, dy * c, b);
                let (lo, hi) = (l0.max(l1), h0.min(h1));
                (lo <= hi).then_some((lo, hi))
            }
        }
    }

    /// 中心から (dx, dy) の画素の覆い具合（0..=1）。跡の外なら `None`。
    pub(crate) fn coverage(&self, dx: f32, dy: f32) -> Option<f32> {
        let along = (dx * self.cos + dy * self.sin) / self.radius;
        let across = (-dx * self.sin + dy * self.cos) / self.thickness;
        let distance = match self.shape {
            TipShape::Round | TipShape::Filbert => (along * along + across * across).sqrt(),
            // 角の丸い長方形（超楕円）
            TipShape::Flat => (along.powi(6) + across.powi(6)).powf(1.0 / 6.0),
        };
        if distance >= 1.0 {
            return None;
        }
        Some(if distance <= 1.0 - self.soft { 1.0 } else { smoothstep((1.0 - distance) / self.soft) })
    }
}

/// 絵の具を使うペンの、キャンバス上での描き方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaintMode {
    /// 下にある乾いていない絵の具と混ぜながら描く。
    #[default]
    Mix,
    /// 下の絵の具と混ぜず、ペン先の色で覆う。
    Overwrite,
}

impl PaintMode {
    pub const ALL: [PaintMode; 2] = [PaintMode::Mix, PaintMode::Overwrite];

    pub fn name(self) -> &'static str {
        match self {
            PaintMode::Mix => "混色",
            PaintMode::Overwrite => "上書き",
        }
    }
}

/// ダブをレイヤーにどう重ねるか。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DabMode {
    /// 色を重ねる。
    #[default]
    Paint,
    /// 下地を削って透明に戻す。
    Erase,
}

/// 円形のペン先。各ペンが部品として持つ。
#[derive(Clone, Debug)]
pub struct RoundTip {
    /// 筆圧 1.0 のときの直径（px）。
    pub size: f32,
    /// 0.0 で外周までぼかす、1.0 で完全なエッジ。
    pub hardness: f32,
    pub color: Color,
    pub opacity: f32,
    /// ダブ間隔（直径に対する比率）。
    pub spacing: f32,
    /// 筆圧をサイズに反映するか。
    pub pressure_size: bool,
    /// 筆圧を不透明度に反映するか。
    pub pressure_opacity: bool,
    pub mode: DabMode,
    /// 跡の形。`size` は毛の並ぶ向きの幅。
    pub shape: TipShape,
}

impl Default for RoundTip {
    fn default() -> Self {
        RoundTip {
            size: 12.0,
            hardness: 0.8,
            color: Color::BLACK,
            opacity: 1.0,
            spacing: 0.1,
            pressure_size: true,
            pressure_opacity: false,
            mode: DabMode::Paint,
            shape: TipShape::Round,
        }
    }
}

impl RoundTip {
    pub fn diameter(&self, pressure: f32) -> f32 {
        let p = if self.pressure_size { pressure.clamp(0.0, 1.0) } else { 1.0 };
        (self.size * p).max(1.0)
    }

    /// ダブの間隔。平たい筆は厚みの向きに進んでも跡がつながるよう、厚みを基準にする。
    pub fn spacing_px(&self, pressure: f32) -> f32 {
        (self.diameter(pressure) * self.shape.aspect() * self.spacing).max(0.5)
    }

    pub(crate) fn footprint(&self, pressure: f32) -> Footprint {
        Footprint::new(self.shape, self.diameter(pressure) * 0.5, self.hardness)
    }

    /// ダブの外接矩形（`clip` で切り取ったもの）。
    pub fn bounds(&self, s: InputSample, clip: Rect) -> Rect {
        let radius = self.diameter(s.pressure) * 0.5;
        Rect::new(
            (s.x - radius).floor() as i32,
            (s.y - radius).floor() as i32,
            (s.x + radius).ceil() as i32 + 1,
            (s.y + radius).ceil() as i32 + 1,
        )
        .intersect(&clip)
    }

    /// ダブが覆う各画素について、覆い具合（0..=1、筆圧・不透明度は含まない）を `f` に渡す。
    pub fn for_each_coverage(&self, s: InputSample, clip: Rect, mut f: impl FnMut(i32, i32, f32)) -> Rect {
        let footprint = self.footprint(s.pressure);
        let bounds = self.bounds(s, clip);
        for y in bounds.y0..bounds.y1 {
            let fy = y as f32 + 0.5 - s.y;
            for x in bounds.x0..bounds.x1 {
                if let Some(coverage) = footprint.coverage(x as f32 + 0.5 - s.x, fy) {
                    f(x, y, coverage);
                }
            }
        }
        bounds
    }

    /// ダブが覆う範囲にある絵の具の平均色（不透明度で重み付け）。何もなければ `None`。
    pub fn average_under(&self, layer: &Layer, s: InputSample, clip: Rect) -> Option<Color> {
        let radius = self.diameter(s.pressure) * 0.5;
        let bounds = Rect::new(
            (s.x - radius).floor() as i32,
            (s.y - radius).floor() as i32,
            (s.x + radius).ceil() as i32 + 1,
            (s.y + radius).ceil() as i32 + 1,
        )
        .intersect(&clip);
        let footprint = self.footprint(s.pressure);
        // 大きいペン先では間引いて読む（平均なので精度は十分）
        let stride = ((radius * self.shape.aspect() / 8.0) as usize).max(1);
        let mut sum = [0.0f32; 4];
        for y in (bounds.y0..bounds.y1).step_by(stride) {
            for x in (bounds.x0..bounds.x1).step_by(stride) {
                if footprint.coverage(x as f32 + 0.5 - s.x, y as f32 + 0.5 - s.y).is_none() {
                    continue;
                }
                let px = layer.pixel(x, y);
                // premultiplied なので、そのまま足して最後に不透明度で割れば重み付き平均になる
                sum[0] += px.r as f32;
                sum[1] += px.g as f32;
                sum[2] += px.b as f32;
                sum[3] += px.a as f32;
            }
        }
        (sum[3] > 0.0).then(|| Color { r: sum[0] / sum[3], g: sum[1] / sum[3], b: sum[2] / sum[3], a: 1.0 })
    }

    /// 円形のダブを 1 つ src-over で描く。変更した範囲を返す。
    pub fn draw(&self, layer: &mut Layer, s: InputSample, clip: Rect) -> Rect {
        let radius = self.diameter(s.pressure) * 0.5;
        let opacity =
            self.opacity * self.color.a * if self.pressure_opacity { s.pressure.clamp(0.0, 1.0) } else { 1.0 };
        if opacity <= 0.0 {
            return Rect::EMPTY;
        }
        let bounds = Rect::new(
            (s.x - radius).floor() as i32,
            (s.y - radius).floor() as i32,
            (s.x + radius).ceil() as i32 + 1,
            (s.y + radius).ceil() as i32 + 1,
        )
        .intersect(&clip);
        if bounds.is_empty() {
            return Rect::EMPTY;
        }

        let footprint = self.footprint(s.pressure);
        let alpha_max = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
        let (cr, cg, cb) = (
            (self.color.r.clamp(0.0, 1.0) * 255.0).round() as u32,
            (self.color.g.clamp(0.0, 1.0) * 255.0).round() as u32,
            (self.color.b.clamp(0.0, 1.0) * 255.0).round() as u32,
        );

        let ts = TILE_SIZE as i32;
        let (tx0, _) = split_coord(bounds.x0);
        let (ty0, _) = split_coord(bounds.y0);
        let (tx1, _) = split_coord(bounds.x1 - 1);
        let (ty1, _) = split_coord(bounds.y1 - 1);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let r = bounds.intersect(&Rect::new(tx * ts, ty * ts, (tx + 1) * ts, (ty + 1) * ts));
                let tile = layer.tile_mut_or_create(tx, ty);
                for y in r.y0..r.y1 {
                    let fy = y as f32 + 0.5 - s.y;
                    let row = tile.row_mut((y - ty * ts) as u32);
                    for x in r.x0..r.x1 {
                        let Some(cov) = footprint.coverage(x as f32 + 0.5 - s.x, fy) else {
                            continue;
                        };
                        let a = (cov * alpha_max as f32 + 0.5) as u32;
                        if a == 0 {
                            continue;
                        }
                        let src = Rgba8 {
                            r: mul255(cr, a) as u8,
                            g: mul255(cg, a) as u8,
                            b: mul255(cb, a) as u8,
                            a: a as u8,
                        };
                        let px = &mut row[(x - tx * ts) as usize];
                        *px = match self.mode {
                            DabMode::Paint => src_over(*px, src),
                            DabMode::Erase => dst_out(*px, a),
                        };
                    }
                }
            }
        }
        bounds
    }
}

#[inline]
fn smoothstep(edge: f32) -> f32 {
    let t = edge.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// インクペン。基本のペン。
#[derive(Clone, Debug, Default)]
pub struct InkPen {
    pub tip: RoundTip,
}

impl Pen for InkPen {
    fn name(&self) -> &str {
        "インクペン"
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

    fn set_color(&mut self, color: Color) {
        self.tip.color = color;
    }

    fn color(&self) -> Option<Color> {
        Some(self.tip.color)
    }

    fn spacing(&self, pressure: f32) -> f32 {
        self.tip.spacing_px(pressure)
    }

    fn begin_stroke(&mut self, _layer: &Layer) {}

    fn dab(&mut self, layer: &mut Layer, sample: InputSample, clip: Rect) -> Rect {
        self.tip.draw(layer, sample, clip)
    }

    fn takes_paint(&self) -> bool {
        true
    }
}

/// 油彩ペン。
///
/// キャンバス上のまだ乾いていない絵の具を拾いながら描くので、上に重ねると下の色と混ざる。
/// 拾った色は描くうちにパレットで付けた色へ戻っていく。
/// 絵の具の量が少ないと、1 ストロークの中で絵の具が減っていき、線の終わりがかすれる。
/// 1 ストロークの中ではダブが重なっても濃くならないので、不透明度を下げると線全体が同じだけ透ける。
pub struct OilPen {
    pub tip: RoundTip,
    /// いまペン先に付いている絵の具（パレットの色と拾った色が混ざったもの）。
    load: Option<Pigment>,
    mode: PaintMode,
    supply: PaintSupply,
    stroke: Option<StrokeBuffer>,
}

/// 絵の具の不透明度の下限。0 にすると何も描けないので、少しは残す。
pub(crate) const MIN_PAINT_OPACITY: f32 = 0.05;

/// 1 ダブごとに、キャンバスから拾う絵の具の割合。
const OIL_PICKUP: f32 = 0.4;
/// 1 ダブごとに、ペン先の絵の具がパレットの色へ戻る割合。
const OIL_REFILL: f32 = 0.1;

impl Default for OilPen {
    fn default() -> Self {
        OilPen {
            tip: RoundTip {
                size: 24.0,
                hardness: 0.6,
                color: Color { r: 0.15, g: 0.25, b: 0.6, a: 1.0 },
                ..RoundTip::default()
            },
            load: None,
            mode: PaintMode::Mix,
            supply: PaintSupply::default(),
            stroke: None,
        }
    }
}

impl OilPen {
    /// `color` のダブを、このストロークの下書きを通して描く。描く前の画素の上に、覆った分だけ重ねる。
    fn paint(&mut self, layer: &mut Layer, sample: InputSample, clip: Rect, color: Color, opacity: f32) -> Rect {
        let OilPen { tip, stroke, .. } = self;
        let buffer = stroke.get_or_insert_with(|| StrokeBuffer::new(layer));
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        let (r, g, b) = (to_u8(color.r), to_u8(color.g), to_u8(color.b));
        let amount = |_x: i32, _y: i32, coverage: f32| (coverage * opacity * 255.0 + 0.5) as u32;
        buffer.cover_dab(layer, tip, sample, clip, ceiling(opacity), amount, |below, a| {
            let a = a as u32;
            let src = Rgba8 { r: mul255(r, a) as u8, g: mul255(g, a) as u8, b: mul255(b, a) as u8, a: a as u8 };
            src_over(below, src)
        })
    }
}

impl Pen for OilPen {
    fn name(&self) -> &str {
        "油彩"
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

    fn set_color(&mut self, color: Color) {
        self.tip.color = color;
    }

    fn color(&self) -> Option<Color> {
        Some(self.tip.color)
    }

    fn spacing(&self, pressure: f32) -> f32 {
        self.tip.spacing_px(pressure)
    }

    fn begin_stroke(&mut self, _layer: &Layer) {
        self.load = Some(Pigment::from_color(self.tip.color));
        self.supply.refill();
        self.stroke = None;
    }

    fn dab(&mut self, layer: &mut Layer, sample: InputSample, clip: Rect) -> Rect {
        // 残りの絵の具が少ないほど薄くなる
        let opacity = self.tip.opacity * self.supply.use_paint(sample);
        if opacity <= 0.0 {
            return Rect::EMPTY;
        }
        if self.mode == PaintMode::Overwrite {
            return self.paint(layer, sample, clip, self.tip.color, opacity);
        }
        let base = Pigment::from_color(self.tip.color);
        let carried = self.load.unwrap_or(base);
        let picked_up = match self.tip.average_under(layer, sample, clip) {
            Some(below) => Pigment::mix(&[(carried, 1.0 - OIL_PICKUP), (Pigment::from_color(below), OIL_PICKUP)]),
            None => Some(carried),
        };
        let refilled =
            Pigment::mix(&[(picked_up.unwrap_or(carried), 1.0 - OIL_REFILL), (base, OIL_REFILL)]).unwrap_or(base);
        self.load = Some(refilled);
        self.paint(layer, sample, clip, refilled.to_color(), opacity)
    }

    fn takes_paint(&self) -> bool {
        true
    }

    fn paint_mode(&self) -> Option<PaintMode> {
        Some(self.mode)
    }

    fn set_paint_mode(&mut self, mode: PaintMode) {
        self.mode = mode;
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

/// 消しゴム。こすったところをすっかり消して、下の紙を見せる。
#[derive(Clone, Debug)]
pub struct Eraser {
    pub tip: RoundTip,
}

impl Default for Eraser {
    fn default() -> Self {
        Eraser { tip: RoundTip { size: 30.0, hardness: 0.9, mode: DabMode::Erase, ..RoundTip::default() } }
    }
}

impl Pen for Eraser {
    fn name(&self) -> &str {
        "消しゴム"
    }

    /// 油彩の絵の具は消しゴムでは消せない。
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

    fn begin_stroke(&mut self, _layer: &Layer) {}

    fn dab(&mut self, layer: &mut Layer, sample: InputSample, clip: Rect) -> Rect {
        self.tip.draw(layer, sample, clip)
    }

    fn erases(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(x: f32, y: f32) -> InputSample {
        InputSample { x, y, pressure: 1.0 }
    }

    #[test]
    fn eraser_clears_painted_pixels() {
        let mut layer = Layer::new("t");
        let clip = Rect::from_size(200, 200);
        let mut ink = InkPen { tip: RoundTip { size: 40.0, hardness: 1.0, ..RoundTip::default() } };
        ink.dab(&mut layer, sample(100.0, 100.0), clip);
        assert_eq!(layer.pixel(100, 100).a, 255);

        let mut eraser =
            Eraser { tip: RoundTip { size: 10.0, hardness: 1.0, mode: DabMode::Erase, ..RoundTip::default() } };
        eraser.dab(&mut layer, sample(100.0, 100.0), clip);
        assert_eq!(layer.pixel(100, 100), Rgba8::default());
        // 消しゴムの外は残る
        assert_eq!(layer.pixel(115, 100).a, 255);
    }

    #[test]
    fn oil_picks_up_paint_below() {
        let clip = Rect::from_size(200, 200);
        let mut layer = Layer::new("t");
        let mut yellow = InkPen { tip: RoundTip { size: 80.0, hardness: 1.0, ..RoundTip::default() } };
        yellow.set_color(Color { r: 0.98, g: 0.82, b: 0.05, a: 1.0 });
        yellow.dab(&mut layer, sample(100.0, 100.0), clip);

        let mut oil = OilPen::default();
        oil.set_color(Color { r: 0.12, g: 0.18, b: 0.62, a: 1.0 });
        oil.tip.hardness = 1.0;
        oil.begin_stroke(&layer);
        for i in 0..5 {
            oil.dab(&mut layer, sample(90.0 + i as f32 * 4.0, 100.0), clip);
        }
        let px = layer.pixel(106, 100);
        // 青と黄が混ざって緑寄りになる
        assert!(px.g > px.r && px.g > px.b, "{px:?}");
    }

    #[test]
    fn oil_on_empty_layer_keeps_palette_color() {
        let clip = Rect::from_size(100, 100);
        let mut layer = Layer::new("t");
        let mut oil = OilPen::default();
        oil.tip.hardness = 1.0;
        oil.set_color(Color { r: 0.8, g: 0.1, b: 0.1, a: 1.0 });
        oil.begin_stroke(&layer);
        oil.dab(&mut layer, sample(50.0, 50.0), clip);
        let px = layer.pixel(50, 50);
        assert!(px.r > 190 && px.g < 40, "{px:?}");
    }

    #[test]
    fn oil_in_overwrite_mode_covers_paint_below() {
        let clip = Rect::from_size(200, 200);
        let mut layer = Layer::new("t");
        let mut yellow = InkPen { tip: RoundTip { size: 80.0, hardness: 1.0, ..RoundTip::default() } };
        yellow.set_color(Color { r: 0.98, g: 0.82, b: 0.05, a: 1.0 });
        yellow.dab(&mut layer, sample(100.0, 100.0), clip);

        let mut oil = OilPen::default();
        oil.set_paint_mode(PaintMode::Overwrite);
        assert_eq!(oil.paint_mode(), Some(PaintMode::Overwrite));
        oil.set_color(Color { r: 0.12, g: 0.18, b: 0.62, a: 1.0 });
        oil.tip.hardness = 1.0;
        oil.begin_stroke(&layer);
        for i in 0..5 {
            oil.dab(&mut layer, sample(90.0 + i as f32 * 4.0, 100.0), clip);
        }
        let px = layer.pixel(106, 100);
        assert!(px.b > px.g && px.r < 40, "{px:?}");
    }

    /// 赤い油彩で、空のレイヤーに (10, 50) から右へ `length` px の線を引く。
    fn oil_line(oil: &mut OilPen, length: f32) -> Layer {
        let clip = Rect::from_size(1200, 100);
        let mut layer = Layer::new("t");
        oil.tip.hardness = 1.0;
        oil.set_color(Color { r: 0.8, g: 0.1, b: 0.1, a: 1.0 });
        oil.begin_stroke(&layer);
        let step = oil.spacing(1.0);
        let mut x = 10.0;
        while x <= 10.0 + length {
            oil.dab(&mut layer, sample(x, 50.0), clip);
            x += step;
        }
        layer
    }

    #[test]
    fn little_paint_runs_dry_along_the_stroke() {
        let mut full = OilPen::default();
        let rich = oil_line(&mut full, 1000.0);
        assert_eq!(rich.pixel(1000, 50).a, 255, "たっぷりなら最後まで描ける");

        let mut scarce = OilPen::default();
        scarce.set_paint_amount(0.0);
        let dry = oil_line(&mut scarce, 1000.0);
        assert!(dry.pixel(20, 50).a > 200, "描き始めは濃い");
        let middle = dry.pixel(110, 50).a;
        assert!(middle > 0 && middle < 200, "途中でかすれる: {middle}");
        assert_eq!(dry.pixel(900, 50).a, 0, "絵の具がなくなると描けない");

        // 次のストロークでは、また絵の具を含ませて描ける
        let again = oil_line(&mut scarce, 10.0);
        assert!(again.pixel(15, 50).a > 200);
    }

    #[test]
    fn oil_opacity_lets_the_paint_below_show() {
        let mut thin = OilPen::default();
        thin.set_opacity(0.3);
        assert_eq!(thin.opacity(), Some(0.3));
        let layer = oil_line(&mut thin, 0.0);
        let alpha = layer.pixel(10, 50).a;
        assert!((60..=110).contains(&alpha), "{alpha}");
        thin.set_opacity(0.0);
        assert_eq!(thin.opacity(), Some(0.05), "まったく描けなくはならない");
    }

    #[test]
    fn row_span_holds_every_covered_pixel() {
        for shape in TipShape::ALL {
            let footprint = Footprint::new(shape, 40.0, 0.5);
            for dy in -45..=45 {
                let fy = dy as f32 + 0.5;
                let span = footprint.row_span(fy);
                for dx in -45..=45 {
                    let fx = dx as f32 + 0.5;
                    if footprint.coverage(fx, fy).is_some() {
                        let (lo, hi) = span.unwrap_or_else(|| panic!("{shape:?} の行 {dy} に範囲がない"));
                        assert!(lo <= fx && fx <= hi, "{shape:?} ({dx}, {dy}) が範囲 {lo}..{hi} の外");
                    }
                }
            }
        }
    }

    #[test]
    fn flat_tip_is_wide_across_and_thin_along_its_bristles() {
        let mut layer = Layer::new("t");
        let clip = Rect::from_size(200, 200);
        let tip = RoundTip { size: 60.0, hardness: 1.0, shape: TipShape::Flat, ..RoundTip::default() };
        tip.draw(&mut layer, sample(100.0, 100.0), clip);
        // 毛の並ぶ向き（右上がり 70°）には端近くまで届き、厚みの向きには短い
        let (c, s) = (FLAT_TIP_ANGLE.cos(), FLAT_TIP_ANGLE.sin());
        let along = (100.0 + 25.0 * c, 100.0 - 25.0 * s);
        let across = (100.0 + 15.0 * s, 100.0 + 15.0 * c);
        assert_eq!(layer.pixel(along.0 as i32, along.1 as i32).a, 255);
        assert_eq!(layer.pixel(across.0 as i32, across.1 as i32).a, 0);
        assert_eq!(layer.pixel(100, 100).a, 255);
    }

    #[test]
    fn soft_eraser_edge_partially_erases() {
        let mut layer = Layer::new("t");
        let clip = Rect::from_size(200, 200);
        let mut ink = InkPen { tip: RoundTip { size: 60.0, hardness: 1.0, ..RoundTip::default() } };
        ink.dab(&mut layer, sample(100.0, 100.0), clip);
        let mut eraser =
            Eraser { tip: RoundTip { size: 20.0, hardness: 0.0, mode: DabMode::Erase, ..RoundTip::default() } };
        eraser.dab(&mut layer, sample(100.0, 100.0), clip);
        let edge = layer.pixel(107, 100).a;
        assert!(edge > 0 && edge < 255, "edge alpha {edge}");
    }
}
