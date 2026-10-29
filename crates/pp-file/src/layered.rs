//! ほかのソフトのレイヤー（.psd、今後は .clip）を、primitivepaint の紙に変換する。
//!
//! 紙は不透明度・合成モード・マスク・フォルダを持たないので、見た目を保つように画素へ焼き込む。
//! - 不透明度とマスクは、画素の濃さに掛ける
//! - 通常の合成のレイヤーは、それぞれ 1 枚の透明フィルムにする
//! - ほかの合成モードとクリッピングは、下の紙に統合する
//! - 通過のフォルダは中身をそのまま並べ、それ以外のフォルダは中身を 1 枚にまとめる
//!
//! 変えたこと・読まなかったものは `notes` に残し、開いたときに一覧で見せる。

use pp_core::{CanvasMode, Document, Layer, Rect, Rgba8, SheetMaterial, TILE_SIZE};

use crate::blend::Blend;

/// 読み込む元のレイヤー 1 枚（またはフォルダ）。
pub struct LayerNode {
    pub name: String,
    pub visible: bool,
    /// 0..=1
    pub opacity: f32,
    pub blend: NodeBlend,
    /// すぐ下のレイヤーでクリッピングしているか。
    pub clip: bool,
    pub mask: Option<Mask>,
    pub content: Content,
}

/// レイヤーの合成モード。
#[derive(Clone, Debug, PartialEq)]
pub enum NodeBlend {
    Mode(Blend),
    /// フォルダの「通過」。
    PassThrough,
    /// 知らない合成モード（名前）。通常として扱う。
    Unknown(String),
}

pub enum Content {
    /// 作品の座標に置いた画素（premultiplied）。
    Pixels(Layer),
    /// フォルダの中身（下から順）。
    Folder(Vec<LayerNode>),
    /// 読めない種類のレイヤー。理由を添える。
    Unsupported(String),
}

/// レイヤーマスク。濃さは画素の `a` に入れ、タイルのない所は `fill` の濃さ。
pub struct Mask {
    pub layer: Layer,
    pub fill: u8,
}

/// 紙に変換した結果。
pub struct Imported {
    pub document: Document,
    pub notes: Vec<String>,
}

/// 1 枚の紙にまとまったもの。
struct Sheet {
    name: String,
    layer: Layer,
    visible: bool,
}

/// `nodes`（下から順）を、`width`×`height` の紙モードの作品にする。`ground` は用紙の地の色（なければ透明）。
pub fn convert(width: u32, height: u32, ground: Option<Rgba8>, nodes: Vec<LayerNode>) -> Option<Imported> {
    let mut converter = Converter { notes: Vec::new() };
    let mut sheets = Vec::new();
    converter.place_all(nodes, &mut sheets);
    let mut base = Layer::new("白地");
    if let Some(color) = ground {
        base.fill(Rect::from_size(width, height), color);
    }
    let document = assemble(width, height, base, sheets)?;
    Some(Imported { document, notes: converter.notes })
}

struct Converter {
    notes: Vec<String>,
}

/// 下から順のレイヤーを、土台と、それにクリッピングしたレイヤーの組に分ける。
fn group_clipping(nodes: Vec<LayerNode>) -> Vec<(LayerNode, Vec<LayerNode>)> {
    let mut groups: Vec<(LayerNode, Vec<LayerNode>)> = Vec::new();
    for node in nodes {
        match groups.last_mut() {
            Some((_, clipped)) if node.clip => clipped.push(node),
            _ => groups.push((node, Vec::new())),
        }
    }
    groups
}

impl Converter {
    fn note(&mut self, text: String) {
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

    /// レイヤーを順に紙にする。
    fn place_all(&mut self, nodes: Vec<LayerNode>, sheets: &mut Vec<Sheet>) {
        for (base, clipped) in group_clipping(nodes) {
            if base.blend == NodeBlend::PassThrough && clipped.is_empty() && base.mask.is_none() && base.opacity >= 1.0
            {
                let Content::Folder(children) = base.content else { continue };
                let start = sheets.len();
                self.place_all(children, sheets);
                if !base.visible {
                    self.note(format!("非表示のフォルダ「{}」の中身は、非表示の紙にしました", base.name));
                    sheets[start..].iter_mut().for_each(|s| s.visible = false);
                }
                continue;
            }
            if matches!(base.content, Content::Folder(_)) {
                self.note(format!("フォルダ「{}」の中身は、1 枚の紙にまとめました", base.name));
            }
            let (name, visible) = (base.name.clone(), base.visible);
            let blend = self.blend_of(&base);
            let Some(unit) = self.render_unit(base, clipped) else { continue };
            let alone = sheets.is_empty() && blend.looks_normal_on_nothing();
            if blend == Blend::Normal || alone {
                sheets.push(Sheet { name, layer: unit, visible });
            } else if !visible {
                self.note(format!("非表示の「{name}」は合成モードがあるため、読み込んでいません"));
            } else {
                if sheets.is_empty() {
                    sheets.push(Sheet { name: name.clone(), layer: Layer::new(""), visible: true });
                }
                let below = sheets.last_mut().expect("今あることを確かめた");
                let note = format!("「{name}」は合成モードがあるため、下の「{}」に統合しました", below.name);
                blend_layer(&mut below.layer, &unit, blend, 1.0, false);
                self.note(note);
            }
        }
    }

    /// `base` とそれにクリッピングしたレイヤーを 1 枚の画素にし、不透明度とマスクを焼き込む。
    fn render_unit(&mut self, base: LayerNode, clipped: Vec<LayerNode>) -> Option<Layer> {
        let base_name = base.name.clone();
        let opacity = base.opacity;
        let Some(mut unit) = self.render_single(base) else {
            if !clipped.is_empty() {
                self.note(format!("「{base_name}」にクリッピングしたレイヤーは、下が読めないので読み込んでいません"));
            }
            return None;
        };
        for node in clipped {
            if !node.visible {
                self.note(format!("非表示の「{}」（クリッピング）は読み込んでいません", node.name));
                continue;
            }
            let (name, layer_opacity) = (node.name.clone(), node.opacity);
            let blend = self.blend_of(&node);
            // クリッピングするレイヤー自身の不透明度は、重ねるときに掛ける
            let Some(pixels) = self.render_single(node) else { continue };
            blend_layer(&mut unit, &pixels, blend, layer_opacity, true);
            self.note(format!("「{name}」はクリッピングのため、「{base_name}」に統合しました"));
        }
        if opacity < 1.0 {
            scale_layer(&mut unit, opacity);
        }
        Some(unit)
    }

    /// レイヤー 1 枚（フォルダなら中身をまとめたもの）の画素。マスクは焼き込み、不透明度はまだ掛けない。
    fn render_single(&mut self, node: LayerNode) -> Option<Layer> {
        let mut pixels = match node.content {
            Content::Pixels(layer) => layer,
            Content::Folder(children) => {
                let mut merged = Layer::new("");
                self.flatten(children, &mut merged);
                merged
            }
            Content::Unsupported(reason) => {
                self.note(format!("「{}」は読み込んでいません（{reason}）", node.name));
                return None;
            }
        };
        // 何も隠さないマスク（真っ白）は、知らせるまでもない
        if let Some(mask) = node.mask
            && apply_mask(&mut pixels, &mask)
        {
            self.note(format!("「{}」のレイヤーマスクは、画素に焼き込みました", node.name));
        }
        Some(pixels)
    }

    /// フォルダの中身を、見た目どおりに `out` へ重ねる（非表示のものは描かない）。
    fn flatten(&mut self, nodes: Vec<LayerNode>, out: &mut Layer) {
        for (base, clipped) in group_clipping(nodes) {
            if !base.visible {
                continue;
            }
            if base.blend == NodeBlend::PassThrough && clipped.is_empty() && base.mask.is_none() && base.opacity >= 1.0
            {
                if let Content::Folder(children) = base.content {
                    self.flatten(children, out);
                }
                continue;
            }
            let blend = self.blend_of(&base);
            let Some(unit) = self.render_unit(base, clipped) else { continue };
            blend_layer(out, &unit, blend, 1.0, false);
        }
    }

    fn blend_of(&mut self, node: &LayerNode) -> Blend {
        match &node.blend {
            NodeBlend::Mode(blend) => *blend,
            NodeBlend::PassThrough => Blend::Normal,
            NodeBlend::Unknown(key) => {
                self.note(format!("「{}」の合成モード（{key}）は知らないため、通常として読みました", node.name));
                Blend::Normal
            }
        }
    }
}

/// 紙を並べて作品にする。一番下の表示している紙を「用紙」に、ほかは透明フィルムにする。
fn assemble(width: u32, height: u32, ground: Layer, sheets: Vec<Sheet>) -> Option<Document> {
    let mut layers = vec![ground];
    let mut rest = sheets.into_iter().peekable();
    let paper = match rest.next_if(|first| first.visible) {
        Some(first) => rename(first.layer, format!("用紙（{}）", first.name), SheetMaterial::Paper),
        None => Layer::new("用紙"),
    };
    layers.push(paper);
    for sheet in rest {
        let mut layer = rename(sheet.layer, sheet.name, SheetMaterial::ClearFilm);
        layer.visible = sheet.visible;
        layers.push(layer);
    }
    let active = layers.len() - 1;
    let bounds = Rect::from_size(width, height);
    for layer in &mut layers {
        crop(layer, bounds);
    }
    Document::from_layers(width, height, CanvasMode::Paper, layers, active)
}

/// `layer` の中身を、名前と素材を付けた新しいレイヤーに移す。透明なタイルは捨てる。
fn rename(layer: Layer, name: String, material: SheetMaterial) -> Layer {
    let mut out = Layer::with_material(name, material);
    for ((tx, ty), tile) in layer.tiles() {
        if !tile.is_transparent() {
            out.insert_tile(tx, ty, tile.clone());
        }
    }
    out
}

/// 作品の外にはみ出した画素を消す。
fn crop(layer: &mut Layer, bounds: Rect) {
    let ts = TILE_SIZE as i32;
    let keys: Vec<(i32, i32)> = layer.tiles().map(|(key, _)| key).collect();
    for (tx, ty) in keys {
        let tile_rect = Rect::new(tx * ts, ty * ts, (tx + 1) * ts, (ty + 1) * ts);
        let inside = tile_rect.intersect(&bounds);
        if inside == tile_rect {
            continue;
        }
        let tile = layer.tile_mut_or_create(tx, ty);
        for y in 0..TILE_SIZE {
            let py = ty * ts + y as i32;
            for (x, p) in tile.row_mut(y).iter_mut().enumerate() {
                if !inside.contains((tx * ts + x as i32) as f32, py as f32) {
                    *p = Rgba8::default();
                }
            }
        }
    }
}

/// `src` を `dst` に `blend` で重ねる。`clip` なら `dst` の不透明な所にだけ。
fn blend_layer(dst: &mut Layer, src: &Layer, blend: Blend, opacity: f32, clip: bool) {
    for ((tx, ty), tile) in src.tiles() {
        if clip && dst.tile(tx, ty).is_none() {
            continue;
        }
        let target = dst.tile_mut_or_create(tx, ty);
        for y in 0..TILE_SIZE {
            for (d, s) in target.row_mut(y).iter_mut().zip(tile.row(y)) {
                if s.a != 0 {
                    *d = blend.composite(*d, *s, opacity, clip);
                }
            }
        }
    }
}

/// 不透明度を掛ける。
fn scale_layer(layer: &mut Layer, opacity: f32) {
    let factor = (opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    let keys: Vec<(i32, i32)> = layer.tiles().map(|(key, _)| key).collect();
    for (tx, ty) in keys {
        let tile = layer.tile_mut_or_create(tx, ty);
        for y in 0..TILE_SIZE {
            for p in tile.row_mut(y) {
                *p = scale_pixel(*p, factor);
            }
        }
    }
}

/// マスクの濃さを掛ける。描いてある所が少しでも隠れたら true。
fn apply_mask(layer: &mut Layer, mask: &Mask) -> bool {
    let mut hid = false;
    let keys: Vec<(i32, i32)> = layer.tiles().map(|(key, _)| key).collect();
    for (tx, ty) in keys {
        let mask_tile = mask.layer.tile(tx, ty);
        if mask_tile.is_none() && mask.fill == 255 {
            continue;
        }
        let tile = layer.tile_mut_or_create(tx, ty);
        for y in 0..TILE_SIZE {
            for (x, p) in tile.row_mut(y).iter_mut().enumerate() {
                let value = mask_tile.map_or(mask.fill, |m| m.get(x as u32, y).a);
                if value < 255 && p.a > 0 {
                    *p = scale_pixel(*p, value as u32);
                    hid = true;
                }
            }
        }
    }
    hid
}

fn scale_pixel(p: Rgba8, factor: u32) -> Rgba8 {
    let mul = |v: u8| ((v as u32 * factor + 127) / 255) as u8;
    Rgba8 { r: mul(p.r), g: mul(p.g), b: mul(p.b), a: mul(p.a) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba8 = Rgba8 { r: 255, g: 0, b: 0, a: 255 };
    const GRAY: Rgba8 = Rgba8 { r: 128, g: 128, b: 128, a: 255 };

    fn filled(color: Rgba8) -> Layer {
        let mut layer = Layer::new("");
        layer.fill(Rect::from_size(4, 4), color);
        layer
    }

    fn node(name: &str, content: Content) -> LayerNode {
        LayerNode {
            name: name.into(),
            visible: true,
            opacity: 1.0,
            blend: NodeBlend::Mode(Blend::Normal),
            clip: false,
            mask: None,
            content,
        }
    }

    #[test]
    fn normal_layers_become_sheets_and_opacity_is_baked() {
        let mut top = node("上", Content::Pixels(filled(RED)));
        top.opacity = 0.5;
        let imported = convert(4, 4, None, vec![node("下", Content::Pixels(filled(GRAY))), top]).unwrap();
        let doc = &imported.document;
        assert_eq!(doc.layers.len(), 3, "白地・用紙・透明フィルム");
        assert_eq!(doc.layers[1].material, SheetMaterial::Paper);
        assert_eq!(doc.layers[2].material, SheetMaterial::ClearFilm);
        assert_eq!(doc.layers[2].pixel(0, 0).a, 128, "不透明度を焼き込む");
    }

    #[test]
    fn multiply_merges_into_the_sheet_below() {
        let mut top = node("影", Content::Pixels(filled(GRAY)));
        top.blend = NodeBlend::Mode(Blend::Multiply);
        let imported = convert(4, 4, None, vec![node("色", Content::Pixels(filled(RED))), top]).unwrap();
        assert_eq!(imported.document.layers.len(), 2);
        let px = imported.document.layers[1].pixel(0, 0);
        assert!((127..=129).contains(&px.r) && px.g == 0, "{px:?}");
        assert!(imported.notes.iter().any(|n| n.contains("統合")));
    }

    #[test]
    fn pass_through_folder_keeps_its_layers_and_others_become_one() {
        let inner = || vec![node("a", Content::Pixels(filled(RED))), node("b", Content::Pixels(filled(GRAY)))];
        let mut open = node("通過", Content::Folder(inner()));
        open.blend = NodeBlend::PassThrough;
        let closed = node("通常", Content::Folder(inner()));
        let imported = convert(4, 4, None, vec![open, closed]).unwrap();
        // 用紙(a) + b + まとめた 1 枚
        assert_eq!(imported.document.layers.len(), 4);
        assert_eq!(imported.document.layers[3].pixel(0, 0), GRAY);
    }

    #[test]
    fn clipping_stays_inside_the_layer_below() {
        let mut base = Layer::new("");
        base.fill(Rect::new(0, 0, 2, 4), GRAY);
        let mut clipped = node("塗り", Content::Pixels(filled(RED)));
        clipped.clip = true;
        let imported = convert(4, 4, None, vec![node("線", Content::Pixels(base)), clipped]).unwrap();
        let paper = &imported.document.layers[1];
        assert_eq!(paper.pixel(0, 0), RED);
        assert_eq!(paper.pixel(3, 0), Rgba8::default());
    }

    #[test]
    fn mask_and_hidden_layers() {
        let mut mask = Layer::new("");
        mask.fill(Rect::new(0, 0, 2, 4), Rgba8 { r: 0, g: 0, b: 0, a: 255 });
        let mut masked = node("マスク", Content::Pixels(filled(RED)));
        masked.mask = Some(Mask { layer: mask, fill: 0 });
        let mut hidden = node("隠す", Content::Pixels(filled(GRAY)));
        hidden.visible = false;
        let imported = convert(4, 4, Some(GRAY), vec![masked, hidden]).unwrap();
        let doc = &imported.document;
        assert_eq!(doc.layers[0].pixel(3, 3), GRAY, "用紙の地の色");
        assert_eq!(doc.layers[1].pixel(0, 0), RED);
        assert_eq!(doc.layers[1].pixel(3, 0).a, 0, "マスクの外");
        assert!(!doc.layers[2].visible, "非表示の紙として残す");
    }
}
