/// ドキュメントを作るときに選ぶキャンバスの種類。モードごとに操作の制約が変わる。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CanvasMode {
    /// 紙。自由に回転でき、重ねた紙（レイヤー）を使える。
    #[default]
    Paper,
    /// 油彩キャンバス。90 度単位でしか回転できず、レイヤーはない。
    OilCanvas,
}

impl CanvasMode {
    pub fn name(&self) -> &str {
        match self {
            CanvasMode::Paper => "紙",
            CanvasMode::OilCanvas => "油彩キャンバス",
        }
    }

    /// 回転の刻み（ラジアン）。`None` なら自由に回転できる。
    pub fn rotation_step(&self) -> Option<f32> {
        match self {
            CanvasMode::Paper => None,
            CanvasMode::OilCanvas => Some(std::f32::consts::FRAC_PI_2),
        }
    }

    /// 角度をこのモードで許される値に丸める。
    pub fn snap_rotation(&self, angle: f32) -> f32 {
        match self.rotation_step() {
            Some(step) => (angle / step).round() * step,
            None => angle,
        }
    }

    pub fn has_layers(&self) -> bool {
        matches!(self, CanvasMode::Paper)
    }

    /// 解像度を決めずに作った作品の解像度（dpi）。新規作成の用紙の大きさと同じく、紙は 350、
    /// 油彩キャンバスは混色の計算を軽くするため粗めの 200。
    pub fn default_dpi(&self) -> u32 {
        match self {
            CanvasMode::Paper => 350,
            CanvasMode::OilCanvas => 200,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn oil_canvas_snaps_to_right_angles() {
        let mode = CanvasMode::OilCanvas;
        assert_eq!(mode.snap_rotation(0.3), 0.0);
        assert_eq!(mode.snap_rotation(1.2), FRAC_PI_2);
        assert_eq!(mode.snap_rotation(-1.0), -FRAC_PI_2);
    }

    #[test]
    fn paper_rotates_freely() {
        assert_eq!(CanvasMode::Paper.snap_rotation(0.3), 0.3);
    }
}
