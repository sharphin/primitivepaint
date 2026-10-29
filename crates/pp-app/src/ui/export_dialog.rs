use pp_core::{Document, Rect, Rgba8};
use pp_file::ImageFormat;

use super::dialog::{self, Dialog, DialogKey, DialogResult, WheelStep};
use super::number_field::NumberField;
use super::pixmap::Align;
use super::widgets::{self, Button};
use super::{Action, Frame, Pixmap, theme};

/// すぐ選べる倍率（%）。175% は、200dpi の油彩キャンバスを 350dpi にする倍率。
const SCALES: [u32; 5] = [50, 100, 150, 175, 200];
/// すぐ選べる解像度（dpi）。
const RESOLUTIONS: [u32; 5] = [72, 150, 300, 350, 600];
/// すぐ選べる品質（JPEG など）。
const QUALITIES: [u32; 4] = [60, 80, 90, 100];

/// 入れられる倍率の範囲（%）。
pub const MIN_SCALE: u32 = 10;
pub const MAX_SCALE: u32 = 1000;
/// 書き出せる画像の一辺の上限（ピクセル）。PNG は一度にメモリに置いて圧縮するので、大きすぎないようにする。
pub const MAX_OUTPUT_SIDE: u32 = 20000;
/// 幅・高さの欄に入れられる上限（単位の最小の桁で数えた値）。上限を超える大きさは、入れた上で知らせる。
const MAX_SIZE_VALUE: u32 = 999_999;

const QUALITY_WHEEL_STEP_COARSE: u32 = 10;

const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 206.0;
/// 品質の欄がある形式で増える高さ。
const QUALITY_HEIGHT: f32 = 56.0;
/// 大きさの決め方を選ぶボタンの幅。
const MODE_WIDTH: f32 = 110.0;
const FIELD_WIDTH: f32 = 70.0;
const PAD: f32 = 14.0;
const ROW_GAP: f32 = 8.0;
const BUTTON_HEIGHT: f32 = 26.0;
const LABEL_HEIGHT: f32 = 20.0;
/// 欄の左端（大きさの決め方のボタンの右）。
const FIELDS_LEFT: f32 = PAD + MODE_WIDTH + 10.0;
/// 欄の右に並べる、すぐ選べる値のボタンの左端。
const PRESETS_LEFT: f32 = FIELDS_LEFT + FIELD_WIDTH + 32.0;

/// 書き出す大きさの決め方（CLIP STUDIO の「出力サイズ」と同じ 3 通り）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeBy {
    /// 作品に対する倍率。解像度は作品のまま。
    #[default]
    Scale,
    /// 幅と高さ（縦横比は作品のまま）。cm などの単位は作品の解像度で画素に換算する。
    Size,
    /// 紙の上の大きさは作品のまま、解像度を変える。
    Resolution,
}

impl SizeBy {
    const ALL: [SizeBy; 3] = [SizeBy::Scale, SizeBy::Size, SizeBy::Resolution];

    fn label(&self) -> &'static str {
        match self {
            SizeBy::Scale => "拡縮率",
            SizeBy::Size => "出力サイズ",
            SizeBy::Resolution => "解像度",
        }
    }

    /// ダイアログの何行目に置くか。
    fn row(&self) -> usize {
        match self {
            SizeBy::Scale => 0,
            SizeBy::Size => 1,
            SizeBy::Resolution => 2,
        }
    }
}

/// 幅・高さの単位。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeUnit {
    #[default]
    Pixel,
    Millimeter,
    Centimeter,
    Inch,
}

impl SizeUnit {
    const ALL: [SizeUnit; 4] = [SizeUnit::Pixel, SizeUnit::Millimeter, SizeUnit::Centimeter, SizeUnit::Inch];

    fn label(&self) -> &'static str {
        match self {
            SizeUnit::Pixel => "px",
            SizeUnit::Millimeter => "mm",
            SizeUnit::Centimeter => "cm",
            SizeUnit::Inch => "in",
        }
    }

    /// 欄に入れられる小数点以下の桁数。
    fn decimals(&self) -> u32 {
        match self {
            SizeUnit::Pixel => 0,
            SizeUnit::Millimeter => 1,
            SizeUnit::Centimeter | SizeUnit::Inch => 2,
        }
    }

    /// 1 インチがこの単位でいくつか。画素は解像度で決まるので `None`。
    fn per_inch(&self) -> Option<f64> {
        match self {
            SizeUnit::Pixel => None,
            SizeUnit::Millimeter => Some(25.4),
            SizeUnit::Centimeter => Some(2.54),
            SizeUnit::Inch => Some(1.0),
        }
    }

    /// 欄の値（小数点以下の桁の分 10 倍した整数）を、解像度 `dpi` の画素数にする。
    fn pixels_of(&self, value: u32, dpi: u32) -> f64 {
        let amount = value as f64 / 10f64.powi(self.decimals() as i32);
        match self.per_inch() {
            Some(per_inch) => amount / per_inch * dpi as f64,
            None => amount,
        }
    }

    /// 解像度 `dpi` の画素数を、欄の値にする。
    fn value_of(&self, pixels: f64, dpi: u32) -> u32 {
        let amount = match self.per_inch() {
            Some(per_inch) => pixels / dpi as f64 * per_inch,
            None => pixels,
        };
        (amount * 10f64.powi(self.decimals() as i32)).round().clamp(1.0, MAX_SIZE_VALUE as f64) as u32
    }

    fn field(&self, value: u32) -> NumberField {
        NumberField::with_decimals(value, 1, MAX_SIZE_VALUE, self.decimals())
    }
}

/// 書き出しの設定のうち、次に開いたときに引き継ぐもの。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportChoices {
    pub size_by: SizeBy,
    pub unit: SizeUnit,
    /// 倍率（%）。
    pub scale: u32,
    pub quality: u8,
}

impl Default for ExportChoices {
    fn default() -> Self {
        ExportChoices {
            size_by: SizeBy::Scale,
            unit: SizeUnit::Pixel,
            scale: 100,
            quality: pp_file::jpeg::DEFAULT_QUALITY,
        }
    }
}

/// 書き出す作品の大きさと解像度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportSource {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}

/// 書き出す画像の大きさ（ピクセル）と解像度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Output {
    width: u32,
    height: u32,
    dpi: u32,
}

impl Output {
    fn fits(&self) -> bool {
        self.width.max(self.height) <= MAX_OUTPUT_SIDE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Scale,
    Width,
    Height,
    Resolution,
    Quality,
}

impl Field {
    const ALL: [Field; 5] = [Field::Scale, Field::Width, Field::Height, Field::Resolution, Field::Quality];

    /// この欄を使う大きさの決め方。品質の欄はどれにも属さない。
    fn size_by(&self) -> Option<SizeBy> {
        match self {
            Field::Scale => Some(SizeBy::Scale),
            Field::Width | Field::Height => Some(SizeBy::Size),
            Field::Resolution => Some(SizeBy::Resolution),
            Field::Quality => None,
        }
    }
}

/// 「画像を統合して書き出し」のダイアログ。CLIP STUDIO と同じく、書き出す大きさを倍率・幅と高さ・解像度の
/// どれかで決める。JPEG などでは品質も決める。
pub struct ExportDialog {
    frame: Frame,
    format: ImageFormat,
    source: ExportSource,
    size_by: SizeBy,
    unit: SizeUnit,
    scale: NumberField,
    width: NumberField,
    height: NumberField,
    resolution: NumberField,
    /// 品質（1〜100）。品質を選ばない形式では `None`。
    quality: Option<NumberField>,
    /// 品質を選ばない形式で、次に引き継ぐ品質。
    kept_quality: u8,
    focus: Field,
}

impl ExportDialog {
    /// `choices` は前に書き出したときの設定。幅・高さと解像度の欄は、作品と同じ大きさから始める。
    pub fn new(format: ImageFormat, source: ExportSource, choices: ExportChoices) -> Self {
        let (min, max) = (pp_file::jpeg::MIN_QUALITY as u32, pp_file::jpeg::MAX_QUALITY as u32);
        let unit = choices.unit;
        let mut dialog = ExportDialog {
            frame: Frame::default(),
            format,
            source,
            size_by: choices.size_by,
            unit,
            scale: NumberField::new(choices.scale.clamp(MIN_SCALE, MAX_SCALE), MIN_SCALE, MAX_SCALE),
            width: unit.field(unit.value_of(source.width as f64, source.dpi)),
            height: unit.field(unit.value_of(source.height as f64, source.dpi)),
            resolution: NumberField::new(source.dpi, Document::MIN_DPI, Document::MAX_DPI),
            quality: format.has_quality().then(|| NumberField::new((choices.quality as u32).clamp(min, max), min, max)),
            kept_quality: choices.quality,
            focus: Field::Scale,
        };
        dialog.focus = dialog.first_field();
        dialog
    }

    fn height(&self) -> f32 {
        if self.quality.is_some() { HEIGHT + QUALITY_HEIGHT } else { HEIGHT }
    }

    fn field(&self, field: Field) -> Option<&NumberField> {
        match field {
            Field::Scale => Some(&self.scale),
            Field::Width => Some(&self.width),
            Field::Height => Some(&self.height),
            Field::Resolution => Some(&self.resolution),
            Field::Quality => self.quality.as_ref(),
        }
    }

    fn field_mut(&mut self, field: Field) -> Option<&mut NumberField> {
        match field {
            Field::Scale => Some(&mut self.scale),
            Field::Width => Some(&mut self.width),
            Field::Height => Some(&mut self.height),
            Field::Resolution => Some(&mut self.resolution),
            Field::Quality => self.quality.as_mut(),
        }
    }

    fn field_at(&self, x: f32, y: f32) -> Option<Field> {
        Field::ALL.into_iter().find(|&field| self.field(field).is_some() && self.field_rect(field).contains(x, y))
    }

    /// 選んでいる決め方の最初の欄。
    fn first_field(&self) -> Field {
        match self.size_by {
            SizeBy::Scale => Field::Scale,
            SizeBy::Size => Field::Width,
            SizeBy::Resolution => Field::Resolution,
        }
    }

    /// Tab で行き来する欄（選んでいる決め方の欄と品質）。
    fn tab_order(&self) -> Vec<Field> {
        Field::ALL.into_iter().filter(|f| f.size_by().map_or(self.quality.is_some(), |by| by == self.size_by)).collect()
    }

    /// 決めた品質。品質を選ばない形式では、前の設定をそのまま引き継ぐ。
    fn quality_value(&self) -> Option<u8> {
        match &self.quality {
            Some(field) => field.value().map(|q| q as u8),
            None => Some(self.kept_quality),
        }
    }

    /// 書き出す大きさと解像度。欄が空や範囲外なら `None`。
    fn output(&self) -> Option<Output> {
        let source = self.source;
        let side = |v: u32, ratio: f64| ((v as f64 * ratio).round() as u32).max(1);
        match self.size_by {
            SizeBy::Scale => {
                let ratio = self.scale.value()? as f64 / 100.0;
                Some(Output { width: side(source.width, ratio), height: side(source.height, ratio), dpi: source.dpi })
            }
            SizeBy::Size => {
                let pixels = |field: &NumberField| -> Option<u32> {
                    Some((self.unit.pixels_of(field.value()?, source.dpi).round() as u32).max(1))
                };
                Some(Output { width: pixels(&self.width)?, height: pixels(&self.height)?, dpi: source.dpi })
            }
            SizeBy::Resolution => {
                let dpi = self.resolution.value()?;
                let ratio = dpi as f64 / source.dpi as f64;
                Some(Output { width: side(source.width, ratio), height: side(source.height, ratio), dpi })
            }
        }
    }

    /// 書き出せる大きさなら、その大きさ。
    fn valid_output(&self) -> Option<Output> {
        self.output().filter(Output::fits)
    }

    /// 大きさの決め方を変える。新しい決め方の欄には、いま書き出す大きさを入れておく（CLIP STUDIO と同じく、
    /// 決め方を変えただけでは大きさが変わらない）。
    fn choose(&mut self, size_by: SizeBy) {
        if size_by == self.size_by {
            return;
        }
        if let Some(out) = self.output() {
            let source = self.source;
            match size_by {
                SizeBy::Scale => {
                    let scale = (out.width as f64 * 100.0 / source.width as f64).round() as u32;
                    self.scale.set_value(scale.clamp(MIN_SCALE, MAX_SCALE));
                }
                SizeBy::Size => {
                    // 出力サイズで決めると解像度は作品のままなので、その解像度で同じ画素数になる大きさにする
                    self.width.set_value(self.unit.value_of(out.width as f64, source.dpi));
                    self.height.set_value(self.unit.value_of(out.height as f64, source.dpi));
                }
                SizeBy::Resolution => {
                    let dpi = (source.dpi as f64 * out.width as f64 / source.width as f64).round() as u32;
                    self.resolution.set_value(dpi.clamp(Document::MIN_DPI, Document::MAX_DPI));
                }
            }
        }
        self.size_by = size_by;
        self.focus = self.first_field();
    }

    /// 幅・高さの単位を変える。入っている大きさは換算して残す。
    fn choose_unit(&mut self, unit: SizeUnit) {
        if unit == self.unit {
            return;
        }
        let dpi = self.source.dpi;
        let convert = |field: &NumberField, fallback: u32| {
            let pixels = field.value().map_or(fallback as f64, |v| self.unit.pixels_of(v, dpi));
            unit.field(unit.value_of(pixels, dpi))
        };
        let width = convert(&self.width, self.source.width);
        let height = convert(&self.height, self.source.height);
        self.width = width;
        self.height = height;
        self.unit = unit;
    }

    /// 幅か高さを変えたら、縦横比が作品と同じになるようにもう一方を合わせる。
    fn keep_aspect(&mut self, changed: Field) {
        let (source, unit) = (self.source, self.unit);
        let (from, to, ratio) = match changed {
            Field::Width => (&self.width, Field::Height, source.height as f64 / source.width as f64),
            Field::Height => (&self.height, Field::Width, source.width as f64 / source.height as f64),
            _ => return,
        };
        let Some(value) = from.value() else { return };
        let pixels = unit.pixels_of(value, source.dpi) * ratio;
        let other = unit.value_of(pixels, source.dpi);
        if let Some(field) = self.field_mut(to) {
            field.set_value(other);
        }
    }

    fn choices(&self) -> Option<ExportChoices> {
        Some(ExportChoices {
            size_by: self.size_by,
            unit: self.unit,
            scale: self.scale.value().unwrap_or(100),
            quality: self.quality_value()?,
        })
    }

    fn row_top(row: usize) -> f32 {
        theme::HEADER_HEIGHT + PAD + row as f32 * (BUTTON_HEIGHT + ROW_GAP)
    }

    fn mode_buttons(&self) -> Vec<(Button, SizeBy)> {
        SizeBy::ALL
            .into_iter()
            .map(|by| {
                let rect = self.frame.sub(PAD, Self::row_top(by.row()), MODE_WIDTH, BUTTON_HEIGHT);
                (Button::new(rect, by.label()).selected(by == self.size_by), by)
            })
            .collect()
    }

    /// 品質の欄の上端（書き出す大きさの行の下に、見出しを挟んで置く）。
    fn quality_top(&self) -> f32 {
        self.size_line_top() + LABEL_HEIGHT + PAD * 0.6 + LABEL_HEIGHT
    }

    /// 欄の左上（ダイアログの中の論理ピクセル）。
    fn field_origin(&self, field: Field) -> (f32, f32) {
        match field {
            Field::Scale => (FIELDS_LEFT, Self::row_top(0)),
            Field::Width => (FIELDS_LEFT + 22.0, Self::row_top(1)),
            Field::Height => (FIELDS_LEFT + 22.0 + FIELD_WIDTH + 34.0, Self::row_top(1)),
            Field::Resolution => (FIELDS_LEFT, Self::row_top(2)),
            Field::Quality => (FIELDS_LEFT, self.quality_top()),
        }
    }

    fn field_rect(&self, field: Field) -> Rect {
        let (left, top) = self.field_origin(field);
        self.frame.sub(left, top, FIELD_WIDTH, BUTTON_HEIGHT)
    }

    /// `left` から右端までに `labels` のボタンを等しい幅で並べる。
    fn button_row(&self, left: f32, top: f32, labels: impl ExactSizeIterator<Item = String>) -> Vec<Button> {
        let count = labels.len();
        let width = (WIDTH - PAD - left - 4.0 * (count - 1) as f32) / count as f32;
        labels
            .enumerate()
            .map(|(i, label)| {
                Button::new(self.frame.sub(left + i as f32 * (width + 4.0), top, width, BUTTON_HEIGHT), label)
            })
            .collect()
    }

    /// 欄の右に並べる、すぐ選べる値のボタン。`unit` は値の後ろに付ける文字。
    fn preset_buttons(&self, field: Field, values: &[u32], unit: &str) -> Vec<(Button, u32)> {
        let current = self.field(field).and_then(NumberField::value);
        let (_, top) = self.field_origin(field);
        self.button_row(PRESETS_LEFT, top, values.iter().map(|v| format!("{v}{unit}")))
            .into_iter()
            .zip(values)
            .map(|(button, &value)| (button.selected(current == Some(value)), value))
            .collect()
    }

    fn scale_buttons(&self) -> Vec<(Button, u32)> {
        self.preset_buttons(Field::Scale, &SCALES, "%")
    }

    fn resolution_buttons(&self) -> Vec<(Button, u32)> {
        self.preset_buttons(Field::Resolution, &RESOLUTIONS, "")
    }

    fn quality_buttons(&self) -> Vec<(Button, u32)> {
        if self.quality.is_none() {
            return Vec::new();
        }
        self.preset_buttons(Field::Quality, &QUALITIES, "")
    }

    fn unit_buttons(&self) -> Vec<(Button, SizeUnit)> {
        let left = FIELDS_LEFT + 22.0 + FIELD_WIDTH * 2.0 + 34.0 + 10.0;
        self.button_row(left, Self::row_top(1), SizeUnit::ALL.iter().map(|u| u.label().to_string()))
            .into_iter()
            .zip(SizeUnit::ALL)
            .map(|(button, unit)| (button.selected(unit == self.unit), unit))
            .collect()
    }

    fn size_line_top(&self) -> f32 {
        Self::row_top(3) - ROW_GAP + 4.0
    }

    fn size_line(&self) -> Rect {
        self.frame.sub(PAD, self.size_line_top(), WIDTH - PAD * 2.0, LABEL_HEIGHT)
    }

    fn export_button(&self) -> Button {
        Button::new(
            self.frame.sub(WIDTH - PAD - 180.0 - 6.0, self.height() - PAD - BUTTON_HEIGHT, 90.0, BUTTON_HEIGHT),
            "書き出し",
        )
        .enabled(self.valid_output().is_some() && self.quality_value().is_some())
    }

    fn cancel_button(&self) -> Button {
        Button::new(
            self.frame.sub(WIDTH - PAD - 90.0, self.height() - PAD - BUTTON_HEIGHT, 90.0, BUTTON_HEIGHT),
            "キャンセル",
        )
    }

    /// 書き出す大きさの説明と、その色。
    fn size_text(&self) -> (String, Rgba8) {
        let source = self.source;
        match self.output() {
            Some(out) if !out.fits() => (
                format!("{}×{}：一辺 {MAX_OUTPUT_SIDE} ピクセルまでです", out.width, out.height),
                theme::colors().warning,
            ),
            Some(out) => (
                format!(
                    "書き出す大きさ  {}×{} ピクセル・{} dpi（作品は {}×{}・{} dpi）",
                    out.width, out.height, out.dpi, source.width, source.height, source.dpi
                ),
                theme::colors().text,
            ),
            None => {
                let text = match self.size_by {
                    SizeBy::Scale => format!("倍率は {MIN_SCALE}〜{MAX_SCALE} の数字を入れてください"),
                    SizeBy::Size => "幅と高さを入れてください".to_string(),
                    SizeBy::Resolution => {
                        format!("解像度は {}〜{} の数字を入れてください", Document::MIN_DPI, Document::MAX_DPI)
                    }
                };
                (text, theme::colors().warning)
            }
        }
    }
}

impl Dialog for ExportDialog {
    fn place(&mut self, screen: Rect, scale: f32) {
        self.frame = Frame { rect: dialog::centered(screen, scale, WIDTH, self.height()), scale };
    }

    fn rect(&self) -> Rect {
        self.frame.rect
    }

    fn key(&mut self, key: DialogKey) -> bool {
        match key {
            DialogKey::Field(key) => {
                let focus = self.focus;
                let changed = self.field_mut(focus).is_some_and(|field| field.key(key));
                if changed {
                    self.keep_aspect(focus);
                }
                changed
            }
            DialogKey::Tab => {
                let order = self.tab_order();
                if order.len() < 2 {
                    return false;
                }
                let index = order.iter().position(|&f| f == self.focus).map_or(0, |i| (i + 1) % order.len());
                self.focus = order[index];
                // Windows と同じく、移った先の欄はすべて選んだ状態にする
                if let Some(field) = self.field_mut(self.focus) {
                    field.select_all();
                }
                true
            }
        }
    }

    fn press(&mut self, x: f32, y: f32, select: bool) -> DialogResult {
        if let Some((_, by)) = self.mode_buttons().into_iter().find(|(b, _)| b.hit(x, y)) {
            self.choose(by);
            return DialogResult::Open(true);
        }
        if let Some(field) = self.field_at(x, y) {
            // 選んでいない決め方の欄を押したら、その決め方に切り替える
            if let Some(by) = field.size_by() {
                self.choose(by);
            }
            let extend = select && field == self.focus;
            self.focus = field;
            if let Some(number) = self.field_mut(field) {
                number.press(x, extend);
            }
            return DialogResult::Open(true);
        }
        if let Some((_, scale)) = self.scale_buttons().into_iter().find(|(b, _)| b.hit(x, y)) {
            self.choose(SizeBy::Scale);
            self.scale.set_value(scale);
            return DialogResult::Open(true);
        }
        if let Some((_, unit)) = self.unit_buttons().into_iter().find(|(b, _)| b.hit(x, y)) {
            self.choose(SizeBy::Size);
            self.choose_unit(unit);
            return DialogResult::Open(true);
        }
        if let Some((_, dpi)) = self.resolution_buttons().into_iter().find(|(b, _)| b.hit(x, y)) {
            self.choose(SizeBy::Resolution);
            self.resolution.set_value(dpi);
            return DialogResult::Open(true);
        }
        if let Some((_, quality)) = self.quality_buttons().into_iter().find(|(b, _)| b.hit(x, y)) {
            if let Some(field) = self.quality.as_mut() {
                field.set_value(quality);
            }
            return DialogResult::Open(true);
        }
        if self.export_button().hit(x, y) {
            return self.confirm();
        }
        if self.cancel_button().hit(x, y) {
            return DialogResult::Cancel;
        }
        DialogResult::Open(false)
    }

    fn is_selecting(&self) -> bool {
        Field::ALL.into_iter().any(|f| self.field(f).is_some_and(NumberField::is_selecting))
    }

    fn drag(&mut self, x: f32) -> bool {
        self.field_mut(self.focus).is_some_and(|field| field.drag(x))
    }

    fn release(&mut self) {
        for f in Field::ALL {
            if let Some(field) = self.field_mut(f) {
                field.release();
            }
        }
    }

    fn wheel(&mut self, x: f32, y: f32, steps: i32, step: WheelStep) -> bool {
        let Some(field) = self.field_at(x, y) else { return false };
        // 幅・高さは、単位の 1（px は 10）を 1 段にする
        let size_step = if self.unit == SizeUnit::Pixel { 10 } else { 10u32.pow(self.unit.decimals()) };
        let (normal, fine, coarse) = match field {
            Field::Scale => (5, 1, 25),
            Field::Width | Field::Height => (size_step, (size_step / 10).max(1), size_step * 10),
            Field::Resolution => (10, 1, 50),
            Field::Quality => (5, 1, QUALITY_WHEEL_STEP_COARSE),
        };
        let amount = match step {
            WheelStep::Normal => normal,
            WheelStep::Fine => fine,
            WheelStep::Coarse => coarse,
        };
        if let Some(by) = field.size_by() {
            self.choose(by);
        }
        let changed = self.field_mut(field).is_some_and(|number| number.wheel(steps, amount));
        if changed {
            self.keep_aspect(field);
        }
        let moved = self.focus != field;
        self.focus = field;
        changed || moved
    }

    fn confirm(&self) -> DialogResult {
        match (self.valid_output(), self.choices()) {
            (Some(out), Some(choices)) => DialogResult::Done(Action::ExportImage {
                format: self.format,
                width: out.width,
                height: out.height,
                dpi: out.dpi,
                choices,
            }),
            _ => DialogResult::Open(false),
        }
    }

    fn render(&self, pixmap: &mut Pixmap) {
        let f = &self.frame;
        widgets::background(pixmap, f);
        pixmap.frame(f.rect, theme::colors().border);
        widgets::header(pixmap, f, &format!("画像を統合して書き出し（{}）", self.format.name()));
        let font = f.font_px(theme::BODY_TEXT);
        let label = |pixmap: &mut Pixmap, left: f32, top: f32, width: f32, text: &str, active: bool| {
            let color = if active { theme::colors().text } else { theme::colors().text_disabled };
            pixmap.text(f.sub(left, top, width, BUTTON_HEIGHT), text, font, color, Align::Left);
        };
        for (button, _) in self.mode_buttons() {
            button.render(pixmap, f);
        }
        for field in Field::ALL {
            let Some(number) = self.field(field) else { continue };
            number.render(pixmap, f, self.field_rect(field), self.focus == field);
        }

        let scale_active = self.size_by == SizeBy::Scale;
        label(pixmap, FIELDS_LEFT + FIELD_WIDTH + 4.0, Self::row_top(0), 16.0, "%", scale_active);
        let size_active = self.size_by == SizeBy::Size;
        label(pixmap, FIELDS_LEFT, Self::row_top(1), 22.0, "幅", size_active);
        label(pixmap, FIELDS_LEFT + 22.0 + FIELD_WIDTH + 6.0, Self::row_top(1), 28.0, "高さ", size_active);
        let resolution_active = self.size_by == SizeBy::Resolution;
        label(pixmap, FIELDS_LEFT + FIELD_WIDTH + 4.0, Self::row_top(2), 24.0, "dpi", resolution_active);
        let buttons = self
            .scale_buttons()
            .into_iter()
            .map(|(b, _)| {
                let on = scale_active && b.selected;
                b.selected(on)
            })
            .chain(self.unit_buttons().into_iter().map(|(b, _)| {
                let on = size_active && b.selected;
                b.selected(on)
            }))
            .chain(self.resolution_buttons().into_iter().map(|(b, _)| {
                let on = resolution_active && b.selected;
                b.selected(on)
            }));
        for button in buttons {
            button.render(pixmap, f);
        }

        let (text, color) = self.size_text();
        pixmap.text(self.size_line(), &text, font, color, Align::Left);
        if let Some(quality) = &self.quality {
            let top = self.quality_top();
            let heading = f.sub(PAD, top - LABEL_HEIGHT, WIDTH - PAD * 2.0, LABEL_HEIGHT);
            let (text, color) = match quality.value() {
                Some(_) => ("品質（大きいほどきれいで、ファイルも大きくなる）".to_string(), theme::colors().text),
                None => {
                    let (min, max) = (pp_file::jpeg::MIN_QUALITY, pp_file::jpeg::MAX_QUALITY);
                    (format!("品質は {min}〜{max} の数字を入れてください"), theme::colors().warning)
                }
            };
            pixmap.text(heading, &text, font, color, Align::Left);
            for (button, _) in self.quality_buttons() {
                button.render(pixmap, f);
            }
        }
        let export = self.export_button();
        let enabled = export.enabled;
        export.selected(enabled).render(pixmap, f);
        self.cancel_button().render(pixmap, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::number_field::FieldKey;

    fn center(r: Rect) -> (f32, f32) {
        ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5)
    }

    const SOURCE: ExportSource = ExportSource { width: 2504, height: 3228, dpi: 200 };

    fn open(format: ImageFormat, choices: ExportChoices) -> ExportDialog {
        let mut dialog = ExportDialog::new(format, SOURCE, choices);
        dialog.place(Rect::new(0, 0, 1200, 800), 1.0);
        dialog
    }

    fn dialog() -> ExportDialog {
        open(ImageFormat::Png, ExportChoices::default())
    }

    fn typed(dialog: &mut ExportDialog, text: &str) {
        dialog.key(DialogKey::Field(FieldKey::SelectAll));
        for c in text.chars() {
            dialog.key(DialogKey::Field(FieldKey::Char(c)));
        }
    }

    fn press_center(dialog: &mut ExportDialog, rect: Rect) -> DialogResult {
        let (x, y) = center(rect);
        dialog.press(x, y, false)
    }

    fn done(dialog: &ExportDialog) -> (u32, u32, u32) {
        match dialog.confirm() {
            DialogResult::Done(Action::ExportImage { width, height, dpi, .. }) => (width, height, dpi),
            other => panic!("書き出せない: {other:?}"),
        }
    }

    #[test]
    fn preset_scale_sets_the_output_size() {
        let mut dialog = dialog();
        let rect = dialog.scale_buttons()[3].0.rect;
        press_center(&mut dialog, rect);
        let rect = dialog.export_button().rect;
        assert_eq!(
            press_center(&mut dialog, rect),
            DialogResult::Done(Action::ExportImage {
                format: ImageFormat::Png,
                width: 4382,
                height: 5649,
                dpi: 200,
                choices: ExportChoices { scale: 175, ..ExportChoices::default() },
            })
        );
    }

    #[test]
    fn typed_and_wheeled_scale() {
        let mut dialog = dialog();
        typed(&mut dialog, "50");
        assert_eq!(done(&dialog), (1252, 1614, 200));
        let (x, y) = center(dialog.field_rect(Field::Scale));
        assert!(dialog.wheel(x, y, 2, WheelStep::Normal));
        assert_eq!(dialog.scale.value(), Some(60));
        typed(&mut dialog, "900");
        assert_eq!(dialog.confirm(), DialogResult::Open(false), "一辺が上限を超える");
        assert!(!dialog.export_button().enabled);
    }

    #[test]
    fn size_keeps_the_aspect_and_converts_units() {
        let mut dialog = dialog();
        let rect = dialog.mode_buttons()[1].0.rect;
        press_center(&mut dialog, rect);
        assert_eq!((dialog.size_by, dialog.focus), (SizeBy::Size, Field::Width));
        assert_eq!(done(&dialog), (2504, 3228, 200), "切り替えただけでは大きさは変わらない");
        typed(&mut dialog, "1252");
        assert_eq!(dialog.height.value(), Some(1614), "高さも縦横比どおりに変わる");
        assert_eq!(done(&dialog), (1252, 1614, 200));

        // 200dpi の 1252 ピクセルは 15.90 cm
        let rect = dialog.unit_buttons()[2].0.rect;
        press_center(&mut dialog, rect);
        assert_eq!(dialog.width.value(), Some(1590));
        dialog.key(DialogKey::Tab);
        assert_eq!(dialog.focus, Field::Height);
        typed(&mut dialog, "41");
        // 41 cm は 200dpi で 3228 ピクセル（元の高さ）
        assert_eq!(done(&dialog), (2504, 3228, 200));
    }

    #[test]
    fn resolution_changes_pixels_but_not_paper_size() {
        let mut dialog = dialog();
        let rect = dialog.resolution_buttons()[4].0.rect;
        press_center(&mut dialog, rect);
        assert_eq!(dialog.size_by, SizeBy::Resolution);
        assert_eq!(done(&dialog), (7512, 9684, 600));
        typed(&mut dialog, "350");
        assert_eq!(done(&dialog), (4382, 5649, 350));
        // 倍率に切り替えると、いまの大きさの倍率が入る
        let rect = dialog.mode_buttons()[0].0.rect;
        press_center(&mut dialog, rect);
        assert_eq!(dialog.scale.value(), Some(175));
        assert_eq!(done(&dialog), (4382, 5649, 200));
    }

    #[test]
    fn pressing_another_rows_field_switches_the_mode() {
        let mut dialog = dialog();
        let rect = dialog.field_rect(Field::Resolution);
        press_center(&mut dialog, rect);
        assert_eq!((dialog.size_by, dialog.focus), (SizeBy::Resolution, Field::Resolution));
        assert!(!dialog.key(DialogKey::Tab), "PNG では欄は解像度だけ");
    }

    #[test]
    fn remembered_choices_reopen() {
        let choices = ExportChoices { size_by: SizeBy::Size, unit: SizeUnit::Millimeter, scale: 50, quality: 70 };
        let dialog = open(ImageFormat::Png, choices);
        assert_eq!((dialog.size_by, dialog.focus), (SizeBy::Size, Field::Width));
        // 200dpi の 2504 ピクセルは 318.0 mm
        assert_eq!(dialog.width.value(), Some(3180));
        assert_eq!(dialog.choices(), Some(choices), "品質を選ばない形式でも、品質は引き継ぐ");
    }

    #[test]
    fn jpeg_quality_from_presets_and_typing() {
        let mut dialog = open(ImageFormat::Jpeg, ExportChoices::default());
        let rect = dialog.quality_buttons()[0].0.rect;
        press_center(&mut dialog, rect);
        assert_eq!(dialog.choices().map(|c| c.quality), Some(60));
        assert!(dialog.key(DialogKey::Tab));
        assert_eq!(dialog.focus, Field::Quality);
        dialog.key(DialogKey::Field(FieldKey::Backspace));
        assert_eq!(dialog.confirm(), DialogResult::Open(false), "品質が空なら書き出さない");
        assert!(!dialog.export_button().enabled);
        let (x, y) = center(dialog.field_rect(Field::Quality));
        typed(&mut dialog, "75");
        assert!(dialog.wheel(x, y, 1, WheelStep::Coarse));
        assert_eq!(dialog.quality_value(), Some(85));
    }

    #[test]
    fn everything_fits_inside_the_dialog() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg] {
            let mut dialog = open(format, ExportChoices::default());
            dialog.place(Rect::new(0, 0, 1200, 800), 1.5);
            let inside = |r: Rect| r.intersect(&dialog.rect()) == r;
            let buttons: Vec<Rect> = dialog
                .mode_buttons()
                .iter()
                .map(|(b, _)| b.rect)
                .chain(dialog.scale_buttons().iter().map(|(b, _)| b.rect))
                .chain(dialog.unit_buttons().iter().map(|(b, _)| b.rect))
                .chain(dialog.resolution_buttons().iter().map(|(b, _)| b.rect))
                .chain(dialog.quality_buttons().iter().map(|(b, _)| b.rect))
                .collect();
            assert!(buttons.iter().all(|&r| inside(r)));
            assert!(inside(dialog.export_button().rect) && inside(dialog.cancel_button().rect));
            assert!(dialog.field_rect(Field::Height).x1 < dialog.unit_buttons()[0].0.rect.x0);
            assert!(dialog.field_rect(Field::Resolution).y1 <= dialog.size_line().y0);
            let last = if dialog.quality.is_some() { dialog.field_rect(Field::Quality) } else { dialog.size_line() };
            assert!(last.y1 <= dialog.export_button().rect.y0);
        }
    }
}
