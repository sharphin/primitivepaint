use crate::Color;

/// 色見本の表（RGB 表）。横に色相、縦に明るさを並べ、右端の 1 列は白から黒までの無彩色。
///
/// 画材店の色見本から絵の具を選ぶのと同じ扱いで、ここで選んだ色はペン先には付かず、
/// 新しいチューブになる。パレットに出してから混ぜて使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorChart {
    hues: u32,
    rows: u32,
}

impl Default for ColorChart {
    fn default() -> Self {
        ColorChart { hues: 24, rows: 9 }
    }
}

impl ColorChart {
    /// 列の数（色相の列と無彩色の 1 列）。
    pub fn columns(&self) -> u32 {
        self.hues + 1
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// 表の中の位置（0..1）にあるマス。表の外なら `None`。
    pub fn cell_at(&self, u: f32, v: f32) -> Option<(u32, u32)> {
        let inside = (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v);
        inside.then(|| ((u * self.columns() as f32) as u32, (v * self.rows as f32) as u32))
    }

    /// マス (column, row) の色。上の行ほど白く、下の行ほど黒い。真ん中の行が純色。
    pub fn color_of(&self, column: u32, row: u32) -> Color {
        let level = row.min(self.rows - 1) as f32;
        let last = (self.rows - 1) as f32;
        if column >= self.hues {
            let value = 1.0 - level / last;
            return Color { r: value, g: value, b: value, a: 1.0 };
        }
        let pure = hue_to_rgb(column as f32 / self.hues as f32);
        let middle = last * 0.5;
        // 端の行でも真っ白・真っ黒にはせず、色味を少し残す
        let (target, amount) = if level < middle {
            (1.0, (middle - level) / (middle + 1.0))
        } else {
            (0.0, (level - middle) / (middle + 1.0))
        };
        let toward = |channel: f32| channel + (target - channel) * amount;
        Color { r: toward(pure[0]), g: toward(pure[1]), b: toward(pure[2]), a: 1.0 }
    }
}

/// 色相（0..1、0 が赤）の純色。
fn hue_to_rgb(hue: f32) -> [f32; 3] {
    let channel = |offset: f32| {
        let k = (hue * 6.0 + offset) % 6.0;
        1.0 - (k.min(4.0 - k).clamp(0.0, 1.0))
    };
    [channel(5.0), channel(3.0), channel(1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Color, r: f32, g: f32, b: f32) -> bool {
        (a.r - r).abs() < 0.01 && (a.g - g).abs() < 0.01 && (a.b - b).abs() < 0.01
    }

    #[test]
    fn middle_row_holds_pure_hues() {
        let chart = ColorChart::default();
        let middle = chart.rows() / 2;
        assert!(close(chart.color_of(0, middle), 1.0, 0.0, 0.0));
        assert!(close(chart.color_of(8, middle), 0.0, 1.0, 0.0));
        assert!(close(chart.color_of(16, middle), 0.0, 0.0, 1.0));
        assert!(close(chart.color_of(4, middle), 1.0, 1.0, 0.0));
    }

    #[test]
    fn rows_go_from_light_to_dark() {
        let chart = ColorChart::default();
        let top = chart.color_of(0, 0);
        let bottom = chart.color_of(0, chart.rows() - 1);
        assert!(top.g > 0.7 && top.r > 0.99, "{top:?}");
        assert!(bottom.r < 0.25 && bottom.r > 0.0, "{bottom:?}");
    }

    #[test]
    fn last_column_is_gray_from_white_to_black() {
        let chart = ColorChart::default();
        let gray = chart.columns() - 1;
        assert!(close(chart.color_of(gray, 0), 1.0, 1.0, 1.0));
        assert!(close(chart.color_of(gray, chart.rows() - 1), 0.0, 0.0, 0.0));
    }

    #[test]
    fn cell_at_maps_positions_and_rejects_outside() {
        let chart = ColorChart::default();
        assert_eq!(chart.cell_at(0.0, 0.0), Some((0, 0)));
        assert_eq!(chart.cell_at(0.999, 0.999), Some((chart.columns() - 1, chart.rows() - 1)));
        assert_eq!(chart.cell_at(1.0, 0.5), None);
        assert_eq!(chart.cell_at(0.5, -0.1), None);
    }
}
