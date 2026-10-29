/// 筆圧カーブ。ペンタブから来た筆圧（入力）を、描画に使う筆圧（出力）に変換する。
///
/// CLIP STUDIO の筆圧設定と同じく、制御点を結んだ折れ線で表す。
/// 両端は (0, 0) と (1, 1) に固定し、間の点だけを持つ。
#[derive(Clone, Debug, PartialEq)]
pub struct PressureCurve {
    name: &'static str,
    /// 入力の昇順に並んだ (入力, 出力)。両端は含まない。
    points: Vec<(f32, f32)>,
}

impl PressureCurve {
    /// `points` は (入力, 出力) の組。範囲外の値は 0..=1 に収め、入力の昇順に並べ替える。
    pub fn new(name: &'static str, points: &[(f32, f32)]) -> Self {
        let mut sorted: Vec<(f32, f32)> = points
            .iter()
            .map(|&(x, y)| (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)))
            .filter(|&(x, _)| x > 0.0 && x < 1.0)
            .collect();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        PressureCurve { name, points: sorted }
    }

    /// 入力と出力が等しい、補正なしのカーブ。
    pub fn linear() -> Self {
        PressureCurve::new("標準", &[])
    }

    /// 軽い筆圧でも太く・濃く出る。
    pub fn soft() -> Self {
        PressureCurve::new("柔らかめ", &[(0.25, 0.45), (0.6, 0.85)])
    }

    /// 強く押したときだけ太く・濃く出る。
    pub fn hard() -> Self {
        PressureCurve::new("硬め", &[(0.4, 0.15), (0.75, 0.5)])
    }

    pub fn presets() -> Vec<PressureCurve> {
        vec![PressureCurve::soft(), PressureCurve::linear(), PressureCurve::hard()]
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn apply(&self, input: f32) -> f32 {
        let x = input.clamp(0.0, 1.0);
        let mut prev = (0.0, 0.0);
        for &next in self.points.iter().chain(std::iter::once(&(1.0, 1.0))) {
            if x <= next.0 {
                let span = next.0 - prev.0;
                if span <= f32::EPSILON {
                    return next.1;
                }
                return prev.1 + (next.1 - prev.1) * (x - prev.0) / span;
            }
            prev = next;
        }
        1.0
    }
}

impl Default for PressureCurve {
    fn default() -> Self {
        PressureCurve::linear()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_is_identity() {
        let curve = PressureCurve::linear();
        for i in 0..=10 {
            let p = i as f32 / 10.0;
            assert!((curve.apply(p) - p).abs() < 1e-6);
        }
    }

    #[test]
    fn endpoints_are_fixed_and_input_is_clamped() {
        for curve in PressureCurve::presets() {
            assert_eq!(curve.apply(0.0), 0.0, "{}", curve.name());
            assert_eq!(curve.apply(1.0), 1.0, "{}", curve.name());
            assert_eq!(curve.apply(-1.0), 0.0);
            assert_eq!(curve.apply(2.0), 1.0);
        }
    }

    #[test]
    fn interpolates_between_points() {
        let curve = PressureCurve::new("t", &[(0.5, 0.8)]);
        assert!((curve.apply(0.25) - 0.4).abs() < 1e-6);
        assert!((curve.apply(0.75) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn soft_is_above_and_hard_is_below_linear() {
        let soft = PressureCurve::soft();
        let hard = PressureCurve::hard();
        for i in 1..10 {
            let p = i as f32 / 10.0;
            assert!(soft.apply(p) > p, "soft at {p}");
            assert!(hard.apply(p) < p, "hard at {p}");
        }
    }

    #[test]
    fn unsorted_and_out_of_range_points_are_normalized() {
        let curve = PressureCurve::new("t", &[(0.8, 2.0), (0.2, 0.1), (1.5, 0.0)]);
        assert_eq!(curve.points, vec![(0.2, 0.1), (0.8, 1.0)]);
    }
}
