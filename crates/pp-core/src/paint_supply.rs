use crate::pen::InputSample;

/// 絵の具の量が 0 のとき、1px 進むごとに減る割合（対数）。量が多いほど、2 乗で遅くなる。
/// 量 0 では約 200px、量 0.5 では約 800px でほとんどかすれて見えなくなる。
const DRY_RATE: f32 = 0.012;
/// 残りがこれより少なくなったら、もう色は付かない。
const EMPTY: f32 = 0.02;

/// 筆に含ませた絵の具の量と、1 ストロークの中での減り具合。油彩筆・水彩筆が部品として持つ。
///
/// 量が少ないと、描くうちに絵の具が減っていき、線の終わりがかすれる。1 なら描いても減らない。
#[derive(Clone, Debug)]
pub struct PaintSupply {
    /// ストロークの始めに含ませる量（0..=1）。
    amount: f32,
    /// このストロークで、筆に残っている割合（始めは 1）。
    remaining: f32,
    /// 前のダブの位置。進んだ距離だけ減らすのに使う。
    last_dab: Option<(f32, f32)>,
}

impl Default for PaintSupply {
    fn default() -> Self {
        PaintSupply { amount: 1.0, remaining: 1.0, last_dab: None }
    }
}

impl PaintSupply {
    pub fn amount(&self) -> f32 {
        self.amount
    }

    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount.clamp(0.0, 1.0);
    }

    /// ストロークの始めに、筆に絵の具を含ませ直す。
    pub fn refill(&mut self) {
        self.remaining = 1.0;
        self.last_dab = None;
    }

    /// 前のダブから `sample` まで進んだ分だけ減らし、このダブで付く濃さ（0..=1）を返す。
    pub fn use_paint(&mut self, sample: InputSample) -> f32 {
        if let Some((x, y)) = self.last_dab {
            let distance = ((sample.x - x).powi(2) + (sample.y - y).powi(2)).sqrt();
            let dryness = (1.0 - self.amount).powi(2);
            self.remaining *= (-DRY_RATE * dryness * distance).exp();
        }
        self.last_dab = Some((sample.x, sample.y));
        if self.remaining < EMPTY { 0.0 } else { self.remaining }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32) -> InputSample {
        InputSample { x, y: 0.0, pressure: 1.0 }
    }

    #[test]
    fn full_supply_never_runs_out() {
        let mut supply = PaintSupply::default();
        supply.refill();
        assert_eq!(supply.use_paint(at(0.0)), 1.0);
        assert_eq!(supply.use_paint(at(5000.0)), 1.0);
    }

    #[test]
    fn scarce_supply_runs_out_and_refills() {
        let mut supply = PaintSupply::default();
        supply.set_amount(0.0);
        supply.refill();
        supply.use_paint(at(0.0));
        let middle = supply.use_paint(at(100.0));
        assert!(middle > 0.0 && middle < 0.5, "{middle}");
        assert_eq!(supply.use_paint(at(1000.0)), 0.0);
        supply.refill();
        assert_eq!(supply.use_paint(at(1000.0)), 1.0);
    }
}
