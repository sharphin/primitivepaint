use pp_core::{PaintMode, PressureCurve, Rect, Rgba8, TipShape};

use super::icons::{self, Icon};
use super::pixmap::Align;
use super::widgets::{self, IconButton, Slider};
use super::{Action, Frame, Panel, Pixmap, PressKind, Response, ToolIcon, UiState, theme};

const BUTTON_SIZE: f32 = 40.0;
const BUTTON_PITCH: f32 = 44.0;

/// 画面左端のツール列。ペンを絵柄で並べる。
#[derive(Default)]
pub struct ToolBar {
    frame: Frame,
}

impl ToolBar {
    pub fn button_rect(frame: &Frame, index: usize) -> Rect {
        frame.sub(4.0, 6.0 + index as f32 * BUTTON_PITCH, BUTTON_SIZE, BUTTON_SIZE)
    }

    /// ペンの下に、少し離して置く「直線」の切り替え。
    fn straight_line_button(&self, state: &UiState) -> IconButton {
        let y = 6.0 + state.tools.len() as f32 * BUTTON_PITCH + 10.0;
        let tooltip = if state.straight_line {
            "直線：ON（どう引いても、まっすぐな線になる）"
        } else {
            "直線：OFF"
        };
        IconButton::new(self.frame.sub(4.0, y, BUTTON_SIZE, BUTTON_SIZE), Icon::StraightLine, tooltip)
            .selected(state.straight_line)
    }
}

impl Panel for ToolBar {
    fn place(&mut self, frame: Frame) {
        self.frame = frame;
    }

    fn frame(&self) -> Frame {
        self.frame
    }

    fn render(&self, state: &UiState, pixmap: &mut Pixmap) {
        widgets::background(pixmap, &self.frame);
        for (i, tool) in state.tools.iter().enumerate() {
            let r = ToolBar::button_rect(&self.frame, i);
            if i == state.current_tool {
                pixmap.fill_rect(r, theme::colors().selected);
                pixmap.frame(r, theme::colors().selected_border);
            }
            draw_icon(pixmap, r, tool.icon);
            if !tool.usable {
                // このキャンバスでは使えないペンは薄く見せる
                pixmap.fill_rect(r, theme::veil(theme::colors().panel, 176));
            }
        }
        let separator = self.straight_line_button(state).rect;
        let y = separator.y0 - self.frame.px(6.0);
        pixmap.fill_rect(
            Rect::new(separator.x0 + self.frame.px(4.0), y, separator.x1 - self.frame.px(4.0), y + 1),
            theme::colors().border,
        );
        self.straight_line_button(state).render(pixmap);
    }

    fn tooltip(&self, x: f32, y: f32, state: &UiState) -> Option<String> {
        let line = self.straight_line_button(state);
        if line.rect.contains(x, y) {
            return Some(line.tooltip);
        }
        let index = (0..state.tools.len()).find(|&i| ToolBar::button_rect(&self.frame, i).contains(x, y))?;
        let tool = &state.tools[index];
        Some(if tool.usable {
            tool.name.clone()
        } else {
            format!("{}（このキャンバスでは使えません）", tool.name)
        })
    }

    fn press(&mut self, x: f32, y: f32, _kind: PressKind, state: &UiState) -> Response {
        if self.straight_line_button(state).hit(x, y) {
            return Response::action(Action::ToggleStraightLine);
        }
        let hit = (0..state.tools.len())
            .find(|&i| state.tools[i].usable && ToolBar::button_rect(&self.frame, i).contains(x, y));
        hit.map_or_else(Response::default, |i| Response::action(Action::SelectTool(i)))
    }
}

/// ペンの絵柄。左下にペン先が来るよう斜めに描く。座標は `r` の中の 0〜1。
fn draw_icon(pixmap: &mut Pixmap, r: Rect, icon: ToolIcon) {
    let size = r.width() as f32;
    let map = |(u, v): (f32, f32)| (r.x0 as f32 + u * size, r.y0 as f32 + v * size);
    let mut line = |a: (f32, f32), b: (f32, f32), width: f32, color: Rgba8| {
        pixmap.line(map(a), map(b), width * size, color);
    };
    const DARK: Rgba8 = Rgba8 { r: 40, g: 40, b: 44, a: 255 };
    const WOOD: Rgba8 = Rgba8 { r: 150, g: 98, b: 56, a: 255 };
    const METAL: Rgba8 = Rgba8 { r: 150, g: 152, b: 160, a: 255 };
    match icon {
        ToolIcon::InkPen => {
            line((0.55, 0.45), (0.86, 0.14), 0.13, DARK);
            let nib = [(0.14, 0.86), (0.42, 0.42), (0.58, 0.58)].map(map);
            pixmap.fill_polygon(&nib, METAL);
            pixmap.line(map((0.16, 0.84)), map((0.4, 0.6)), 1.0, DARK);
        }
        ToolIcon::Pencil => {
            line((0.34, 0.66), (0.86, 0.14), 0.18, Rgba8 { r: 236, g: 186, b: 40, a: 255 });
            let wood = [(0.14, 0.86), (0.27, 0.59), (0.41, 0.73)].map(map);
            pixmap.fill_polygon(&wood, Rgba8 { r: 226, g: 190, b: 140, a: 255 });
            let lead = [(0.14, 0.86), (0.19, 0.75), (0.25, 0.81)].map(map);
            pixmap.fill_polygon(&lead, DARK);
        }
        ToolIcon::OilBrush => {
            line((0.5, 0.5), (0.88, 0.12), 0.1, WOOD);
            line((0.38, 0.62), (0.52, 0.48), 0.17, METAL);
            line((0.17, 0.83), (0.4, 0.6), 0.2, Rgba8 { r: 196, g: 52, b: 40, a: 255 });
        }
        ToolIcon::WatercolorBrush => {
            line((0.5, 0.5), (0.88, 0.12), 0.08, DARK);
            line((0.4, 0.6), (0.52, 0.48), 0.12, METAL);
            let tip = [(0.12, 0.88), (0.3, 0.56), (0.38, 0.54), (0.46, 0.62), (0.44, 0.7)].map(map);
            pixmap.fill_polygon(&tip, Rgba8 { r: 52, g: 96, b: 200, a: 255 });
        }
        ToolIcon::Eraser => {
            line((0.22, 0.78), (0.44, 0.56), 0.3, Rgba8 { r: 250, g: 250, b: 250, a: 255 });
            line((0.44, 0.56), (0.78, 0.22), 0.3, Rgba8 { r: 60, g: 110, b: 200, a: 255 });
            let outline = [(0.11, 0.67), (0.67, 0.11), (0.89, 0.33), (0.33, 0.89)].map(map);
            for i in 0..4 {
                pixmap.line(outline[i], outline[(i + 1) % 4], 1.0, DARK);
            }
        }
        ToolIcon::KneadedEraser => {
            // 指でちぎった灰色の塊
            let lump = [(0.2, 0.52), (0.34, 0.3), (0.56, 0.24), (0.78, 0.34), (0.8, 0.58), (0.62, 0.76), (0.36, 0.78)]
                .map(map);
            pixmap.fill_polygon(&lump, Rgba8 { r: 150, g: 150, b: 156, a: 255 });
            for i in 0..lump.len() {
                pixmap.line(lump[i], lump[(i + 1) % lump.len()], 1.0, DARK);
            }
            pixmap.line(map((0.4, 0.46)), map((0.52, 0.4)), 1.0, Rgba8 { r: 200, g: 200, b: 204, a: 255 });
        }
    }
}

const SIZE_MIN: f32 = 1.0;
const SIZE_MAX: f32 = 1000.0;
const PAD: f32 = 10.0;
const ROW: f32 = 20.0;
const ROW_GAP: f32 = 8.0;
/// ブラシサイズの見本（CLIP STUDIO のブラシサイズパレットと同じく、押すとその大きさになる）。
const SIZE_SAMPLES: [f32; 30] = [
    1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 10.0, 12.0, 15.0, 17.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 70.0,
    80.0, 100.0, 120.0, 150.0, 170.0, 200.0, 250.0, 300.0, 400.0,
];
const SAMPLE_COLUMNS: usize = 6;
const SAMPLE_HEIGHT: f32 = 34.0;
const CURVE_HEIGHT: f32 = 28.0;
const MODE_BUTTON: f32 = 30.0;

/// つまみで動かしている値。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dragging {
    Size,
    Amount,
    Opacity,
}

/// つまみの段（絵柄・つまみ・値の表示）。
struct SliderRow {
    icon: Rect,
    slider: Slider,
    value: Rect,
}

/// ツールプロパティの中の配置。選んでいるペンによって段が増減する。
struct PropertyLayout {
    /// 同じ欄にあるペンを選ぶボタン（消しゴムの消し方など）。
    variants: Vec<Rect>,
    size: SliderRow,
    shapes: Vec<(IconButton, TipShape)>,
    samples: Vec<(Rect, f32)>,
    curves: Vec<Rect>,
    modes: Vec<(IconButton, PaintMode)>,
    amount: Option<SliderRow>,
    opacity: Option<SliderRow>,
    swatch: Rect,
}

/// ツール列の右のツールプロパティ。見出しにペンの名前を出し、項目は絵柄で並べる（名前はポインタを載せると出る）。
#[derive(Default)]
pub struct ToolProperty {
    frame: Frame,
    dragging: Option<Dragging>,
}

impl ToolProperty {
    fn content_width(&self) -> f32 {
        self.frame.rect.width() as f32 / self.frame.scale - PAD * 2.0
    }

    fn slider_row(&self, y: f32, value: f32) -> SliderRow {
        let width = self.content_width();
        SliderRow {
            icon: self.frame.sub(PAD, y, ROW, ROW),
            slider: Slider {
                rect: self.frame.sub(PAD + ROW + 8.0, y + 2.0, width - ROW - 8.0 - 46.0, ROW - 4.0),
                value,
            },
            value: self.frame.sub(PAD + width - 40.0, y, 40.0, ROW),
        }
    }

    fn layout(&self, state: &UiState) -> PropertyLayout {
        let width = self.content_width();
        let mut y = theme::HEADER_HEIGHT + ROW_GAP;
        let variants: Vec<Rect> = (0..state.variants.len())
            .map(|i| self.frame.sub(PAD + i as f32 * (MODE_BUTTON + 4.0), y, MODE_BUTTON, MODE_BUTTON))
            .collect();
        if !variants.is_empty() {
            y += MODE_BUTTON + ROW_GAP;
        }
        let size = self.slider_row(y, fraction_from_size(state.pen_size));
        y += ROW + 4.0;

        let cell = width / SAMPLE_COLUMNS as f32;
        let samples = SIZE_SAMPLES
            .iter()
            .enumerate()
            .map(|(i, &value)| {
                let (column, row) = ((i % SAMPLE_COLUMNS) as f32, (i / SAMPLE_COLUMNS) as f32);
                (self.frame.sub(PAD + column * cell, y + row * SAMPLE_HEIGHT, cell - 2.0, SAMPLE_HEIGHT - 2.0), value)
            })
            .collect();
        y += SAMPLE_HEIGHT * SIZE_SAMPLES.len().div_ceil(SAMPLE_COLUMNS) as f32 + ROW_GAP;

        let count = state.curves.len().max(1) as f32;
        let curve_width = (width - 4.0 * (count - 1.0)) / count;
        let curves = (0..state.curves.len())
            .map(|i| self.frame.sub(PAD + i as f32 * (curve_width + 4.0), y, curve_width, CURVE_HEIGHT))
            .collect();
        y += CURVE_HEIGHT + ROW_GAP;

        let mut shapes = Vec::new();
        if let Some(current) = state.pen_tip_shape {
            for (i, &shape) in TipShape::ALL.iter().enumerate() {
                let rect = self.frame.sub(PAD + i as f32 * (MODE_BUTTON + 4.0), y, MODE_BUTTON, MODE_BUTTON - 4.0);
                let icon = match shape {
                    TipShape::Round => Icon::RoundTip,
                    TipShape::Flat => Icon::FlatTip,
                    TipShape::Filbert => Icon::FilbertTip,
                };
                let tooltip = format!("筆の形：{}", shape.name());
                shapes.push((IconButton::new(rect, icon, tooltip).selected(shape == current), shape));
            }
            y += MODE_BUTTON + ROW_GAP - 4.0;
        }
        let mut modes = Vec::new();
        if let Some(current) = state.pen_paint_mode {
            for (i, &mode) in PaintMode::ALL.iter().enumerate() {
                let rect = self.frame.sub(PAD + i as f32 * (MODE_BUTTON + 4.0), y, MODE_BUTTON, MODE_BUTTON - 4.0);
                let icon = match mode {
                    PaintMode::Mix => Icon::MixPaint,
                    PaintMode::Overwrite => Icon::OverwritePaint,
                };
                let tooltip = format!("キャンバスでの描き方：{}", mode.name());
                modes.push((IconButton::new(rect, icon, tooltip).selected(mode == current), mode));
            }
            y += MODE_BUTTON + ROW_GAP - 4.0;
        }
        let mut slider_if = |value: Option<f32>| {
            value.map(|v| {
                let row = self.slider_row(y, v);
                y += ROW + ROW_GAP;
                row
            })
        };
        let amount = slider_if(state.pen_paint_amount);
        let opacity = slider_if(state.pen_opacity);

        let swatch = self.frame.sub(PAD, y, 44.0, 24.0);
        PropertyLayout { variants, size, shapes, samples, curves, modes, amount, opacity, swatch }
    }

    fn render_slider(&self, pixmap: &mut Pixmap, row: &SliderRow, icon: Icon, value: &str) {
        icons::draw(pixmap, row.icon, icon, theme::colors().text);
        row.slider.render(pixmap, &self.frame);
        pixmap.text(row.value, value, self.frame.font_px(theme::BODY_TEXT), theme::colors().text, Align::Right);
    }

    fn render_sample(&self, pixmap: &mut Pixmap, rect: Rect, size: f32, selected: bool) {
        if selected {
            pixmap.fill_rect(rect, theme::colors().selected);
            pixmap.frame(rect, theme::colors().selected_border);
        }
        // 点は実際の太さ（画面の倍率 1 のとき）で描き、枠に入らない大きさは枠いっぱいに収める
        let label_height = self.frame.px(12.0);
        let dot_area = Rect::new(rect.x0, rect.y0, rect.x1, rect.y1 - label_height);
        let max_diameter = dot_area.width().min(dot_area.height()) as f32 - self.frame.px(4.0) as f32;
        let diameter = (size * self.frame.scale).clamp(1.2, max_diameter.max(1.2));
        let (cx, cy) = ((dot_area.x0 + dot_area.x1) as f32 * 0.5, (dot_area.y0 + dot_area.y1) as f32 * 0.5);
        pixmap.fill_circle(cx, cy, diameter * 0.5, theme::colors().text);
        let label = Rect::new(rect.x0, rect.y1 - label_height - 1, rect.x1, rect.y1 - 1);
        let text = format!("{size}");
        pixmap.text(label, &text, self.frame.font_px(theme::SMALL_TEXT), theme::colors().text, Align::Center);
    }

    fn render_curve(&self, pixmap: &mut Pixmap, rect: Rect, curve: &PressureCurve, selected: bool) {
        let (fill, border) = if selected {
            (theme::colors().selected, theme::colors().selected_border)
        } else {
            (theme::colors().button, theme::colors().border)
        };
        pixmap.fill_rect(rect, fill);
        pixmap.frame(rect, border);
        // 横が入力（筆圧）、縦が出力。左下が 0
        let inset = self.frame.px(5.0) as f32;
        let (x0, y0) = (rect.x0 as f32 + inset, rect.y1 as f32 - inset);
        let (w, h) = (rect.width() as f32 - inset * 2.0, rect.height() as f32 - inset * 2.0);
        let points: Vec<(f32, f32)> =
            (0..=16).map(|i| i as f32 / 16.0).map(|t| (x0 + t * w, y0 - curve.apply(t) * h)).collect();
        for pair in points.windows(2) {
            pixmap.line(pair[0], pair[1], self.frame.scale * 1.5, theme::colors().text);
        }
    }

    fn value_at(&self, dragging: Dragging, x: f32, state: &UiState) -> Option<Action> {
        let layout = self.layout(state);
        match dragging {
            Dragging::Size => Some(Action::SetPenSize(size_from_fraction(layout.size.slider.value_at(x)))),
            Dragging::Amount => layout.amount.map(|row| Action::SetPaintAmount(row.slider.value_at(x))),
            Dragging::Opacity => layout.opacity.map(|row| Action::SetOpacity(row.slider.value_at(x))),
        }
    }
}

/// スライダー位置（0〜1）からペンのサイズへ。細い所を細かく選べるよう対数にする。
pub fn size_from_fraction(t: f32) -> f32 {
    SIZE_MIN * (SIZE_MAX / SIZE_MIN).powf(t.clamp(0.0, 1.0))
}

pub fn fraction_from_size(size: f32) -> f32 {
    ((size.clamp(SIZE_MIN, SIZE_MAX) / SIZE_MIN).ln() / (SIZE_MAX / SIZE_MIN).ln()).clamp(0.0, 1.0)
}

fn percent(value: f32) -> String {
    format!("{}%", (value * 100.0).round())
}

impl Panel for ToolProperty {
    fn place(&mut self, frame: Frame) {
        self.frame = frame;
    }

    fn frame(&self) -> Frame {
        self.frame
    }

    fn render(&self, state: &UiState, pixmap: &mut Pixmap) {
        widgets::background(pixmap, &self.frame);
        widgets::header(pixmap, &self.frame, &state.pen_name);
        let layout = self.layout(state);

        for (i, (rect, variant)) in layout.variants.iter().zip(&state.variants).enumerate() {
            if i == state.current_variant {
                pixmap.fill_rect(*rect, theme::colors().selected);
                pixmap.frame(*rect, theme::colors().selected_border);
            } else {
                pixmap.frame(*rect, theme::colors().border);
            }
            let inset = self.frame.px(3.0);
            draw_icon(
                pixmap,
                Rect::new(rect.x0 + inset, rect.y0 + inset, rect.x1 - inset, rect.y1 - inset),
                variant.icon,
            );
        }
        self.render_slider(pixmap, &layout.size, Icon::BrushSize, &format!("{:.1}", state.pen_size));
        for &(rect, size) in &layout.samples {
            self.render_sample(pixmap, rect, size, (state.pen_size - size).abs() < 0.05);
        }
        for (i, (rect, curve)) in layout.curves.iter().zip(&state.curves).enumerate() {
            self.render_curve(pixmap, *rect, curve, i == state.current_curve);
        }
        for (button, _) in &layout.shapes {
            button.render(pixmap);
        }
        for (button, _) in &layout.modes {
            button.render(pixmap);
        }
        if let (Some(row), Some(amount)) = (&layout.amount, state.pen_paint_amount) {
            self.render_slider(pixmap, row, Icon::PaintAmount, &percent(amount));
        }
        if let (Some(row), Some(opacity)) = (&layout.opacity, state.pen_opacity) {
            self.render_slider(pixmap, row, Icon::Opacity, &percent(opacity));
        }

        match state.pen_color {
            Some(color) => pixmap.fill_rect(layout.swatch, theme::opaque(color)),
            // 色を使わないペン（消しゴム）は斜線
            None => {
                let s = layout.swatch;
                pixmap.fill_rect(s, theme::colors().button);
                pixmap.line(
                    (s.x0 as f32, s.y1 as f32),
                    (s.x1 as f32, s.y0 as f32),
                    self.frame.scale,
                    theme::colors().border,
                );
            }
        }
        pixmap.frame(layout.swatch, theme::colors().border);
    }

    fn tooltip(&self, x: f32, y: f32, state: &UiState) -> Option<String> {
        let layout = self.layout(state);
        let on = |r: &Rect| r.contains(x, y);
        if let Some(i) = layout.variants.iter().position(on) {
            return Some(format!("消し方：{}", state.variants[i].name));
        }
        if on(&layout.size.icon) || on(&layout.size.slider.grab_rect()) {
            return Some("ブラシサイズ".into());
        }
        if let Some(&(_, size)) = layout.samples.iter().find(|(r, _)| on(r)) {
            return Some(format!("ブラシサイズ {size}"));
        }
        if let Some(i) = layout.curves.iter().position(on) {
            return Some(format!("筆圧：{}", state.curves[i].name()));
        }
        if let Some((button, _)) = layout.modes.iter().find(|(b, _)| on(&b.rect)) {
            return Some(button.tooltip.clone());
        }
        if let Some((button, _)) = layout.shapes.iter().find(|(b, _)| on(&b.rect)) {
            return Some(button.tooltip.clone());
        }
        if layout.amount.as_ref().is_some_and(|row| on(&row.icon) || on(&row.slider.grab_rect())) {
            return Some("絵の具の量（少ないと、描くうちにかすれる）".into());
        }
        if layout.opacity.as_ref().is_some_and(|row| on(&row.icon) || on(&row.slider.grab_rect())) {
            return Some("絵の具の不透明度".into());
        }
        if on(&layout.swatch) {
            return Some(match (state.pen_color, state.pen_takes_paint) {
                (Some(_), true) => "ペン先の色（パレットの RGB 表か、右クリックのスポイトで取る）".into(),
                (Some(_), false) => "ペン先の色（このペンは変えられません）".into(),
                (None, _) => "色を使わないペン".into(),
            });
        }
        None
    }

    fn press(&mut self, x: f32, y: f32, _kind: PressKind, state: &UiState) -> Response {
        let layout = self.layout(state);
        let grabbed = [
            (Some(&layout.size), Dragging::Size),
            (layout.amount.as_ref(), Dragging::Amount),
            (layout.opacity.as_ref(), Dragging::Opacity),
        ]
        .into_iter()
        .find_map(|(row, target)| row.filter(|r| r.slider.grab_rect().contains(x, y)).map(|_| target));
        if let Some(target) = grabbed {
            self.dragging = Some(target);
            return self.value_at(target, x, state).map_or_else(Response::default, Response::action);
        }
        if let Some(i) = layout.variants.iter().position(|r| r.contains(x, y)) {
            return Response::action(Action::SelectVariant(i));
        }
        if let Some(&(_, size)) = layout.samples.iter().find(|(r, _)| r.contains(x, y)) {
            return Response::action(Action::SetPenSize(size));
        }
        if let Some(i) = layout.curves.iter().position(|r| r.contains(x, y)) {
            return Response::action(Action::SelectCurve(i));
        }
        if let Some((_, shape)) = layout.shapes.iter().find(|(b, _)| b.hit(x, y)) {
            return Response::action(Action::SetTipShape(*shape));
        }
        let mode = layout.modes.into_iter().find(|(b, _)| b.hit(x, y));
        mode.map_or_else(Response::default, |(_, mode)| Response::action(Action::SetPaintMode(mode)))
    }

    fn drag(&mut self, x: f32, _y: f32, state: &UiState) -> Response {
        let Some(target) = self.dragging else { return Response::default() };
        self.value_at(target, x, state).map_or_else(Response::default, Response::action)
    }

    fn release(&mut self) -> Response {
        self.dragging = None;
        Response::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::state;

    fn property() -> ToolProperty {
        let mut property = ToolProperty::default();
        property.place(Frame { rect: Rect::new(48, 52, 244, 800), scale: 1.0 });
        property
    }

    fn center(r: Rect) -> (f32, f32) {
        ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5)
    }

    fn oil_state() -> UiState {
        let mut state = state();
        state.pen_paint_mode = Some(PaintMode::Mix);
        state.pen_tip_shape = Some(TipShape::Round);
        state.pen_paint_amount = Some(1.0);
        state.pen_opacity = Some(1.0);
        state
    }

    #[test]
    fn tip_shape_buttons_set_the_shape() {
        let mut property = property();
        let state = oil_state();
        let layout = property.layout(&state);
        assert_eq!(layout.shapes.len(), 3);
        let r = layout.shapes[1].0.rect;
        let (x, y) = ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5);
        assert_eq!(property.tooltip(x, y, &state).as_deref(), Some("筆の形：平筆"));
        assert_eq!(property.press(x, y, PressKind::Primary, &state).action, Some(Action::SetTipShape(TipShape::Flat)));
    }

    #[test]
    fn straight_line_toggle_sits_below_the_tools() {
        let mut bar = ToolBar::default();
        bar.place(Frame { rect: Rect::new(0, 100, 48, 800), scale: 1.0 });
        let state = crate::ui::tests::state();
        let button = bar.straight_line_button(&state).rect;
        let last_tool = ToolBar::button_rect(&bar.frame, state.tools.len() - 1);
        assert!(button.y0 > last_tool.y1);
        let (x, y) = ((button.x0 + button.x1) as f32 * 0.5, (button.y0 + button.y1) as f32 * 0.5);
        assert_eq!(bar.press(x, y, PressKind::Primary, &state).action, Some(Action::ToggleStraightLine));
        assert_eq!(bar.tooltip(x, y, &state).as_deref(), Some("直線：OFF"));
    }

    #[test]
    fn eraser_kinds_are_chosen_in_the_property_panel() {
        let mut property = property();
        let mut state = state();
        state.pen_color = None;
        state.variants = vec![
            crate::ui::ToolInfo { name: "消しゴム".into(), icon: ToolIcon::Eraser, usable: true },
            crate::ui::ToolInfo { name: "練り消し".into(), icon: ToolIcon::KneadedEraser, usable: true },
        ];
        let layout = property.layout(&state);
        assert_eq!(layout.variants.len(), 2);
        let r = layout.variants[1];
        let (x, y) = ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5);
        assert_eq!(property.tooltip(x, y, &state).as_deref(), Some("消し方：練り消し"));
        assert_eq!(property.press(x, y, PressKind::Primary, &state).action, Some(Action::SelectVariant(1)));
        assert!(property.layout(&crate::ui::tests::state()).variants.is_empty(), "1 種類しかない欄には出さない");
    }

    #[test]
    fn size_slider_round_trips() {
        for size in [1.0, 3.5, 12.0, 240.0, 1000.0] {
            let back = size_from_fraction(fraction_from_size(size));
            assert!((back - size).abs() / size < 1e-3, "{size} -> {back}");
        }
    }

    #[test]
    fn dragging_the_slider_sets_the_size() {
        let mut property = property();
        let state = state();
        let track = property.layout(&state).size.slider.rect;
        let y = (track.y0 + track.y1) as f32 * 0.5;
        let start = property.press(track.x0 as f32, y, PressKind::Primary, &state);
        assert_eq!(start.action, Some(Action::SetPenSize(1.0)));
        let end = property.drag(track.x1 as f32 + 50.0, y, &state);
        assert_eq!(end.action, Some(Action::SetPenSize(1000.0)));
        property.release();
        assert_eq!(property.drag(track.x0 as f32, y, &state).action, None);
    }

    #[test]
    fn size_samples_set_their_size() {
        let mut property = property();
        let state = state();
        let (rect, size) = property.layout(&state).samples[6];
        let (x, y) = center(rect);
        assert_eq!(property.press(x, y, PressKind::Primary, &state).action, Some(Action::SetPenSize(size)));
        assert_eq!(property.tooltip(x, y, &state), Some(format!("ブラシサイズ {size}")));
    }

    #[test]
    fn oil_rows_appear_only_for_pens_that_have_them() {
        let mut property = property();
        let plain = property.layout(&state());
        assert!(plain.modes.is_empty() && plain.amount.is_none() && plain.opacity.is_none());

        let state = oil_state();
        let layout = property.layout(&state);
        let (button, mode) = &layout.modes[1];
        assert_eq!(*mode, PaintMode::Overwrite);
        let (x, y) = center(button.rect);
        assert_eq!(
            property.press(x, y, PressKind::Primary, &state).action,
            Some(Action::SetPaintMode(PaintMode::Overwrite))
        );

        let amount = layout.amount.unwrap().slider.rect;
        let y = (amount.y0 + amount.y1) as f32 * 0.5;
        assert_eq!(
            property.press(amount.x0 as f32, y, PressKind::Primary, &state).action,
            Some(Action::SetPaintAmount(0.0))
        );
        property.release();
        let opacity = layout.opacity.unwrap().slider.rect;
        let y = (opacity.y0 + opacity.y1) as f32 * 0.5;
        let half = (opacity.x0 + opacity.x1) as f32 * 0.5;
        let Some(Action::SetOpacity(value)) = property.press(half, y, PressKind::Primary, &state).action else {
            panic!("不透明度が変わらない");
        };
        assert!((value - 0.5).abs() < 0.02, "{value}");
    }

    #[test]
    fn rows_do_not_overlap_and_fit_in_the_panel() {
        let property = property();
        let layout = property.layout(&oil_state());
        let mut rects = vec![layout.size.icon, layout.size.slider.grab_rect(), layout.swatch];
        rects.extend(layout.samples.iter().map(|(r, _)| *r));
        rects.extend(layout.curves.iter().copied());
        rects.extend(layout.modes.iter().map(|(b, _)| b.rect));
        rects.extend(layout.shapes.iter().map(|(b, _)| b.rect));
        rects.extend([layout.amount.unwrap().slider.rect, layout.opacity.unwrap().slider.rect]);
        for (i, a) in rects.iter().enumerate() {
            assert!(a.intersect(&property.frame.rect) == *a, "{i} はパネルからはみ出す");
            for b in &rects[i + 1..] {
                assert!(a.intersect(b).is_empty(), "{a:?} と {b:?} が重なる");
            }
        }
    }
}
