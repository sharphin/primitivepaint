use crate::Rgba8;

/// タイル一辺のピクセル数。大きいキャンバスでも描いた部分だけメモリを使うよう、
/// レイヤーはこのサイズのタイルを疎に持つ。
pub const TILE_SIZE: u32 = 256;

const TILE_PIXELS: usize = (TILE_SIZE * TILE_SIZE) as usize;

/// 1 タイル分のピクセル（行優先）。
#[derive(Clone)]
pub struct Tile {
    pixels: Box<[Rgba8]>,
}

impl Tile {
    pub fn transparent() -> Self {
        Tile { pixels: vec![Rgba8::default(); TILE_PIXELS].into_boxed_slice() }
    }

    /// 保存したピクセル列から作る。長さが 1 タイル分でなければ `None`。
    pub fn from_pixels(pixels: Vec<Rgba8>) -> Option<Self> {
        (pixels.len() == TILE_PIXELS).then(|| Tile { pixels: pixels.into_boxed_slice() })
    }

    /// 全ピクセル（行優先）。
    pub fn pixels(&self) -> &[Rgba8] {
        &self.pixels
    }

    /// 1 ピクセルも描かれていない（すべて透明）か。
    pub fn is_transparent(&self) -> bool {
        self.pixels.iter().all(|p| p.a == 0)
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> Rgba8 {
        self.pixels[(y * TILE_SIZE + x) as usize]
    }

    #[inline]
    pub fn get_mut(&mut self, x: u32, y: u32) -> &mut Rgba8 {
        &mut self.pixels[(y * TILE_SIZE + x) as usize]
    }

    pub fn row(&self, y: u32) -> &[Rgba8] {
        let start = (y * TILE_SIZE) as usize;
        &self.pixels[start..start + TILE_SIZE as usize]
    }

    pub fn row_mut(&mut self, y: u32) -> &mut [Rgba8] {
        let start = (y * TILE_SIZE) as usize;
        &mut self.pixels[start..start + TILE_SIZE as usize]
    }
}

/// ピクセル座標からタイル座標とタイル内オフセットを求める（負の座標にも対応）。
#[inline]
pub fn split_coord(v: i32) -> (i32, u32) {
    let t = TILE_SIZE as i32;
    (v.div_euclid(t), v.rem_euclid(t) as u32)
}
