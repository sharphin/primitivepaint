use std::cell::RefCell;

use pp_core::Rect;

use super::pixmap::Align;
use super::{Frame, Pixmap, theme};

/// 数字を入れる欄へのキー操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKey {
    Char(char),
    Backspace,
    Delete,
    /// 左右の矢印・Home・End。`select` なら Shift を押していて、選択を広げる。
    Left {
        select: bool,
    },
    Right {
        select: bool,
    },
    Home {
        select: bool,
    },
    End {
        select: bool,
    },
    SelectAll,
}

/// 正の数を入れる欄。キーで書き換えるほか、マウスで選択したり、ホイールで値を上下したりできる。
/// 小数を入れる欄（`with_decimals`）では、値は小数点以下の桁の分だけ 10 倍した整数で扱う（25.70 cm なら 2570）。
pub struct NumberField {
    text: String,
    /// 文字を入れる位置（文字の境目の番号）。
    caret: usize,
    /// 選択のもう一方の端。`caret` と同じなら選択していない。
    anchor: usize,
    min: u32,
    max: u32,
    /// 整数部分の桁数の上限。
    max_digits: usize,
    /// 小数点以下の桁数。0 なら整数だけ。
    decimals: u32,
    /// マウスで選択している途中か。
    selecting: bool,
    /// 最後に描いたときの、文字の境目の x（画面のピクセル）。押した位置から文字の境目を探すのに使う。
    edges: RefCell<Vec<i32>>,
}

impl NumberField {
    pub fn new(value: u32, min: u32, max: u32) -> Self {
        NumberField::with_decimals(value, min, max, 0)
    }

    /// 小数点以下 `decimals` 桁まで入る欄。`value`・`min`・`max` は 10^`decimals` 倍した整数。
    pub fn with_decimals(value: u32, min: u32, max: u32, decimals: u32) -> Self {
        let mut field = NumberField {
            text: String::new(),
            caret: 0,
            anchor: 0,
            min,
            max,
            max_digits: (max / 10u32.pow(decimals)).to_string().len(),
            decimals,
            selecting: false,
            edges: RefCell::new(Vec::new()),
        };
        field.set_value(value);
        field
    }

    /// 範囲内の数字が入っていれば、その値（小数の欄では 10^桁数 倍した整数）。
    pub fn value(&self) -> Option<u32> {
        self.parse(&self.text).filter(|v| (self.min..=self.max).contains(v))
    }

    /// 値を入れ替える。入れ替えた値はすべて選んだ状態にする（続けて打つと置き換わる）。
    pub fn set_value(&mut self, value: u32) {
        self.text = self.format(value);
        self.select_all();
    }

    fn unit(&self) -> u32 {
        10u32.pow(self.decimals)
    }

    /// 値を欄に出す文字にする。小数の欄では、小数点以下をいつも桁数どおりに出す（25.70）。
    fn format(&self, value: u32) -> String {
        if self.decimals == 0 {
            return value.to_string();
        }
        let unit = self.unit();
        format!("{}.{:0width$}", value / unit, value % unit, width = self.decimals as usize)
    }

    fn parse(&self, text: &str) -> Option<u32> {
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if whole.is_empty() || fraction.len() > self.decimals as usize {
            return None;
        }
        let padded = format!("{fraction:0<width$}", width = self.decimals as usize);
        let scaled = whole.parse::<u64>().ok()? * self.unit() as u64 + padded.parse::<u64>().unwrap_or(0);
        u32::try_from(scaled).ok()
    }

    /// 打っている途中の文字として入れてよいか。数字と（小数の欄では）小数点 1 つだけで、
    /// 整数部分は先頭に 0 を付けない（小数の欄の「0.5」の 0 は付けてよい）。桁数も上限まで。
    fn acceptable(&self, text: &str) -> bool {
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        let digits_only = |s: &str| s.chars().all(|c| c.is_ascii_digit());
        let leading_zero =
            if self.decimals == 0 { whole.starts_with('0') } else { whole.len() > 1 && whole.starts_with('0') };
        (self.decimals > 0 || !text.contains('.'))
            && digits_only(whole)
            && digits_only(fraction)
            && !leading_zero
            && whole.len() <= self.max_digits
            && fraction.len() <= self.decimals as usize
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    /// 選んでいる範囲を消す。消したら true。
    fn delete_selection(&mut self) -> bool {
        let (start, end) = self.selection();
        if start == end {
            return false;
        }
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = start;
        true
    }

    fn move_caret(&mut self, to: usize, select: bool) {
        self.caret = to.min(self.text.len());
        if !select {
            self.anchor = self.caret;
        }
    }

    /// キー操作。表示が変わったら true。
    pub fn key(&mut self, key: FieldKey) -> bool {
        let before = (self.text.clone(), self.caret, self.anchor);
        match key {
            FieldKey::Char(c) if c.is_ascii_digit() || c == '.' => {
                let (start, end) = self.selection();
                let mut next = self.text.clone();
                next.replace_range(start..end, &c.to_string());
                if !self.acceptable(&next) {
                    return false;
                }
                self.text = next;
                self.move_caret(start + 1, false);
            }
            FieldKey::Char(_) => return false,
            FieldKey::Backspace => {
                if !self.delete_selection() && self.caret > 0 {
                    self.text.remove(self.caret - 1);
                    self.move_caret(self.caret - 1, false);
                }
            }
            FieldKey::Delete => {
                if !self.delete_selection() && self.caret < self.text.len() {
                    self.text.remove(self.caret);
                }
            }
            FieldKey::Left { select } => {
                let (start, end) = self.selection();
                // 選択があるときは、選択の左端へ（Windows と同じ）
                let to = if !select && start != end { start } else { self.caret.saturating_sub(1) };
                self.move_caret(to, select);
            }
            FieldKey::Right { select } => {
                let (start, end) = self.selection();
                let to = if !select && start != end { end } else { self.caret + 1 };
                self.move_caret(to, select);
            }
            FieldKey::Home { select } => self.move_caret(0, select),
            FieldKey::End { select } => self.move_caret(self.text.len(), select),
            FieldKey::SelectAll => self.select_all(),
        }
        before != (self.text.clone(), self.caret, self.anchor)
    }

    /// ホイールで値を `steps` 段（上が正）、1 段 `step` ずつ上下する。空や範囲外のときは範囲に収める。
    pub fn wheel(&mut self, steps: i32, step: u32) -> bool {
        let current = self.parse(&self.text).map_or(self.min as i64, |v| v as i64);
        let next = (current + steps as i64 * step as i64).clamp(self.min as i64, self.max as i64) as u32;
        let text = self.format(next);
        if self.text == text {
            return false;
        }
        self.text = text;
        self.move_caret(self.text.len(), false);
        true
    }

    /// 画面の x に一番近い文字の境目。
    fn edge_at(&self, x: f32) -> usize {
        let edges = self.edges.borrow();
        (0..edges.len())
            .min_by(|&a, &b| (edges[a] as f32 - x).abs().total_cmp(&(edges[b] as f32 - x).abs()))
            .unwrap_or(self.text.len())
            .min(self.text.len())
    }

    /// 欄を押した。`select` なら Shift を押していて、今の位置から選択を広げる。
    pub fn press(&mut self, x: f32, select: bool) {
        let to = self.edge_at(x);
        self.move_caret(to, select);
        self.selecting = true;
    }

    /// 押したまま動かすと、押した位置からそこまでを選ぶ。表示が変わったら true。
    pub fn drag(&mut self, x: f32) -> bool {
        if !self.selecting {
            return false;
        }
        let before = self.caret;
        self.move_caret(self.edge_at(x), true);
        before != self.caret
    }

    pub fn release(&mut self) {
        self.selecting = false;
    }

    pub fn is_selecting(&self) -> bool {
        self.selecting
    }

    pub fn render(&self, pixmap: &mut Pixmap, frame: &Frame, rect: Rect, focused: bool) {
        pixmap.fill_rect(rect, theme::colors().button);
        pixmap.frame(rect, if focused { theme::colors().selected_border } else { theme::colors().border });
        let font = frame.font_px(theme::BODY_TEXT);
        let inner = Rect::new(rect.x0 + frame.px(6.0), rect.y0, rect.x1 - frame.px(6.0), rect.y1);
        let edges: Vec<i32> = (0..=self.text.len())
            .map(|i| inner.x0 + pixmap.measure_text(&self.text[..i], font).unwrap_or(0.0).round() as i32)
            .collect();
        let (start, end) = self.selection();
        let (top, bottom) = (rect.y0 + frame.px(5.0), rect.y1 - frame.px(5.0));
        if focused && start != end {
            pixmap.fill_rect(Rect::new(edges[start], top, edges[end] + 1, bottom), theme::colors().selected);
        }
        pixmap.text(inner, &self.text, font, theme::colors().text, Align::Left);
        if focused && start == end {
            // 文字を入れる位置
            let x = edges[self.caret] + 1;
            pixmap.fill_rect(Rect::new(x, top, x + 1, bottom), theme::colors().text);
        }
        *self.edges.borrow_mut() = edges;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field() -> NumberField {
        NumberField::new(3000, 16, 10000)
    }

    fn typed(field: &mut NumberField, text: &str) {
        for c in text.chars() {
            field.key(FieldKey::Char(c));
        }
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut f = field();
        typed(&mut f, "1234");
        assert_eq!(f.value(), Some(1234));
        f.key(FieldKey::SelectAll);
        typed(&mut f, "5");
        assert_eq!(f.text, "5");
        assert_eq!(f.value(), None, "16 より小さい");
    }

    #[test]
    fn shift_arrows_select_and_delete_removes_them() {
        let mut f = field();
        f.key(FieldKey::End { select: false });
        f.key(FieldKey::Left { select: true });
        f.key(FieldKey::Left { select: true });
        f.key(FieldKey::Delete);
        assert_eq!(f.text, "30");
        f.key(FieldKey::Home { select: false });
        f.key(FieldKey::Delete);
        assert_eq!(f.text, "0");
        assert!(!f.key(FieldKey::Char('0')) || !f.text.starts_with('0'));
        f.key(FieldKey::Home { select: false });
        typed(&mut f, "5");
        assert_eq!(f.text, "50");
        f.key(FieldKey::Backspace);
        assert_eq!(f.text, "0", "Backspace は左の 1 文字");
    }

    #[test]
    fn leading_zero_and_too_many_digits_are_refused() {
        let mut f = field();
        assert!(!f.key(FieldKey::Char('0')), "選択を 0 で置き換えると先頭が 0 になる");
        f.key(FieldKey::End { select: false });
        typed(&mut f, "0");
        assert_eq!(f.text, "30000", "5 桁までは入る");
        assert!(!f.key(FieldKey::Char('1')));
    }

    #[test]
    fn wheel_steps_the_value_within_range() {
        let mut f = field();
        assert!(f.wheel(2, 10));
        assert_eq!(f.value(), Some(3020));
        f.wheel(-1000, 10);
        assert_eq!(f.value(), Some(16));
        f.key(FieldKey::SelectAll);
        f.key(FieldKey::Delete);
        f.wheel(1, 1);
        assert_eq!(f.value(), Some(17), "空なら最小から数える");
        f.wheel(10_000, 100);
        assert_eq!(f.value(), Some(10000));
        assert!(!f.wheel(1, 1), "上限より上には行かない");
    }

    #[test]
    fn decimal_field_reads_and_shows_fixed_digits() {
        let mut f = NumberField::with_decimals(2570, 1, 100_000, 2);
        assert_eq!(f.text, "25.70");
        assert_eq!(f.value(), Some(2570));
        typed(&mut f, "0.5");
        assert_eq!(f.value(), Some(50), "0.5 は 50");
        assert!(!f.key(FieldKey::Char('.')), "小数点は 1 つだけ");
        typed(&mut f, "5");
        assert!(!f.key(FieldKey::Char('1')), "小数点以下は 2 桁まで");
        assert_eq!(f.value(), Some(55));
        f.key(FieldKey::SelectAll);
        typed(&mut f, "12");
        assert_eq!(f.value(), Some(1200), "小数点がなければ整数");
        f.key(FieldKey::SelectAll);
        assert!(!f.key(FieldKey::Char('0')) || f.text == "0");
        assert!(!f.key(FieldKey::Char('0')), "00 は入らない");
        assert!(f.wheel(1, 10));
        assert_eq!(f.text, "0.10");
    }

    #[test]
    fn integer_field_refuses_the_decimal_point() {
        let mut f = field();
        f.key(FieldKey::End { select: false });
        assert!(!f.key(FieldKey::Char('.')));
    }

    #[test]
    fn dragging_selects_between_the_pressed_edges() {
        let mut f = field();
        // 1 文字 10px で描いたことにする
        *f.edges.borrow_mut() = vec![100, 110, 120, 130, 140];
        f.press(111.0, false);
        assert!(f.drag(138.0));
        f.release();
        assert!(!f.drag(100.0), "離したら選択は動かない");
        assert_eq!(f.selection(), (1, 4));
        f.key(FieldKey::Backspace);
        assert_eq!(f.text, "3");
    }
}
