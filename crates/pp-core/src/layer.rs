use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::tile::{TILE_SIZE, Tile, split_coord};
use crate::{Rect, Rgba8};

/// レイヤーの合成モード。CLIP STUDIO の合成モードは今後ここに追加していく。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
}

/// 重ねた紙の素材。利用者から見たレイヤーの種類で、透け方は素材で決まる。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SheetMaterial {
    /// 一番下の用紙（油彩ではキャンバス）。外したり動かしたりできない。
    #[default]
    Paper,
    /// トレーシングペーパー。下が半透明に透ける。
    TracingPaper,
    /// 透明フィルム（セル）。下が完全に透ける。
    ClearFilm,
}

impl SheetMaterial {
    pub fn name(&self) -> &str {
        match self {
            SheetMaterial::Paper => "用紙",
            SheetMaterial::TracingPaper => "トレーシングペーパー",
            SheetMaterial::ClearFilm => "透明フィルム",
        }
    }

    /// この紙を重ねたとき、下にあるものに掛かる白いもや（premultiplied）。透明なら `None`。
    pub fn veil(&self) -> Option<Rgba8> {
        match self {
            SheetMaterial::TracingPaper => Some(Rgba8 { r: 150, g: 150, b: 150, a: 150 }),
            SheetMaterial::Paper | SheetMaterial::ClearFilm => None,
        }
    }
}

static NEXT_LAYER_ID: AtomicU64 = AtomicU64::new(1);

/// ラスターレイヤー。タイルは描かれた場所だけ確保される。
pub struct Layer {
    /// 並べ替えても変わらない番号。
    id: u64,
    pub name: String,
    pub material: SheetMaterial,
    pub visible: bool,
    /// 0..=255
    pub opacity: u8,
    pub blend: BlendMode,
    /// タイルは `Arc` で共有し、書き込むときだけ複製する（コピーオンライト）。
    /// これによりストローク中のスナップショット（`StrokeBuffer`）を安く取れる。
    tiles: HashMap<(i32, i32), Arc<Tile>>,
}

/// ある時点のレイヤーの中身。タイルを共有するので、取るだけならほぼコストがかからない。
#[derive(Clone)]
pub struct LayerSnapshot {
    tiles: HashMap<(i32, i32), Arc<Tile>>,
}

impl LayerSnapshot {
    pub fn tile(&self, tx: i32, ty: i32) -> Option<&Tile> {
        self.tiles.get(&(tx, ty)).map(|t| t.as_ref())
    }

    pub fn pixel(&self, x: i32, y: i32) -> Rgba8 {
        let (tx, ox) = split_coord(x);
        let (ty, oy) = split_coord(y);
        self.tiles.get(&(tx, ty)).map(|t| t.get(ox, oy)).unwrap_or_default()
    }
}

impl Layer {
    pub fn new(name: impl Into<String>) -> Self {
        Layer::with_material(name, SheetMaterial::Paper)
    }

    pub fn with_material(name: impl Into<String>, material: SheetMaterial) -> Self {
        Layer {
            id: NEXT_LAYER_ID.fetch_add(1, Ordering::Relaxed),
            name: name.into(),
            material,
            visible: true,
            opacity: 255,
            blend: BlendMode::Normal,
            tiles: HashMap::new(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn tile(&self, tx: i32, ty: i32) -> Option<&Tile> {
        self.tiles.get(&(tx, ty)).map(|t| t.as_ref())
    }

    /// 書き込み用にタイルを取り出す。スナップショットと共有中なら、ここで複製される。
    pub fn tile_mut_or_create(&mut self, tx: i32, ty: i32) -> &mut Tile {
        let shared = self.tiles.entry((tx, ty)).or_insert_with(|| Arc::new(Tile::transparent()));
        Arc::make_mut(shared)
    }

    pub fn snapshot(&self) -> LayerSnapshot {
        LayerSnapshot { tiles: self.tiles.clone() }
    }

    /// 確保されているタイルを `((タイル x, タイル y), タイル)` で返す。順番は決まっていない。
    pub fn tiles(&self) -> impl Iterator<Item = ((i32, i32), &Tile)> {
        self.tiles.iter().map(|(key, tile)| (*key, tile.as_ref()))
    }

    /// (tx, ty) のタイルを置き換える（ファイルから読み込むときに使う）。
    pub fn insert_tile(&mut self, tx: i32, ty: i32, tile: Tile) {
        self.tiles.insert((tx, ty), Arc::new(tile));
    }

    /// タイル (tx0..=tx1, ty0..=ty1) をすべて書き込み用に取り出す（ないタイルは作る）。
    /// 別々のタイルを同時に書けるよう、まとめて借りる。
    pub(crate) fn tiles_mut_in(&mut self, tx0: i32, ty0: i32, tx1: i32, ty1: i32) -> Vec<((i32, i32), &mut Tile)> {
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                self.tiles.entry((tx, ty)).or_insert_with(|| Arc::new(Tile::transparent()));
            }
        }
        self.tiles
            .iter_mut()
            .filter(|((tx, ty), _)| (tx0..=tx1).contains(tx) && (ty0..=ty1).contains(ty))
            .map(|(key, tile)| (*key, Arc::make_mut(tile)))
            .collect()
    }

    pub(crate) fn remove_tile(&mut self, tx: i32, ty: i32) {
        self.tiles.remove(&(tx, ty));
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn pixel(&self, x: i32, y: i32) -> Rgba8 {
        let (tx, ox) = split_coord(x);
        let (ty, oy) = split_coord(y);
        self.tile(tx, ty).map(|t| t.get(ox, oy)).unwrap_or_default()
    }

    pub fn set_pixel(&mut self, x: i32, y: i32, px: Rgba8) {
        let (tx, ox) = split_coord(x);
        let (ty, oy) = split_coord(y);
        *self.tile_mut_or_create(tx, ty).get_mut(ox, oy) = px;
    }

    /// 単色で塗りつぶす（背景用紙レイヤーなど）。`bounds` の範囲のタイルを確保する。
    pub fn fill(&mut self, bounds: Rect, px: Rgba8) {
        let ts = TILE_SIZE as i32;
        let (tx0, _) = split_coord(bounds.x0);
        let (ty0, _) = split_coord(bounds.y0);
        let (tx1, _) = split_coord(bounds.x1 - 1);
        let (ty1, _) = split_coord(bounds.y1 - 1);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let tile = self.tile_mut_or_create(tx, ty);
                let r = bounds.intersect(&Rect::new(tx * ts, ty * ts, (tx + 1) * ts, (ty + 1) * ts));
                for y in r.y0..r.y1 {
                    let row = tile.row_mut((y - ty * ts) as u32);
                    row[(r.x0 - tx * ts) as usize..(r.x1 - tx * ts) as usize].fill(px);
                }
            }
        }
    }
}
