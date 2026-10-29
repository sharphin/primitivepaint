use pp_core::Rect;

/// これより多くなったら、全体を 1 つの矩形にまとめる。
const MAX_RECTS: usize = 256;

/// 描き直しが必要な所。離れた所が同時に変わっても間を描き直さないよう、矩形をいくつか持つ。
///
/// 長い直線の下書きのように細長く斜めに伸びるものを、外接矩形 1 つで描き直すと、
/// 間の何もない所まで描いて GPU に送ることになり重い。
#[derive(Default)]
pub struct DirtyRegion {
    rects: Vec<Rect>,
}

impl DirtyRegion {
    /// `rect` を足す。くっつけても無駄が増えない矩形とはまとめる。
    pub fn add(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        let mut merged = rect;
        // まとめた結果がさらに別の矩形とまとまることがあるので、まとまらなくなるまで繰り返す
        while let Some(i) = self.rects.iter().position(|r| worth_merging(r, &merged)) {
            merged = merged.union(&self.rects.swap_remove(i));
        }
        self.rects.push(merged);
        if self.rects.len() > MAX_RECTS {
            let all = self.bounds();
            self.rects = vec![all];
        }
    }

    /// すべてを囲む矩形。
    pub fn bounds(&self) -> Rect {
        self.rects.iter().fold(Rect::EMPTY, |all, r| all.union(r))
    }

    /// 持っている矩形を取り出して空にする。
    pub fn take(&mut self) -> Vec<Rect> {
        std::mem::take(&mut self.rects)
    }
}

/// 2 つをまとめても、描き直す面積が別々のときより増えないか。
fn worth_merging(a: &Rect, b: &Rect) -> bool {
    let area = |r: &Rect| r.width() as u64 * r.height() as u64;
    area(&a.union(b)) <= area(a) + area(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_apart_rects_stay_separate_and_overlapping_ones_merge() {
        let mut dirty = DirtyRegion::default();
        dirty.add(Rect::new(0, 0, 10, 10));
        dirty.add(Rect::new(1000, 1000, 1010, 1010));
        assert_eq!(dirty.rects.len(), 2, "離れているので間は描き直さない");
        dirty.add(Rect::new(5, 0, 15, 10));
        assert_eq!(dirty.rects.len(), 2);
        assert!(dirty.rects.contains(&Rect::new(0, 0, 15, 10)));
        dirty.add(Rect::new(0, 0, 2000, 2000));
        assert_eq!(dirty.take(), vec![Rect::new(0, 0, 2000, 2000)], "中に入るものはまとまる");
        assert!(dirty.rects.is_empty());
    }

    #[test]
    fn too_many_rects_become_one() {
        let mut dirty = DirtyRegion::default();
        for i in 0..(MAX_RECTS as i32 + 1) {
            dirty.add(Rect::new(i * 20, i * 20, i * 20 + 1, i * 20 + 1));
        }
        assert_eq!(dirty.take().len(), 1);
    }
}
