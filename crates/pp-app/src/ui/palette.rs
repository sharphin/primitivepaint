use pp_core::{Color, ColorChart, Palette, PaletteBoard, PaletteBrush, PaletteKnife, Rect, Rgba8};

use super::widgets;
use super::{Action, Frame, Panel, Pixmap, PressKind, Response, UiState};

const WIDTH: u32 = 288;
/// 色見本（RGB 表）の位置と大きさ。パネルの一番上に置く。
const CHART_LEFT: u32 = 4;
const CHART_WIDTH: u32 = WIDTH - CHART_LEFT * 2;
const CHART_TOP: u32 = 4;
const CHART_HEIGHT: u32 = 72;
const BOARD_TOP: u32 = CHART_TOP + CHART_HEIGHT + 4;
const BOARD_HEIGHT: u32 = 240;
const KNIFE_RADIUS: f32 = 10.0;
/// パレットの上でのペンの太さ（半径）の範囲。パレットは小さいので、キャンバスのサイズをそのままは使わない。
const BRUSH_RADIUS_MIN: f32 = 2.0;
const BRUSH_RADIUS_MAX: f32 = 20.0;

const BACKGROUND: Rgba8 = Rgba8 { r: 60, g: 60, b: 64, a: 255 };
const HIGHLIGHT: Rgba8 = Rgba8 { r: 255, g: 255, b: 255, a: 255 };

/// パネル上でいま何をしているか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gesture {
    None,
    /// パレットナイフで混ぜている。
    Knife,
    /// ペンでパレットに描いている（いつも混色）。
    Brush,
    /// スポイトで色を取っている。なぞった所の色が付き、離したときに最後に触れた色が残る。
    Eyedropper,
}

/// いま持っているペンが、パレットの上で何をするか。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PenOnPalette {
    /// その色の絵の具で描く。半径はパレットの画素で。
    Paint(Color, f32),
    /// 絵の具を拭き取る（消しゴム）。
    Wipe(f32),
    /// 何もしない（鉛筆のように絵の具を使わないペン）。
    Nothing,
}

impl PenOnPalette {
    pub fn from_state(state: &UiState) -> Self {
        let radius = (state.pen_size * 0.5).clamp(BRUSH_RADIUS_MIN, BRUSH_RADIUS_MAX);
        match state.pen_color {
            Some(color) if state.pen_takes_paint => PenOnPalette::Paint(color, radius),
            Some(_) => PenOnPalette::Nothing,
            None => PenOnPalette::Wipe(radius),
        }
    }
}

/// パネルを操作した結果。
#[derive(Clone, Copy, Debug, Default)]
pub struct PanelResponse {
    /// ペン先に付いた絵の具の色。
    pub picked: Option<Color>,
    /// 表示を更新する必要があるか。
    pub changed: bool,
}

/// 画面右上の混色パネル。上に色見本（RGB 表）、下にパレットが並ぶ。
///
/// - 色見本（RGB 表）のマスを押すと、その色がペン先に付く。
/// - ペンでパレットに描くと、その場の絵の具と混ざる（パレットの上はいつも混色）。消しゴムは絵の具を拭き取る。
/// - スポイト（右クリックか Alt + 押す）で、パレットの色をペン先に取る。スポイトはパレットの上でだけ使える。
/// - ナイフ操作（Shift + ドラッグ）で、絵の具を混ぜて伸ばす。
pub struct PalettePanel {
    palette: Palette,
    /// 下地の色。アプリ全体の設定で、作品には保存しない。
    board: PaletteBoard,
    knife: PaletteKnife,
    brush: Option<PaletteBrush>,
    chart: ColorChart,
    /// 色見本で選んだマス。
    chart_cell: Option<(u32, u32)>,
    gesture: Gesture,
    /// 前に `take_edited` で確かめてから、パレットの絵の具が変わったか。保存していない変更の印に使う。
    edited: bool,
}

impl PalettePanel {
    pub fn new() -> Self {
        PalettePanel {
            palette: Palette::new(WIDTH, BOARD_HEIGHT),
            board: PaletteBoard::default(),
            knife: PaletteKnife::new(KNIFE_RADIUS),
            brush: None,
            chart: ColorChart::default(),
            chart_cell: None,
            gesture: Gesture::None,
            edited: false,
        }
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// 下地の色を変える（オプションメニュー）。
    pub fn set_board(&mut self, board: PaletteBoard) {
        self.board = board;
    }

    /// パレットを差し替え、それまでのパレットを返す（タブを切り替えるとき）。
    /// 大きさが違えば差し替えず、渡されたものを `Err` で返す。
    pub fn replace_palette(&mut self, palette: Palette) -> Result<Palette, Palette> {
        if (palette.width(), palette.height()) != Self::palette_size() {
            return Err(palette);
        }
        self.release();
        self.edited = false;
        Ok(std::mem::replace(&mut self.palette, palette))
    }

    /// 新しい作品に使う、何も載っていないパレット。
    pub fn blank_palette() -> Palette {
        let (width, height) = Self::palette_size();
        Palette::new(width, height)
    }

    fn palette_size() -> (u32, u32) {
        (WIDTH, BOARD_HEIGHT)
    }

    /// 前に呼んでから絵の具が変わったか。
    pub fn take_edited(&mut self) -> bool {
        std::mem::take(&mut self.edited)
    }

    pub fn size(&self) -> (u32, u32) {
        (WIDTH, BOARD_TOP + BOARD_HEIGHT)
    }

    /// パネル座標 (x, y) で押した。`pen` はいま持っているペンがパレットの上ですること。
    pub fn press(&mut self, x: f32, y: f32, kind: PressKind, pen: PenOnPalette) -> PanelResponse {
        if y < BOARD_TOP as f32 {
            return self.choose_from_chart(x, y);
        }
        let by = y - BOARD_TOP as f32;
        match kind {
            PressKind::Eyedropper => {
                self.gesture = Gesture::Eyedropper;
                return PanelResponse { picked: self.palette.pick(x, by), changed: false };
            }
            PressKind::Knife => {
                self.gesture = Gesture::Knife;
                self.knife.begin(&self.palette, x, by);
                return PanelResponse::default();
            }
            PressKind::Primary => {}
        }
        let brush = match pen {
            PenOnPalette::Paint(color, radius) => PaletteBrush::paint(color, radius),
            PenOnPalette::Wipe(radius) => PaletteBrush::wipe(radius),
            PenOnPalette::Nothing => return PanelResponse::default(),
        };
        self.gesture = Gesture::Brush;
        let brush = self.brush.insert(brush);
        brush.stroke_to(&mut self.palette, x, by);
        self.edited = true;
        PanelResponse { picked: None, changed: true }
    }

    /// 色見本のマスを押した。その色をペン先に付ける。
    fn choose_from_chart(&mut self, x: f32, y: f32) -> PanelResponse {
        let u = (x - CHART_LEFT as f32) / CHART_WIDTH as f32;
        let v = (y - CHART_TOP as f32) / CHART_HEIGHT as f32;
        let Some((column, row)) = self.chart.cell_at(u, v) else {
            return PanelResponse::default();
        };
        self.chart_cell = Some((column, row));
        PanelResponse { picked: Some(self.chart.color_of(column, row)), changed: true }
    }

    pub fn drag(&mut self, x: f32, y: f32) -> PanelResponse {
        let by = y - BOARD_TOP as f32;
        match self.gesture {
            Gesture::Knife => self.knife.drag(&mut self.palette, x, by),
            Gesture::Brush => {
                if let Some(brush) = &mut self.brush {
                    brush.stroke_to(&mut self.palette, x, by);
                }
            }
            // 絵の具のない所（板の上やパレットの外）を通っても、付いている色はそのまま
            Gesture::Eyedropper => return PanelResponse { picked: self.palette.pick(x, by), changed: false },
            Gesture::None => return PanelResponse::default(),
        }
        self.edited = true;
        PanelResponse { picked: None, changed: true }
    }

    pub fn release(&mut self) -> PanelResponse {
        self.knife.end();
        self.brush = None;
        self.gesture = Gesture::None;
        PanelResponse::default()
    }

    pub fn render(&self) -> Vec<Rgba8> {
        let (w, h) = self.size();
        let mut out = vec![BACKGROUND; (w * h) as usize];
        self.render_chart(&mut out);
        self.palette.render(&mut out[(BOARD_TOP * w) as usize..], self.board);
        out
    }

    fn render_chart(&self, out: &mut [Rgba8]) {
        let columns = self.chart.columns();
        let rows = self.chart.rows();
        let cell_left = |c: u32| CHART_LEFT + c * CHART_WIDTH / columns;
        let cell_top = |r: u32| CHART_TOP + r * CHART_HEIGHT / rows;
        for row in 0..rows {
            for column in 0..columns {
                let color = super::theme::opaque(self.chart.color_of(column, row));
                let selected = self.chart_cell == Some((column, row));
                let (x0, x1) = (cell_left(column), cell_left(column + 1));
                let (y0, y1) = (cell_top(row), cell_top(row + 1));
                for y in y0..y1 {
                    for x in x0..x1 {
                        let edge = y == y0 || y + 1 == y1 || x == x0 || x + 1 == x1;
                        out[(y * WIDTH + x) as usize] = if selected && edge { HIGHLIGHT } else { color };
                    }
                }
            }
        }
    }
}

const DOCK_PAD: f32 = 6.0;

/// 右上の「パレット」パネル。混色パネルを見出し付きで、表示スケールに合わせて拡大して置く。
pub struct PaletteDock {
    frame: Frame,
    board: PalettePanel,
}

impl PaletteDock {
    pub const WIDTH: f32 = WIDTH as f32 + DOCK_PAD * 2.0;
    pub const HEIGHT: f32 = super::theme::HEADER_HEIGHT + (BOARD_TOP + BOARD_HEIGHT) as f32 + DOCK_PAD * 2.0;

    pub fn new() -> Self {
        PaletteDock { frame: Frame::default(), board: PalettePanel::new() }
    }

    pub fn board(&self) -> &PalettePanel {
        &self.board
    }

    pub fn board_mut(&mut self) -> &mut PalettePanel {
        &mut self.board
    }

    fn board_rect(&self) -> Rect {
        let (w, h) = self.board.size();
        self.frame.sub(DOCK_PAD, super::theme::HEADER_HEIGHT + DOCK_PAD, w as f32, h as f32)
    }

    /// 画面座標をパレット画像の座標にする。
    fn to_board(&self, x: f32, y: f32) -> (f32, f32) {
        let r = self.board_rect();
        ((x - r.x0 as f32) / self.frame.scale, (y - r.y0 as f32) / self.frame.scale)
    }

    fn respond(&self, response: PanelResponse) -> Response {
        Response { action: response.picked.map(Action::PickColor), redraw: response.changed }
    }
}

impl Panel for PaletteDock {
    fn place(&mut self, frame: Frame) {
        self.frame = frame;
    }

    fn frame(&self) -> Frame {
        self.frame
    }

    fn render(&self, _state: &UiState, pixmap: &mut Pixmap) {
        widgets::background(pixmap, &self.frame);
        widgets::header(pixmap, &self.frame, "パレット");
        pixmap.blit_scaled(self.board_rect(), &self.board.render(), self.board.size());
    }

    fn press(&mut self, x: f32, y: f32, kind: PressKind, state: &UiState) -> Response {
        if !self.board_rect().contains(x, y) {
            return Response::default();
        }
        let (bx, by) = self.to_board(x, y);
        let response = self.board.press(bx, by, kind, PenOnPalette::from_state(state));
        self.respond(response)
    }

    fn drag(&mut self, x: f32, y: f32, _state: &UiState) -> Response {
        let (bx, by) = self.to_board(x, y);
        let response = self.board.drag(bx, by);
        self.respond(response)
    }

    fn release(&mut self) -> Response {
        let response = self.board.release();
        self.respond(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YELLOW: Color = Color { r: 0.98, g: 0.82, b: 0.05, a: 1.0 };
    const BLUE: Color = Color { r: 0.12, g: 0.18, b: 0.62, a: 1.0 };

    /// パレット（板）の上の点をパネル座標にする。
    fn on_board(x: f32, y: f32) -> (f32, f32) {
        (x, BOARD_TOP as f32 + y)
    }

    fn eyedrop(panel: &mut PalettePanel, x: f32, y: f32) -> PanelResponse {
        panel.press(x, y, PressKind::Eyedropper, PenOnPalette::Nothing)
    }

    /// `color` のペンで (x0, y) から (x1, y) までなぞる。
    fn paint(panel: &mut PalettePanel, color: Color, radius: f32, (x0, x1): (f32, f32), y: f32) {
        let pen = PenOnPalette::Paint(color, radius);
        assert!(panel.press(x0, y, PressKind::Primary, pen).changed);
        panel.drag(x1, y);
        panel.release();
    }

    #[test]
    fn touching_the_chart_puts_its_color_on_the_pen() {
        let mut panel = PalettePanel::new();
        // 色見本の真ん中の行、左端（赤）を押す
        let row_middle = CHART_TOP as f32 + CHART_HEIGHT as f32 * 0.5;
        let pen = PenOnPalette::Nothing;
        let picked = panel.press(CHART_LEFT as f32 + 2.0, row_middle, PressKind::Primary, pen).picked.unwrap();
        assert!(picked.r > 0.99 && picked.g < 0.01 && picked.b < 0.01, "{picked:?}");
        // 右端の列は無彩色
        let right = CHART_LEFT as f32 + CHART_WIDTH as f32 - 2.0;
        let gray = panel.press(right, row_middle, PressKind::Primary, pen).picked.unwrap();
        assert!((gray.r - gray.g).abs() < 0.01 && (gray.g - gray.b).abs() < 0.01, "{gray:?}");
        // 表と板の間の隙間では何も起きない
        assert!(panel.press(100.0, BOARD_TOP as f32 - 1.0, PressKind::Primary, pen).picked.is_none());
    }

    #[test]
    fn pen_paints_on_the_palette_and_mixes_without_changing_its_color() {
        let mut panel = PalettePanel::new();
        let (_, by) = on_board(0.0, 100.0);
        paint(&mut panel, YELLOW, 10.0, (140.0, 160.0), by);
        assert!(eyedrop(&mut panel, 150.0, by).picked.unwrap().r > 0.9);

        // 青いペンで黄色の上をなぞる
        let blue = PenOnPalette::Paint(BLUE, 6.0);
        let start = panel.press(120.0, by, PressKind::Primary, blue);
        assert!(start.changed && start.picked.is_none());
        assert!(panel.drag(150.0, by).picked.is_none(), "描いてもペンの色は変わらない");
        panel.drag(180.0, by);
        panel.release();
        let mixed = eyedrop(&mut panel, 150.0, by).picked.unwrap();
        assert!(mixed.g > mixed.r && mixed.g > mixed.b, "{mixed:?}");
        // 何もなかった所には青が載る
        assert!(eyedrop(&mut panel, 116.0, by).picked.unwrap().b > 0.4);
    }

    #[test]
    fn eraser_wipes_and_pencil_does_nothing_on_the_palette() {
        let mut panel = PalettePanel::new();
        let (bx, by) = on_board(150.0, 100.0);
        paint(&mut panel, YELLOW, 10.0, (bx, bx), by);

        assert!(!panel.press(bx, by, PressKind::Primary, PenOnPalette::Nothing).changed);
        panel.release();
        assert!(eyedrop(&mut panel, bx, by).picked.is_some());
        panel.release();

        panel.press(bx, by, PressKind::Primary, PenOnPalette::Wipe(16.0));
        panel.release();
        assert!(eyedrop(&mut panel, bx, by).picked.is_none());
    }

    #[test]
    fn dragging_the_eyedropper_through_paint_keeps_the_last_color_touched() {
        let mut panel = PalettePanel::new();
        let (bx, by) = on_board(150.0, 100.0);
        paint(&mut panel, BLUE, 10.0, (bx, bx), by);

        // 何もない所で押してから絵の具の上を通り、また何もない所で離す
        let (sx, sy) = on_board(40.0, 100.0);
        assert!(eyedrop(&mut panel, sx, sy).picked.is_none());
        let picked = panel.drag(bx, by).picked.unwrap();
        assert!(picked.b > picked.r, "{picked:?}");
        let (ex, ey) = on_board(260.0, 100.0);
        assert!(panel.drag(ex, ey).picked.is_none(), "板の上では色が変わらない");
        assert!(panel.release().picked.is_none());
    }

    #[test]
    fn knife_spreads_paint() {
        let mut panel = PalettePanel::new();
        let (bx, by) = on_board(100.0, 100.0);
        paint(&mut panel, YELLOW, 8.0, (bx, bx), by);
        panel.press(bx, by, PressKind::Knife, PenOnPalette::Nothing);
        panel.drag(bx + 60.0, by);
        panel.release();
        assert!(eyedrop(&mut panel, bx + 40.0, by).picked.is_some());
    }

    #[test]
    fn render_matches_panel_size() {
        let mut panel = PalettePanel::new();
        let (w, h) = panel.size();
        assert_eq!(panel.render().len(), (w * h) as usize);
        let right = CHART_LEFT as f32 + CHART_WIDTH as f32 - 1.0;
        panel.press(right, BOARD_TOP as f32 - 5.0, PressKind::Primary, PenOnPalette::Nothing);
        assert_eq!(panel.render().len(), (w * h) as usize);
    }
}
