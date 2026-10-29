//! 書き出すときに、合成した絵を指定の大きさに拡大・縮小する。
//!
//! 大きいキャンバスでもメモリを使いすぎないよう、出力を何行かずつまとめて作る。
//! 縮小は面積の平均（細かい線がちらつかない）、拡大は双線形補間で、どちらも premultiplied のまま計算する。

use pp_core::{Document, Rect, Rgba8};

/// 一度に作る出力の行数。
const ROWS_PER_BAND: u32 = 256;

/// 出力の 1 画素に寄与する元の画素と、その重み（合計 1）。
type Taps = Vec<(usize, f32)>;

/// 書き出す絵の大きさ（ピクセル）。作品と違う大きさなら拡大・縮小する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputSize {
    width: u32,
    height: u32,
}

impl OutputSize {
    pub fn new(width: u32, height: u32) -> Self {
        OutputSize { width: width.max(1), height: height.max(1) }
    }

    /// 作品と同じ大きさ。
    pub fn of(document: &Document) -> Self {
        OutputSize::new(document.width, document.height)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// `document` を見た目どおりに合成してこの大きさにし、上の行から順に 1 行ずつ `row` に渡す。
    pub fn for_each_row(&self, document: &Document, mut row: impl FnMut(&[Rgba8])) {
        let (source_w, source_h) = (document.width, document.height);
        if (self.width, self.height) == (source_w, source_h) {
            self.unscaled_rows(document, &mut row);
            return;
        }
        let columns = taps(source_w as usize, self.width as usize);
        let lines = taps(source_h as usize, self.height as usize);
        let mut out = vec![Rgba8::default(); self.width as usize];
        for y0 in (0..self.height as usize).step_by(ROWS_PER_BAND as usize) {
            let y1 = (y0 + ROWS_PER_BAND as usize).min(self.height as usize);
            // この帯の出力に要る元の行だけを合成し、先に横方向を縮める
            let first = lines[y0].iter().map(|&(i, _)| i).min().unwrap_or(0);
            let last = lines[y1 - 1].iter().map(|&(i, _)| i).max().unwrap_or(first);
            let mut source = vec![Rgba8::default(); source_w as usize * (last - first + 1)];
            document.composite(Rect::new(0, first as i32, source_w as i32, last as i32 + 1), &mut source);
            let narrowed: Vec<Vec<[f32; 4]>> =
                source.chunks_exact(source_w as usize).map(|line| resample_line(line, &columns)).collect();
            for taps in &lines[y0..y1] {
                for (x, pixel) in out.iter_mut().enumerate() {
                    let mut sum = [0.0f32; 4];
                    for &(i, weight) in taps {
                        let p = narrowed[i - first][x];
                        for c in 0..4 {
                            sum[c] += p[c] * weight;
                        }
                    }
                    *pixel = to_pixel(sum);
                }
                row(&out);
            }
        }
    }

    fn unscaled_rows(&self, document: &Document, row: &mut impl FnMut(&[Rgba8])) {
        let (width, height) = (self.width, self.height);
        let mut band = vec![Rgba8::default(); width as usize * ROWS_PER_BAND as usize];
        for y0 in (0..height).step_by(ROWS_PER_BAND as usize) {
            let y1 = (y0 + ROWS_PER_BAND).min(height);
            document.composite(Rect::new(0, y0 as i32, width as i32, y1 as i32), &mut band);
            for line in band[..width as usize * (y1 - y0) as usize].chunks_exact(width as usize) {
                row(line);
            }
        }
    }
}

/// 元の長さ `source` を `target` にするときの、出力の各画素の重み。
fn taps(source: usize, target: usize) -> Vec<Taps> {
    let scale = source as f32 / target as f32;
    (0..target)
        .map(|i| {
            let taps: Taps = if scale > 1.0 {
                // 縮小：出力の 1 画素が覆う元の範囲を、重なった長さで平均する
                let (from, to) = (i as f32 * scale, (i + 1) as f32 * scale);
                (from.floor() as usize..(to.ceil() as usize).min(source))
                    .map(|s| (s, ((s + 1) as f32).min(to) - (s as f32).max(from)))
                    .filter(|&(_, w)| w > 0.0)
                    .collect()
            } else {
                // 拡大：出力の画素の中心を元の座標に置き、両隣の画素から補間する
                let center = ((i as f32 + 0.5) * scale - 0.5).clamp(0.0, (source - 1) as f32);
                let left = center.floor() as usize;
                let right = (left + 1).min(source - 1);
                let t = center - left as f32;
                vec![(left, 1.0 - t), (right, t)]
            };
            let total: f32 = taps.iter().map(|&(_, w)| w).sum();
            taps.into_iter().map(|(s, w)| (s, w / total)).collect()
        })
        .collect()
}

fn resample_line(line: &[Rgba8], columns: &[Taps]) -> Vec<[f32; 4]> {
    columns
        .iter()
        .map(|taps| {
            let mut sum = [0.0f32; 4];
            for &(i, weight) in taps {
                let p = line[i];
                for (c, v) in [p.r, p.g, p.b, p.a].into_iter().enumerate() {
                    sum[c] += v as f32 * weight;
                }
            }
            sum
        })
        .collect()
}

fn to_pixel(sum: [f32; 4]) -> Rgba8 {
    let to_u8 = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    let a = to_u8(sum[3]);
    // premultiplied なので、色は不透明度を超えない
    Rgba8 { r: to_u8(sum[0]).min(a), g: to_u8(sum[1]).min(a), b: to_u8(sum[2]).min(a), a }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pp_core::CanvasMode;

    fn rows(document: &Document, size: OutputSize) -> Vec<Vec<Rgba8>> {
        let mut found = Vec::new();
        size.for_each_row(document, |row| found.push(row.to_vec()));
        found
    }

    #[test]
    fn halving_averages_each_two_by_two_block() {
        let mut document = Document::new(4, 2, CanvasMode::Paper);
        let black = Rgba8 { r: 0, g: 0, b: 0, a: 255 };
        document.layers[1].set_pixel(0, 0, black);
        document.layers[1].set_pixel(1, 1, black);
        let found = rows(&document, OutputSize::new(2, 1));
        assert_eq!(found.len(), 1);
        // 白い紙の上の黒 2 画素と白 2 画素で、灰色になる
        let gray = found[0][0];
        assert!((126..=129).contains(&gray.r) && gray.a == 255, "{gray:?}");
        assert_eq!(found[0][1], Rgba8 { r: 255, g: 255, b: 255, a: 255 });
    }

    #[test]
    fn enlarging_keeps_the_size_and_blends_edges() {
        let mut document = Document::new(300, 300, CanvasMode::Paper);
        for y in 0..300 {
            for x in 0..150 {
                document.layers[1].set_pixel(x, y, Rgba8 { r: 0, g: 0, b: 0, a: 255 });
            }
        }
        let found = rows(&document, OutputSize::new(525, 525));
        assert_eq!((found.len(), found[0].len()), (525, 525));
        assert_eq!(found[400][10].r, 0);
        assert_eq!(found[400][500].r, 255);
        let edge = found[400][262].r;
        assert!(edge > 0 && edge < 255, "境目はなめらか: {edge}");
    }

    #[test]
    fn same_size_is_the_plain_composite() {
        let mut document = Document::new(3, 3, CanvasMode::Paper);
        document.layers[1].set_pixel(2, 2, Rgba8 { r: 0, g: 0, b: 255, a: 255 });
        let found = rows(&document, OutputSize::of(&document));
        assert_eq!(found[2][2], Rgba8 { r: 0, g: 0, b: 255, a: 255 });
    }
}
