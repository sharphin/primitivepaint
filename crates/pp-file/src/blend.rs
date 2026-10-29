//! ほかのソフトのレイヤーの合成モード。primitivepaint の紙は合成モードを持たないので、
//! 読み込むときにこの計算で下の紙に焼き込む。
//!
//! 式は W3C Compositing and Blending の分離型・非分離型の定義に合わせる。
//! CLIP STUDIO 独自の「発光」系などは、近いもの（覆い焼き・加算）で代用する。

use pp_core::Rgba8;

/// 合成モード。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    Subtract,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    Add,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    Divide,
}

impl Blend {
    /// Photoshop（.psd）の合成モードの 4 文字。`pass`（通過）はフォルダにしか使われず、ここでは扱わない。
    /// 知らないものなら `None`。
    pub fn from_psd_key(key: &[u8; 4]) -> Option<Blend> {
        Some(match key {
            b"norm" => Blend::Normal,
            // ディザ合成は、ばらつきを付けずに通常として扱う
            b"diss" => Blend::Normal,
            b"dark" => Blend::Darken,
            b"mul " => Blend::Multiply,
            b"idiv" => Blend::ColorBurn,
            b"lbrn" => Blend::LinearBurn,
            b"dkCl" => Blend::DarkerColor,
            b"lite" => Blend::Lighten,
            b"scrn" => Blend::Screen,
            b"div " => Blend::ColorDodge,
            b"lddg" => Blend::Add,
            b"lgCl" => Blend::LighterColor,
            b"over" => Blend::Overlay,
            b"sLit" => Blend::SoftLight,
            b"hLit" => Blend::HardLight,
            b"vLit" => Blend::VividLight,
            b"lLit" => Blend::LinearLight,
            b"pLit" => Blend::PinLight,
            b"hMix" => Blend::HardMix,
            b"diff" => Blend::Difference,
            b"smud" => Blend::Exclusion,
            b"fsub" => Blend::Subtract,
            b"fdiv" => Blend::Divide,
            b"hue " => Blend::Hue,
            b"sat " => Blend::Saturation,
            b"colr" => Blend::Color,
            b"lum " => Blend::Luminosity,
            _ => return None,
        })
    }

    /// 下の色 `b` と上の色 `s`（どちらも 0..=1、premultiplied ではない）を混ぜた色。
    fn mix(self, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
        let each = |f: fn(f32, f32) -> f32| [f(b[0], s[0]), f(b[1], s[1]), f(b[2], s[2])];
        match self {
            Blend::Normal => s,
            Blend::Darken => each(f32::min),
            Blend::Multiply => each(|b, s| b * s),
            Blend::ColorBurn => each(|b, s| {
                if b >= 1.0 {
                    1.0
                } else if s <= 0.0 {
                    0.0
                } else {
                    1.0 - ((1.0 - b) / s).min(1.0)
                }
            }),
            Blend::LinearBurn => each(|b, s| (b + s - 1.0).max(0.0)),
            Blend::Subtract => each(|b, s| (b - s).max(0.0)),
            Blend::DarkerColor => {
                if lum(s) < lum(b) {
                    s
                } else {
                    b
                }
            }
            Blend::Lighten => each(f32::max),
            Blend::Screen => each(screen),
            Blend::ColorDodge => each(dodge),
            Blend::Add => each(|b, s| (b + s).min(1.0)),
            Blend::LighterColor => {
                if lum(s) > lum(b) {
                    s
                } else {
                    b
                }
            }
            Blend::Overlay => each(|b, s| hard_light(s, b)),
            Blend::SoftLight => each(soft_light),
            Blend::HardLight => each(hard_light),
            Blend::VividLight => each(|b, s| if s <= 0.5 { burn(b, 2.0 * s) } else { dodge(b, 2.0 * s - 1.0) }),
            Blend::LinearLight => each(|b, s| (b + 2.0 * s - 1.0).clamp(0.0, 1.0)),
            Blend::PinLight => each(|b, s| if s <= 0.5 { b.min(2.0 * s) } else { b.max(2.0 * s - 1.0) }),
            Blend::HardMix => each(|b, s| if b + s >= 1.0 { 1.0 } else { 0.0 }),
            Blend::Difference => each(|b, s| (b - s).abs()),
            Blend::Exclusion => each(|b, s| b + s - 2.0 * b * s),
            Blend::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
            Blend::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
            Blend::Color => set_lum(s, lum(b)),
            Blend::Luminosity => set_lum(b, lum(s)),
            Blend::Divide => each(|b, s| if s <= 0.0 { if b > 0.0 { 1.0 } else { 0.0 } } else { (b / s).min(1.0) }),
        }
    }

    /// 下に何もない所に重ねたとき、通常と同じに見えるか（乗算などは透明の上では通常と同じ）。
    pub fn looks_normal_on_nothing(self) -> bool {
        !matches!(self, Blend::Subtract | Blend::Difference | Blend::Exclusion | Blend::Divide | Blend::HardMix)
    }

    /// 下 `below` の上に `src` をこのモードで重ねる（どちらも premultiplied）。`opacity` は 0..=1。
    /// `clip` なら下の不透明な所にだけ描く（下のレイヤーでクリッピング）。
    pub fn composite(self, below: Rgba8, src: Rgba8, opacity: f32, clip: bool) -> Rgba8 {
        let a_s = src.a as f32 / 255.0 * opacity;
        let a_b = below.a as f32 / 255.0;
        if a_s <= 0.0 || (clip && a_b <= 0.0) {
            return below;
        }
        let straight = |p: Rgba8| {
            let a = p.a as f32;
            if a <= 0.0 { [0.0; 3] } else { [p.r as f32 / a, p.g as f32 / a, p.b as f32 / a] }
        };
        let (c_s, c_b) = (straight(src), straight(below));
        let mixed = self.mix(c_b, c_s);
        let mut out = [0.0f32; 3];
        for i in 0..3 {
            let both = a_s * a_b * mixed[i];
            let only_below = (1.0 - a_s) * a_b * c_b[i];
            let only_src = if clip { 0.0 } else { (1.0 - a_b) * a_s * c_s[i] };
            out[i] = both + only_below + only_src;
        }
        let alpha = if clip { a_b } else { a_s + a_b - a_s * a_b };
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let a = to_u8(alpha);
        Rgba8 { r: to_u8(out[0]).min(a), g: to_u8(out[1]).min(a), b: to_u8(out[2]).min(a), a }
    }
}

fn screen(b: f32, s: f32) -> f32 {
    b + s - b * s
}

fn dodge(b: f32, s: f32) -> f32 {
    if b <= 0.0 {
        0.0
    } else if s >= 1.0 {
        1.0
    } else {
        (b / (1.0 - s)).min(1.0)
    }
}

fn burn(b: f32, s: f32) -> f32 {
    if b >= 1.0 {
        1.0
    } else if s <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - b) / s).min(1.0)
    }
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 { b * 2.0 * s } else { screen(b, 2.0 * s - 1.0) }
}

fn soft_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b - (1.0 - 2.0 * s) * b * (1.0 - b)
    } else {
        let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
        b + (2.0 * s - 1.0) * (d - b)
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    c.map(|v| {
        let low = if n < 0.0 { l + (v - l) * l / (l - n) } else { v };
        if x > 1.0 { l + (low - l) * (1.0 - l) / (x - l) } else { low }
    })
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max <= min {
        return [0.0; 3];
    }
    c.map(|v| (v - min) * s / (max - min))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgba8 = Rgba8 { r: 255, g: 255, b: 255, a: 255 };

    #[test]
    fn multiply_on_white_keeps_the_color_and_on_nothing_shows_it() {
        let red = Rgba8 { r: 200, g: 0, b: 0, a: 255 };
        assert_eq!(Blend::Multiply.composite(WHITE, red, 1.0, false), red);
        assert_eq!(Blend::Multiply.composite(Rgba8::default(), red, 1.0, false), red);
        let gray = Rgba8 { r: 128, g: 128, b: 128, a: 255 };
        let dark = Blend::Multiply.composite(gray, red, 1.0, false);
        assert!((99..=101).contains(&dark.r) && dark.g == 0, "{dark:?}");
    }

    #[test]
    fn clip_keeps_the_alpha_below() {
        let half = Rgba8 { r: 0, g: 0, b: 128, a: 128 };
        let red = Rgba8 { r: 255, g: 0, b: 0, a: 255 };
        let clipped = Blend::Normal.composite(half, red, 1.0, true);
        assert_eq!(clipped.a, 128);
        assert_eq!((clipped.r, clipped.b), (128, 0));
        assert_eq!(Blend::Normal.composite(Rgba8::default(), red, 1.0, true), Rgba8::default());
    }

    #[test]
    fn opacity_scales_the_upper_layer() {
        let black = Rgba8 { r: 0, g: 0, b: 0, a: 255 };
        let mixed = Blend::Normal.composite(WHITE, black, 0.5, false);
        assert!((127..=128).contains(&mixed.r) && mixed.a == 255, "{mixed:?}");
    }

    #[test]
    fn screen_lightens() {
        let gray = Rgba8 { r: 128, g: 128, b: 128, a: 255 };
        let lighter = Blend::Screen.composite(gray, gray, 1.0, false);
        assert!(lighter.r > 180, "{lighter:?}");
    }
}
