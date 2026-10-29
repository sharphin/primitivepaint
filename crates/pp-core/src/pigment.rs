//! 絵の具の混色。光の足し算（RGB の平均）ではなく、Kubelka-Munk 理論で
//! 「顔料が光を吸収・散乱する量」を混ぜるので、青と黄を混ぜると緑になる。

use crate::Color;

/// 混色計算用の絵の具の色。リニア RGB の反射率（0..=1）で持つ。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pigment {
    reflectance: [f32; 3],
}

/// 反射率 0 や 1 で K/S が発散しないように丸める範囲。
const MIN_REFLECTANCE: f32 = 0.001;
const MAX_REFLECTANCE: f32 = 0.999;

impl Pigment {
    /// sRGB の色から作る。
    pub fn from_color(color: Color) -> Self {
        Pigment { reflectance: [srgb_to_linear(color.r), srgb_to_linear(color.g), srgb_to_linear(color.b)] }
    }

    /// リニア RGB の反射率から作る（ファイルから読み込むときに使う）。
    pub fn from_reflectance(reflectance: [f32; 3]) -> Self {
        Pigment { reflectance: reflectance.map(|r| if r.is_finite() { r.clamp(0.0, 1.0) } else { 0.0 }) }
    }

    pub fn reflectance(&self) -> [f32; 3] {
        self.reflectance
    }

    pub fn to_color(self) -> Color {
        let [r, g, b] = self.reflectance;
        Color { r: linear_to_srgb(r), g: linear_to_srgb(g), b: linear_to_srgb(b), a: 1.0 }
    }

    /// `parts` の (絵の具, 量) を混ぜる。量の合計が 0 なら `None`。
    pub fn mix(parts: &[(Pigment, f32)]) -> Option<Pigment> {
        let total: f32 = parts.iter().map(|(_, amount)| amount.max(0.0)).sum();
        if total <= 0.0 {
            return None;
        }
        let mut ks = [0.0f32; 3];
        for (pigment, amount) in parts {
            let weight = amount.max(0.0) / total;
            for (channel, k) in ks.iter_mut().enumerate() {
                *k += weight * absorption(pigment.reflectance[channel]);
            }
        }
        Some(Pigment { reflectance: ks.map(reflectance_of) })
    }
}

/// 反射率から K/S（吸収 / 散乱）を求める。
fn absorption(reflectance: f32) -> f32 {
    let r = reflectance.clamp(MIN_REFLECTANCE, MAX_REFLECTANCE);
    (1.0 - r) * (1.0 - r) / (2.0 * r)
}

/// K/S から反射率に戻す。
fn reflectance_of(ks: f32) -> f32 {
    1.0 + ks - (ks * ks + 2.0 * ks).sqrt()
}

fn srgb_to_linear(v: f32) -> f32 {
    let c = v.clamp(0.0, 1.0);
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(v: f32) -> f32 {
    let c = v.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pigment(r: f32, g: f32, b: f32) -> Pigment {
        Pigment::from_color(Color { r, g, b, a: 1.0 })
    }

    #[test]
    fn blue_and_yellow_make_green() {
        let blue = pigment(0.1, 0.2, 0.6);
        let yellow = pigment(0.98, 0.82, 0.05);
        let green = Pigment::mix(&[(blue, 1.0), (yellow, 1.0)]).unwrap().to_color();
        assert!(green.g > green.r && green.g > green.b, "{green:?}");
    }

    #[test]
    fn mixing_one_pigment_returns_itself() {
        let red = pigment(0.8, 0.1, 0.1);
        let mixed = Pigment::mix(&[(red, 3.0)]).unwrap().to_color();
        assert!((mixed.r - 0.8).abs() < 0.01 && (mixed.g - 0.1).abs() < 0.01);
    }

    #[test]
    fn white_lightens_and_amount_matters() {
        let red = pigment(0.8, 0.1, 0.1);
        let white = pigment(1.0, 1.0, 1.0);
        let little_white = Pigment::mix(&[(red, 3.0), (white, 1.0)]).unwrap().to_color();
        let much_white = Pigment::mix(&[(red, 1.0), (white, 3.0)]).unwrap().to_color();
        assert!(much_white.g > little_white.g && little_white.g > 0.1);
    }

    #[test]
    fn nothing_to_mix() {
        assert!(Pigment::mix(&[]).is_none());
        assert!(Pigment::mix(&[(pigment(0.5, 0.5, 0.5), 0.0)]).is_none());
    }
}
