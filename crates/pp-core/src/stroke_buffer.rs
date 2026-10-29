use std::collections::HashMap;

use crate::layer::{Layer, LayerSnapshot};
use crate::pen::{Footprint, InputSample, RoundTip};
use crate::tile::{TILE_SIZE, Tile, split_coord};
use crate::{Rect, Rgba8};

/// 覆い具合 1 の画素に `strength`（0..=1）を掛けた量で覆う筆の、`cover_dab` に渡す最大の量。
pub(crate) fn ceiling(strength: f32) -> u8 {
    (strength.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// この面積（ピクセル）より大きいダブは、タイルごとに手分けして描く。小さいダブでは手分けの手間のほうがかかる。
const PARALLEL_AREA: u32 = 128 * 128;

/// `cover_dab` で 1 タイル分を描くための材料。
struct TileJob<'a> {
    key: (i32, i32),
    cells: &'a mut Box<[u8]>,
    target: &'a mut Tile,
    before: Option<&'a Tile>,
}

/// 描いているダブ 1 つ。
struct Dab {
    footprint: Footprint,
    sample: InputSample,
    bounds: Rect,
    ceiling: u8,
}

impl Dab {
    /// `job` のタイルのうち、このダブがかかる所を描き、書き換えた範囲を返す。
    fn cover_tile(
        &self,
        job: &mut TileJob,
        amount: &impl Fn(i32, i32, f32) -> u32,
        compose: &impl Fn(Rgba8, u8) -> Rgba8,
    ) -> Rect {
        let ts = TILE_SIZE as i32;
        let (tx, ty) = job.key;
        let r = self.bounds.intersect(&Rect::new(tx * ts, ty * ts, (tx + 1) * ts, (ty + 1) * ts));
        let (sx, sy) = (self.sample.x, self.sample.y);
        let mut wrote = Rect::EMPTY;
        for y in r.y0..r.y1 {
            let oy = (y - ty * ts) as u32;
            let fy = y as f32 + 0.5 - sy;
            let Some((lo, hi)) = self.footprint.row_span(fy) else {
                continue;
            };
            // 画素の中心 x + 0.5 が中心から lo..=hi にある画素だけを見る
            let from = ((sx - 0.5 + lo).floor() as i32).max(r.x0);
            let to = ((sx - 0.5 + hi).ceil() as i32 + 1).min(r.x1);
            let row = &mut job.cells[(oy * TILE_SIZE) as usize..((oy + 1) * TILE_SIZE) as usize];
            let (mut first, mut last) = (i32::MAX, i32::MIN);
            for x in from..to {
                let ox = (x - tx * ts) as u32;
                let cell = &mut row[ox as usize];
                if *cell >= self.ceiling {
                    continue;
                }
                let Some(coverage) = self.footprint.coverage(x as f32 + 0.5 - sx, fy) else {
                    continue;
                };
                let a = amount(x, y, coverage).min(255) as u8;
                if a <= *cell {
                    continue;
                }
                *cell = a;
                let below = job.before.map_or(Rgba8::default(), |t| t.get(ox, oy));
                *job.target.get_mut(ox, oy) = compose(below, a);
                first = first.min(x);
                last = x;
            }
            if first <= last {
                wrote = wrote.union(&Rect::new(first, y, last + 1, y + 1));
            }
        }
        wrote
    }
}

/// 1 ストロークの間だけ使う下書き。描き始める前のレイヤーと、ストロークが各画素をどれだけ覆ったかを持つ。
///
/// 同じストロークの中でダブが重なっても濃くならず、ストロークを重ねたときだけ濃くなる。
/// 鉛筆や水彩のように「1 回なぞった濃さ」が決まっている画材に使う。
pub struct StrokeBuffer {
    before: LayerSnapshot,
    coverage: HashMap<(i32, i32), Box<[u8]>>,
}

impl StrokeBuffer {
    pub fn new(layer: &Layer) -> Self {
        StrokeBuffer { before: layer.snapshot(), coverage: HashMap::new() }
    }

    /// `tip` のダブ 1 つ分を覆う。各画素の覆い具合（0..=1）から `amount(x, y, 覆い具合)` で覆う量（0..=255）を決め、
    /// このストロークでそれまでより多く覆ったところにだけ、`compose(描く前の画素, 量)` を書く。書き換えた範囲を返す。
    ///
    /// `ceiling` は `amount` が返しうる最大の量。すでにそこまで覆った画素は、覆い具合を計算せずに飛ばす。
    /// タイルを探すのは 1 タイルにつき 1 回だけにし、太い筆ではタイルごとに手分けして同時に描く。
    #[allow(clippy::too_many_arguments)]
    pub fn cover_dab(
        &mut self,
        layer: &mut Layer,
        tip: &RoundTip,
        sample: InputSample,
        clip: Rect,
        ceiling: u8,
        amount: impl Fn(i32, i32, f32) -> u32 + Sync,
        compose: impl Fn(Rgba8, u8) -> Rgba8 + Sync,
    ) -> Rect {
        let bounds = tip.bounds(sample, clip);
        if bounds.is_empty() || ceiling == 0 {
            return Rect::EMPTY;
        }
        let (tx0, _) = split_coord(bounds.x0);
        let (ty0, _) = split_coord(bounds.y0);
        let (tx1, _) = split_coord(bounds.x1 - 1);
        let (ty1, _) = split_coord(bounds.y1 - 1);
        // 何も描かなかったタイルは、あとで消す（新しく作ったときだけ）
        let mut created = Vec::new();
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                if layer.tile(tx, ty).is_none() {
                    created.push((tx, ty));
                }
                self.coverage
                    .entry((tx, ty))
                    .or_insert_with(|| vec![0u8; (TILE_SIZE * TILE_SIZE) as usize].into_boxed_slice());
            }
        }
        let mut covered: Vec<_> = self
            .coverage
            .iter_mut()
            .filter(|((tx, ty), _)| (tx0..=tx1).contains(tx) && (ty0..=ty1).contains(ty))
            .collect();
        covered.sort_by_key(|(key, _)| **key);
        let mut targets = layer.tiles_mut_in(tx0, ty0, tx1, ty1);
        targets.sort_by_key(|(key, _)| *key);
        let dab = Dab { footprint: tip.footprint(sample.pressure), sample, bounds, ceiling };
        let before = &self.before;
        let mut jobs: Vec<TileJob> = covered
            .into_iter()
            .zip(targets)
            .map(|((&key, cells), (_, target))| TileJob { key, cells, target, before: before.tile(key.0, key.1) })
            .collect();
        let run = |job: &mut TileJob| dab.cover_tile(job, &amount, &compose);
        let written: Vec<Rect> = if bounds.width() * bounds.height() < PARALLEL_AREA || jobs.len() < 2 {
            jobs.iter_mut().map(run).collect()
        } else {
            let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(jobs.len());
            let per_worker = jobs.len().div_ceil(workers);
            std::thread::scope(|scope| {
                let handles: Vec<_> = jobs
                    .chunks_mut(per_worker)
                    .map(|chunk| scope.spawn(|| chunk.iter_mut().map(run).collect::<Vec<Rect>>()))
                    .collect();
                handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
            })
        };
        let unused: Vec<(i32, i32)> = jobs
            .iter()
            .zip(&written)
            .filter(|(job, rect)| rect.is_empty() && created.contains(&job.key))
            .map(|(job, _)| job.key)
            .collect();
        drop(jobs);
        for (tx, ty) in unused {
            layer.remove_tile(tx, ty);
        }
        written.iter().fold(Rect::EMPTY, |region, rect| region.union(rect))
    }

    /// (x, y) をストロークが `amount`（0..=255）だけ覆ったことを記録し、覆い具合が増えたときは
    /// `compose(描く前の画素, 覆い具合)` の結果をレイヤーに書く。
    pub fn cover(&mut self, layer: &mut Layer, x: i32, y: i32, amount: u8, compose: impl Fn(Rgba8, u8) -> Rgba8) {
        let (tx, ox) = split_coord(x);
        let (ty, oy) = split_coord(y);
        let tile = self
            .coverage
            .entry((tx, ty))
            .or_insert_with(|| vec![0u8; (TILE_SIZE * TILE_SIZE) as usize].into_boxed_slice());
        let cell = &mut tile[(oy * TILE_SIZE + ox) as usize];
        if amount <= *cell {
            return;
        }
        *cell = amount;
        layer.set_pixel(x, y, compose(self.before.pixel(x, y), amount));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_cover_in_one_stroke_does_not_build_up() {
        let mut layer = Layer::new("t");
        let mut buffer = StrokeBuffer::new(&layer);
        let half = |_below: Rgba8, a: u8| Rgba8 { r: 0, g: 0, b: 0, a };
        buffer.cover(&mut layer, 3, 3, 128, half);
        buffer.cover(&mut layer, 3, 3, 128, half);
        buffer.cover(&mut layer, 3, 3, 100, half);
        assert_eq!(layer.pixel(3, 3).a, 128);
        buffer.cover(&mut layer, 3, 3, 200, half);
        assert_eq!(layer.pixel(3, 3).a, 200);
    }
}
