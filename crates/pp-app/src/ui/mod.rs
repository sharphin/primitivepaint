//! CLIP STUDIO PAINT の配置にならった画面まわり（ウィンドウ枠の中のパネル）。
//!
//! 上にメニューバーとコマンドバー、左にツールとツールプロパティ、右上に混色パレット、右下に紙の一覧を置き、
//! 残った中央の上に作品のタブ、その下にキャンバスを表示する。メニューの一覧と「新規」のダイアログは、その上に重ねる。パネルは CPU で 1 枚の画像（`Pixmap`）に描き、変わった所だけ GPU に送る。

mod command_bar;
mod dialog;
mod dirty;
mod export_dialog;
mod font;
mod icons;
mod menu;
mod new_document;
mod number_field;
mod palette;
mod pixmap;
mod scroll_bar;
mod sheets;
mod tabs;
pub mod theme;
mod tools;
mod widgets;

use pp_core::{CanvasMode, Color, PaintMode, PaletteBoard, PressureCurve, Rect, Rgba8, SheetMaterial, TipShape};
use pp_file::{CopyFormat, ImageFormat};

use command_bar::CommandBar;
use dialog::{Dialog, DialogResult};
pub use dialog::{DialogKey, WheelStep};
use dirty::DirtyRegion;
use export_dialog::ExportDialog;
pub use export_dialog::{ExportChoices, ExportSource};
use menu::MenuBar;
use new_document::NewDocumentDialog;
pub use number_field::FieldKey;
use palette::PaletteDock;
pub use palette::PalettePanel;
use pixmap::Pixmap;
use scroll_bar::ScrollBar;
use sheets::SheetPanel;
use tabs::TabBar;
use tools::{ToolBar, ToolProperty};

use crate::view::ScrollSpan;

pub use font::Font;

/// 拡大・縮小のボタンやキーで、1 回に何倍にするか。
pub const ZOOM_STEP: f32 = 1.25;

/// パネルの操作から App に頼むこと。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// ツール列の欄を選ぶ。
    SelectTool(usize),
    /// 今の欄の中で、何番目のペンを使うか（消しゴムの消し方など）。
    SelectVariant(usize),
    SetPenSize(f32),
    SelectCurve(usize),
    /// 画面の配色（ライト・ダーク）を切り替える。
    SetTheme(theme::UiTheme),
    /// 混色パレットの下地の色を選ぶ。
    SetPaletteBoard(PaletteBoard),
    /// パレットからペン先に絵の具を付けた（スポイト）。
    PickColor(Color),
    SetPaintMode(PaintMode),
    /// 油彩・水彩の筆の形。
    SetTipShape(TipShape),
    /// 直線で引くかを切り替える。ペンは持ち替えない。
    ToggleStraightLine,
    /// 滑らかな回転を切り替える。ON の間は描かず、押した所を軸に回す。
    ToggleSmoothRotate,
    /// 油彩などで、ペン先に含ませる絵の具の量（0..=1）。
    SetPaintAmount(f32),
    /// 絵の具の不透明度（0..=1）。
    SetOpacity(f32),
    /// 回転の刻みを何回分回すか（負なら左回り）。
    RotateStep(f32),
    ResetRotation,
    /// 画面中央を中心に何倍にするか。
    Zoom(f32),
    /// スクロールバーのつまみの先頭を `start`（割合）に動かす。
    ScrollTo {
        vertical: bool,
        start: f32,
    },
    FitView,
    /// 等倍（100%）で表示する。
    ActualSize,
    AddSheet(SheetMaterial),
    RemoveSheet,
    /// 選んでいる紙を上（true）か下へ入れ替える。
    MoveSheet(bool),
    /// `Document::layers` の番号で紙を選ぶ。
    SelectSheet(usize),
    /// `Document::layers` の番号の紙をめくる・戻す。
    FlipSheet(usize),
    /// 「新規」のダイアログを開く。
    ShowNewDocument,
    /// 新しい作品をタブで開く。
    NewDocument {
        mode: CanvasMode,
        width: u32,
        height: u32,
    },
    Open,
    Save,
    SaveAs,
    /// 見た目どおりに合成した画像の書き出し。まず設定（倍率）を開く。
    Export(ImageFormat),
    /// 決めた大きさ（ピクセル）・解像度で書き出す。`choices` は次に開いたときの初期値にする。
    ExportImage {
        format: ImageFormat,
        width: u32,
        height: u32,
        dpi: u32,
        choices: ExportChoices,
    },
    /// 作品をほかのソフトの形式で保存する（「複製を保存」）。作品の保存先は変えない。
    SaveCopy(CopyFormat),
    SelectTab(usize),
    CloseTab(usize),
    CloseCurrentTab,
}

/// どう押したか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressKind {
    /// 左ボタンかペン先。
    Primary,
    /// Shift を押しながら。パレットではパレットナイフになる。
    Knife,
    /// 右ボタンか、Alt を押しながら。パレットではスポイトになる。
    Eyedropper,
}

/// パネルを操作した結果。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Response {
    pub action: Option<Action>,
    /// そのパネルを描き直す必要があるか。
    pub redraw: bool,
}

impl Response {
    pub fn action(action: Action) -> Self {
        Response { action: Some(action), redraw: true }
    }
}

/// ツール列に出す絵柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolIcon {
    InkPen,
    Pencil,
    OilBrush,
    WatercolorBrush,
    Eraser,
    KneadedEraser,
}

pub struct ToolInfo {
    pub name: String,
    pub icon: ToolIcon,
    /// 今のキャンバスモードで使えるか。
    pub usable: bool,
}

pub struct TabInfo {
    pub name: String,
    /// 保存していない変更があるか。
    pub modified: bool,
}

pub struct SheetInfo {
    /// `Document::layers` の番号。
    pub index: usize,
    pub name: String,
    pub material: SheetMaterial,
    pub visible: bool,
    pub active: bool,
}

/// キャンバスの上に出すペン先の輪郭。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushCursor {
    pub x: f32,
    pub y: f32,
    /// ペン先の半径（画面のピクセル、筆圧 1 のとき）。
    pub radius: f32,
}

impl BrushCursor {
    /// 小さいペン先でも位置が分かるよう、これより小さい輪郭は描かず十字だけにする。
    const MIN_RING: f32 = 3.0;
    const CROSS: f32 = 5.0;

    fn bounds(&self) -> Rect {
        // 十字は画面の倍率で 3 倍まで大きくなる
        let reach = self.radius.max(Self::CROSS * 3.0) + 3.0;
        Rect::new(
            (self.x - reach).floor() as i32,
            (self.y - reach).floor() as i32,
            (self.x + reach).ceil() as i32 + 1,
            (self.y + reach).ceil() as i32 + 1,
        )
    }

    fn render(&self, pixmap: &mut Pixmap, scale: f32) {
        let (x, y) = (self.x, self.y);
        let mut lines: Vec<Segment> = Vec::new();
        if self.radius >= Self::MIN_RING {
            lines.extend(circle_segments((x, y), self.radius));
        }
        let arm = Self::CROSS * scale.clamp(1.0, 3.0);
        lines.push(((x - arm, y), (x - 1.5, y)));
        lines.push(((x + 1.5, y), (x + arm, y)));
        lines.push(((x, y - arm), (x, y - 1.5)));
        lines.push(((x, y + 1.5), (x, y + arm)));
        draw_outline(pixmap, &lines);
    }
}

/// 直線を引いている間に出す下書き。筆の幅の輪郭と、真ん中の線。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineGuide {
    pub from: (f32, f32),
    pub to: (f32, f32),
    /// 筆の半径（画面のピクセル、筆圧 1 のとき）。
    pub radius: f32,
}

impl LineGuide {
    /// 長い線を分ける長さ（画面のピクセル）。分けた線ごとに描き直すので、斜めの長い線でも間の何もない所は描かない。
    const PIECE: f32 = 96.0;
    /// 線の中心から、描く線の外側までの距離（縁取りの太さの半分 + にじみ）。
    const REACH: f32 = 3.0;

    /// 描く線分。長い線は短く分けておく。
    fn segments(&self) -> Vec<Segment> {
        let (a, b, r) = (self.from, self.to, self.radius);
        let mut lines = vec![(a, b)];
        if r >= BrushCursor::MIN_RING {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.5 {
                // 線の両側の縁
                let (nx, ny) = (-dy / len * r, dx / len * r);
                lines.push(((a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny)));
                lines.push(((a.0 - nx, a.1 - ny), (b.0 - nx, b.1 - ny)));
            }
            lines.extend(circle_segments(a, r));
            lines.extend(circle_segments(b, r));
        }
        lines.into_iter().flat_map(split_segment).collect()
    }

    /// 線分が描く範囲。
    fn piece_bounds(&(a, b): &Segment) -> Rect {
        Rect::new(
            (a.0.min(b.0) - Self::REACH).floor() as i32,
            (a.1.min(b.1) - Self::REACH).floor() as i32,
            (a.0.max(b.0) + Self::REACH).ceil() as i32 + 1,
            (a.1.max(b.1) + Self::REACH).ceil() as i32 + 1,
        )
    }

    /// 描く範囲を、線分ごとに分けたもの。
    fn pieces(&self) -> Vec<Rect> {
        self.segments().iter().map(Self::piece_bounds).collect()
    }

    /// `area` にかかる線分だけを描く。
    fn render(&self, pixmap: &mut Pixmap, area: Rect) {
        let lines: Vec<Segment> =
            self.segments().into_iter().filter(|s| !Self::piece_bounds(s).intersect(&area).is_empty()).collect();
        draw_outline(pixmap, &lines);
    }
}

/// 長い線分を `LineGuide::PIECE` 以下の長さに分ける。
fn split_segment((a, b): Segment) -> Vec<Segment> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let count = ((dx * dx + dy * dy).sqrt() / LineGuide::PIECE).ceil().max(1.0) as usize;
    let point = |i: usize| {
        let t = i as f32 / count as f32;
        (a.0 + dx * t, a.1 + dy * t)
    };
    (0..count).map(|i| (point(i), point(i + 1))).collect()
}

/// 画面上の線分（始点, 終点）。
type Segment = ((f32, f32), (f32, f32));

/// 中心 `c`、半径 `radius` の円を線分に分ける。
fn circle_segments(c: (f32, f32), radius: f32) -> Vec<Segment> {
    let segments = (radius * 0.8).clamp(16.0, 96.0) as usize;
    let point = |i: usize| {
        let t = i as f32 / segments as f32 * std::f32::consts::TAU;
        (c.0 + radius * t.cos(), c.1 + radius * t.sin())
    };
    (0..segments).map(|i| (point(i), point(i + 1))).collect()
}

/// どんな色の上でも見えるよう、白の縁取りに黒の線で描く。
fn draw_outline(pixmap: &mut Pixmap, lines: &[Segment]) {
    const LIGHT: Rgba8 = Rgba8 { r: 255, g: 255, b: 255, a: 200 };
    const DARK: Rgba8 = Rgba8 { r: 30, g: 30, b: 30, a: 255 };
    for &(a, b) in lines {
        pixmap.line(a, b, 4.0, LIGHT);
    }
    for &(a, b) in lines {
        pixmap.line(a, b, 2.0, DARK);
    }
}

/// パネルの表示に必要な、アプリの今の状態。
pub struct UiState {
    pub mode: CanvasMode,
    /// ツール列の欄。複数のペンがある欄は、最後に選んだペンを出す。
    pub tools: Vec<ToolInfo>,
    pub current_tool: usize,
    pub pen_name: String,
    /// 今の欄にあるペン（1 つしかない欄は空）。ツールプロパティで選ぶ。
    pub variants: Vec<ToolInfo>,
    pub current_variant: usize,
    /// 筆の形。選べないペンは `None`。
    pub pen_tip_shape: Option<TipShape>,
    /// 直線で引くか。
    pub straight_line: bool,
    /// 滑らかな回転が ON か。
    pub smooth_rotate: bool,
    pub pen_size: f32,
    pub pen_color: Option<Color>,
    /// パレットの絵の具を付けて描くペンか（パレットの上にも描ける）。
    pub pen_takes_paint: bool,
    /// キャンバス上での描き方。選べないペンは `None`。
    pub pen_paint_mode: Option<PaintMode>,
    /// 絵の具の量・不透明度。選べないペンは `None`。
    pub pen_paint_amount: Option<f32>,
    pub pen_opacity: Option<f32>,
    pub curves: Vec<PressureCurve>,
    pub current_curve: usize,
    /// 下から順に並んだ紙。
    pub sheets: Vec<SheetInfo>,
    pub tabs: Vec<TabInfo>,
    pub current_tab: usize,
    /// キャンバスの横・縦のスクロール位置。
    pub scroll_h: ScrollSpan,
    pub scroll_v: ScrollSpan,
}

/// パネルの画面上の位置と、UI の拡大率（Windows の表示スケール）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub rect: Rect,
    pub scale: f32,
}

impl Default for Frame {
    fn default() -> Self {
        Frame { rect: Rect::EMPTY, scale: 1.0 }
    }
}

impl Frame {
    /// 論理ピクセルを物理ピクセルにする。
    pub fn px(&self, logical: f32) -> i32 {
        (logical * self.scale).round() as i32
    }

    /// パネル左上からの論理座標で矩形を作る。
    pub fn sub(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        let x0 = self.rect.x0 + self.px(x);
        let y0 = self.rect.y0 + self.px(y);
        Rect::new(x0, y0, x0 + self.px(w), y0 + self.px(h))
    }

    pub fn font_px(&self, logical: f32) -> f32 {
        logical * self.scale
    }
}

/// 画面の区画の 1 つ。各パネルはこのトレイトを実装する。
pub trait Panel {
    fn place(&mut self, frame: Frame);
    fn frame(&self) -> Frame;
    fn render(&self, state: &UiState, pixmap: &mut Pixmap);
    /// 画面座標 (x, y) で押した。
    fn press(&mut self, x: f32, y: f32, kind: PressKind, state: &UiState) -> Response;
    fn drag(&mut self, _x: f32, _y: f32, _state: &UiState) -> Response {
        Response::default()
    }
    fn release(&mut self) -> Response {
        Response::default()
    }
    /// ポインタが (x, y) に来た（押していない）。表示が変わったら true。
    fn hover(&mut self, _x: f32, _y: f32) -> bool {
        false
    }
    /// (x, y) にあるボタンなどの名前。ポインタを載せたときに説明として出す。
    fn tooltip(&self, _x: f32, _y: f32, _state: &UiState) -> Option<String> {
        None
    }
}

/// ポインタの近くに出す説明。
struct Tooltip {
    text: String,
    rect: Rect,
}

const TOOLTIP_TEXT: f32 = theme::BODY_TEXT;

/// パネルの名前。描き直しの指定に使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelId {
    Menu,
    Command,
    Tabs,
    Tools,
    Property,
    Palette,
    Sheets,
    ScrollH,
    ScrollV,
}

const PANEL_IDS: [PanelId; 9] = [
    PanelId::Menu,
    PanelId::Command,
    PanelId::Tabs,
    PanelId::Tools,
    PanelId::Property,
    PanelId::Palette,
    PanelId::Sheets,
    PanelId::ScrollH,
    PanelId::ScrollV,
];

// 区画の大きさ（論理ピクセル）
const MENU_BAR_HEIGHT: f32 = 22.0;
const COMMAND_BAR_HEIGHT: f32 = 30.0;
const TAB_BAR_HEIGHT: f32 = 19.0;
const SCROLL_BAR_WIDTH: f32 = 12.0;
const TOOL_BAR_WIDTH: f32 = 48.0;
const PROPERTY_WIDTH: f32 = 196.0;

/// ウィンドウ内のパネル一式。
pub struct Chrome {
    pixmap: Pixmap,
    size: (u32, u32),
    scale: f32,
    canvas_area: Rect,
    menu: MenuBar,
    command: CommandBar,
    tabs: TabBar,
    tools: ToolBar,
    property: ToolProperty,
    palette: PaletteDock,
    sheets: SheetPanel,
    scroll_h: ScrollBar,
    scroll_v: ScrollBar,
    /// 最後に描いたスクロールの位置。変わったらスクロールバーを描き直す。
    drawn_scroll: (ScrollSpan, ScrollSpan),
    /// 開いている「新規」のダイアログ。開いている間は、ほかのパネルもキャンバスも操作できない。
    dialog: Option<Box<dyn Dialog>>,
    /// ダイアログの欄の文字を、マウスで選んでいる途中か。
    dialog_selecting: bool,
    /// Shift を押しているか。ダイアログの欄で、押した位置まで選択を広げるのに使う。
    shift: bool,
    tooltip: Option<Tooltip>,
    /// キャンバスの上に出すペン先の輪郭。OS のカーソルの代わりに使う。
    brush_cursor: Option<BrushCursor>,
    /// 直線を引いている間の下書き。
    line_guide: Option<LineGuide>,
    /// 押したまま操作しているパネル。
    captured: Option<PanelId>,
    /// 次に描き直す範囲。
    dirty: DirtyRegion,
}

impl Chrome {
    pub fn new(font: Option<Font>) -> Self {
        Chrome {
            pixmap: Pixmap::new(font),
            size: (0, 0),
            scale: 1.0,
            canvas_area: Rect::EMPTY,
            menu: MenuBar::default(),
            command: CommandBar::default(),
            tabs: TabBar::default(),
            tools: ToolBar::default(),
            property: ToolProperty::default(),
            palette: PaletteDock::new(),
            sheets: SheetPanel::default(),
            scroll_h: ScrollBar::new(false),
            scroll_v: ScrollBar::new(true),
            drawn_scroll: (ScrollSpan::default(), ScrollSpan::default()),
            dialog: None,
            dialog_selecting: false,
            shift: false,
            tooltip: None,
            brush_cursor: None,
            line_guide: None,
            captured: None,
            dirty: DirtyRegion::default(),
        }
    }

    /// ウィンドウの大きさか表示スケールが変わったら呼ぶ。区画を割り当て直す。
    pub fn resize(&mut self, width: u32, height: u32, scale: f32) {
        self.size = (width, height);
        self.scale = scale;
        let s = |v: f32| (v * scale).round() as i32;
        let (w, h) = (width as i32, height as i32);
        let menu_bottom = s(MENU_BAR_HEIGHT).min(h);
        let top = (menu_bottom + s(COMMAND_BAR_HEIGHT)).min(h);
        let tools_right = s(TOOL_BAR_WIDTH).min(w);
        let property_right = (tools_right + s(PROPERTY_WIDTH)).min(w);
        let right_left = (w - s(PaletteDock::WIDTH)).max(property_right);
        let palette_bottom = (top + s(PaletteDock::HEIGHT)).min(h);

        let frame = |rect: Rect| Frame { rect, scale };
        self.menu.place(frame(Rect::new(0, 0, w, menu_bottom)));
        self.command.place(frame(Rect::new(0, menu_bottom, w, top)));
        self.tools.place(frame(Rect::new(0, top, tools_right, h)));
        self.property.place(frame(Rect::new(tools_right, top, property_right, h)));
        self.palette.place(frame(Rect::new(right_left, top, w, palette_bottom)));
        self.sheets.place(frame(Rect::new(right_left, palette_bottom, w, h)));
        let tabs_bottom = (top + s(TAB_BAR_HEIGHT)).min(h);
        self.tabs.place(frame(Rect::new(property_right, top, right_left, tabs_bottom)));
        // キャンバスの右と下にスクロールバー。縦のバーは右下の角まで伸ばす
        let bar = s(SCROLL_BAR_WIDTH);
        let area_right = (right_left - bar).max(property_right);
        let area_bottom = (h - bar).max(tabs_bottom);
        self.scroll_v.place(frame(Rect::new(area_right, tabs_bottom, right_left, h)));
        self.scroll_h.place(frame(Rect::new(property_right, area_bottom, area_right, h)));
        self.canvas_area = Rect::new(property_right, tabs_bottom, area_right, area_bottom);
        if let Some(dialog) = &mut self.dialog {
            dialog.place(Rect::from_size(width, height), scale);
        }

        self.pixmap.resize(width, height);
        self.dirty.add(self.pixmap.bounds());
    }

    /// キャンバスを見せる中央の区画。
    pub fn canvas_area(&self) -> Rect {
        self.canvas_area
    }

    /// (x, y) がパネルの上か（キャンバスの区画の外か）。メニューやダイアログが開いている間は、どこでも UI の上とみなす。
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.is_modal() || self.menu.is_open() || {
            Rect::from_size(self.size.0, self.size.1).contains(x, y) && !self.canvas_area.contains(x, y)
        }
    }

    /// ダイアログが開いていて、ほかの操作を受け付けないか。
    pub fn is_modal(&self) -> bool {
        self.dialog.is_some()
    }

    /// 「新規」のダイアログを開く。`mode` は最初に選んでおくキャンバスモード。
    pub fn open_new_document(&mut self, mode: CanvasMode) {
        self.close_menu();
        self.open_dialog(Box::new(NewDocumentDialog::new(mode)));
    }

    /// 「画像を統合して書き出し」のダイアログを開く。`source` は作品の大きさと解像度、`choices` は前に書き出したときの設定。
    pub fn open_export(&mut self, format: ImageFormat, source: ExportSource, choices: ExportChoices) {
        self.open_dialog(Box::new(ExportDialog::new(format, source, choices)));
    }

    fn open_dialog(&mut self, mut dialog: Box<dyn Dialog>) {
        self.close_menu();
        self.close_dialog();
        dialog.place(Rect::from_size(self.size.0, self.size.1), self.scale);
        self.dirty.add(dialog.rect());
        self.dialog = Some(dialog);
    }

    fn close_dialog(&mut self) {
        self.dialog_selecting = false;
        if let Some(dialog) = self.dialog.take() {
            self.dirty.add(dialog.rect());
        }
    }

    fn close_menu(&mut self) {
        if self.menu.is_open() {
            self.dirty.add(self.menu.overlay_rect());
            self.dirty.add(self.menu.frame().rect);
            self.menu.close();
        }
    }

    /// Esc：開いているメニューかダイアログを閉じる。閉じたら true。
    pub fn cancel(&mut self) -> bool {
        let open = self.dialog.is_some() || self.menu.is_open();
        self.close_dialog();
        self.close_menu();
        open
    }

    /// ダイアログの入力欄へのキー入力。
    pub fn set_shift(&mut self, held: bool) {
        self.shift = held;
    }

    /// ホイール。ダイアログが開いていれば、ポインタの下の欄の値を上下して true を返す（キャンバスには渡さない）。
    pub fn wheel(&mut self, x: f32, y: f32, steps: i32, step: WheelStep) -> bool {
        let Some(dialog) = &mut self.dialog else { return false };
        if dialog.wheel(x, y, steps, step) {
            self.dirty.add(dialog.rect());
        }
        true
    }

    pub fn dialog_key(&mut self, key: DialogKey) {
        if let Some(dialog) = &mut self.dialog
            && dialog.key(key)
        {
            self.dirty.add(dialog.rect());
        }
    }

    /// Enter：ダイアログの内容で決める。
    pub fn confirm(&mut self) -> Option<Action> {
        let result = self.dialog.as_ref()?.confirm();
        self.finish_dialog(result)
    }

    fn finish_dialog(&mut self, result: DialogResult) -> Option<Action> {
        match result {
            DialogResult::Open(redraw) => {
                if redraw && let Some(dialog) = &self.dialog {
                    self.dirty.add(dialog.rect());
                }
                None
            }
            DialogResult::Cancel => {
                self.close_dialog();
                None
            }
            DialogResult::Done(action) => {
                self.close_dialog();
                Some(action)
            }
        }
    }

    /// メニューの一覧に触る操作の前後で、一覧が占める範囲を描き直す。
    fn with_menu<T>(&mut self, operate: impl FnOnce(&mut MenuBar) -> (T, bool)) -> T {
        let before = self.menu.overlay_rect();
        let (result, changed) = operate(&mut self.menu);
        if changed {
            let after = self.menu.overlay_rect();
            self.dirty.add(before);
            self.dirty.add(after);
            self.dirty.add(self.menu.frame().rect);
        }
        result
    }

    pub fn is_capturing(&self) -> bool {
        self.captured.is_some() || self.dialog_selecting
    }

    fn panel(&self, id: PanelId) -> &dyn Panel {
        match id {
            PanelId::Menu => &self.menu,
            PanelId::Command => &self.command,
            PanelId::Tabs => &self.tabs,
            PanelId::Tools => &self.tools,
            PanelId::Property => &self.property,
            PanelId::Palette => &self.palette,
            PanelId::Sheets => &self.sheets,
            PanelId::ScrollH => &self.scroll_h,
            PanelId::ScrollV => &self.scroll_v,
        }
    }

    fn panel_mut(&mut self, id: PanelId) -> &mut dyn Panel {
        match id {
            PanelId::Menu => &mut self.menu,
            PanelId::Command => &mut self.command,
            PanelId::Tabs => &mut self.tabs,
            PanelId::Tools => &mut self.tools,
            PanelId::Property => &mut self.property,
            PanelId::Palette => &mut self.palette,
            PanelId::Sheets => &mut self.sheets,
            PanelId::ScrollH => &mut self.scroll_h,
            PanelId::ScrollV => &mut self.scroll_v,
        }
    }

    fn panel_at(&self, x: f32, y: f32) -> Option<PanelId> {
        PANEL_IDS.into_iter().find(|&id| self.panel(id).frame().rect.contains(x, y))
    }

    pub fn invalidate(&mut self, id: PanelId) {
        self.dirty.add(self.panel(id).frame().rect);
    }

    /// 混色パネルの板（保存・読み込みで中身を出し入れする）。
    pub fn palette(&self) -> &PalettePanel {
        self.palette.board()
    }

    /// 前に呼んでから、パレットの絵の具が変わったか。
    pub fn take_palette_edited(&mut self) -> bool {
        self.palette.board_mut().take_edited()
    }

    pub fn palette_mut(&mut self) -> &mut PalettePanel {
        self.invalidate(PanelId::Palette);
        self.palette.board_mut()
    }

    pub fn invalidate_all(&mut self) {
        self.dirty.add(self.pixmap.bounds());
    }

    fn handle(&mut self, id: PanelId, response: Response) -> Option<Action> {
        if response.redraw {
            self.invalidate(id);
        }
        response.action
    }

    /// 説明の文字を (x, y) の右下に出す。画面からはみ出さないよう寄せる。
    fn show_tooltip(&mut self, text: Option<String>, x: f32, y: f32) -> bool {
        if text.as_deref() == self.tooltip.as_ref().map(|t| t.text.as_str()) {
            return false;
        }
        self.hide_tooltip();
        let Some(text) = text else { return true };
        let px = TOOLTIP_TEXT * self.scale;
        // 字幅を測らずに、全角は 1 字、半角は 0.6 字として幅を見積もる
        let chars: f32 = text.chars().map(|c| if c.is_ascii() { 0.6 } else { 1.0 }).sum();
        let (w, h) = ((chars * px + 12.0 * self.scale) as i32, (px * 1.8) as i32);
        let (sw, sh) = (self.size.0 as i32, self.size.1 as i32);
        let x0 = ((x + 12.0 * self.scale) as i32).min(sw - w).max(0);
        let below = (y + 20.0 * self.scale) as i32;
        let y0 = if below + h > sh { (y as i32 - h - 4).max(0) } else { below };
        let rect = Rect::new(x0, y0, x0 + w, y0 + h);
        self.dirty.add(rect);
        self.tooltip = Some(Tooltip { text, rect });
        true
    }

    /// キャンバスの上のペン先の輪郭を、画面の (x, y) に半径 `radius`（画面のピクセル）で出す。`None` で消す。
    pub fn set_brush_cursor(&mut self, cursor: Option<BrushCursor>) {
        if cursor == self.brush_cursor {
            return;
        }
        if let Some(old) = self.brush_cursor.take() {
            self.dirty.add(old.bounds());
        }
        if let Some(new) = cursor {
            self.dirty.add(new.bounds());
        }
        self.brush_cursor = cursor;
    }

    /// 直線の下書きを出す。`None` で消す。
    pub fn set_line_guide(&mut self, guide: Option<LineGuide>) {
        if guide == self.line_guide {
            return;
        }
        if let Some(old) = self.line_guide.take() {
            for piece in old.pieces() {
                self.dirty.add(piece);
            }
        }
        if let Some(new) = guide {
            for piece in new.pieces() {
                self.dirty.add(piece);
            }
        }
        self.line_guide = guide;
    }

    fn hide_tooltip(&mut self) {
        if let Some(tooltip) = self.tooltip.take() {
            self.dirty.add(tooltip.rect);
        }
    }

    /// ボタンを押していない間のポインタの移動。描き直しが要るなら true。
    pub fn hover(&mut self, x: f32, y: f32, state: &UiState) -> bool {
        if self.is_modal() {
            return false;
        }
        let text = if self.menu.is_open() {
            None
        } else {
            self.panel_at(x, y).and_then(|id| self.panel(id).tooltip(x, y, state))
        };
        let tooltip_changed = self.show_tooltip(text, x, y);
        self.hover_panels(x, y) || tooltip_changed
    }

    fn hover_panels(&mut self, x: f32, y: f32) -> bool {
        if self.menu.is_open() {
            return self.with_menu(|menu| {
                let changed = menu.hover_open(x, y);
                (changed, changed)
            });
        }
        let mut changed = false;
        for id in PANEL_IDS {
            if self.panel_mut(id).hover(x, y) {
                self.invalidate(id);
                changed = true;
            }
        }
        changed
    }

    /// パネルの上で押したら、そのパネルが離すまで操作を受け取る。
    pub fn press(&mut self, x: f32, y: f32, kind: PressKind, state: &UiState) -> Option<Action> {
        self.hide_tooltip();
        if let Some(dialog) = &mut self.dialog {
            let result = dialog.press(x, y, self.shift);
            self.dialog_selecting = dialog.is_selecting();
            return self.finish_dialog(result);
        }
        if self.menu.is_open() {
            return self.with_menu(|menu| {
                let response = menu.press_open(x, y);
                (response.action, response.changed)
            });
        }
        let id = self.panel_at(x, y)?;
        if id == PanelId::Menu {
            return self.with_menu(|menu| {
                let response = menu.press(x, y, kind, state);
                (response.action, response.redraw)
            });
        }
        self.captured = Some(id);
        let response = self.panel_mut(id).press(x, y, kind, state);
        self.handle(id, response)
    }

    pub fn drag(&mut self, x: f32, y: f32, state: &UiState) -> Option<Action> {
        if self.dialog_selecting
            && let Some(dialog) = &mut self.dialog
        {
            if dialog.drag(x) {
                self.dirty.add(dialog.rect());
            }
            return None;
        }
        let id = self.captured?;
        let response = self.panel_mut(id).drag(x, y, state);
        self.handle(id, response)
    }

    pub fn release(&mut self) -> Option<Action> {
        if self.dialog_selecting {
            self.dialog_selecting = false;
            if let Some(dialog) = &mut self.dialog {
                dialog.release();
            }
            return None;
        }
        let id = self.captured.take()?;
        let response = self.panel_mut(id).release();
        self.handle(id, response)
    }

    /// 描き直しが必要な所を描き、描いた矩形を返す（何もなければ空）。
    pub fn render(&mut self, state: &UiState) -> Vec<Rect> {
        // 表示位置はいろいろな操作で変わるので、描くときに比べてスクロールバーを描き直す
        if self.drawn_scroll != (state.scroll_h, state.scroll_v) {
            self.drawn_scroll = (state.scroll_h, state.scroll_v);
            self.invalidate(PanelId::ScrollH);
            self.invalidate(PanelId::ScrollV);
        }
        let bounds = Rect::from_size(self.size.0, self.size.1);
        let rects: Vec<Rect> =
            self.dirty.take().iter().map(|r| r.intersect(&bounds)).filter(|r| !r.is_empty()).collect();
        if rects.is_empty() {
            return rects;
        }
        // パネルを借りたまま描けるよう、画像はいったん取り出す
        let mut pixmap = std::mem::replace(&mut self.pixmap, Pixmap::new(None));
        for &dirty in &rects {
            self.render_area(state, &mut pixmap, dirty);
        }
        pixmap.set_clip(pixmap.bounds());
        self.pixmap = pixmap;
        rects
    }

    /// `dirty` の中だけを描く。
    fn render_area(&self, state: &UiState, pixmap: &mut Pixmap, dirty: Rect) {
        pixmap.set_clip(dirty);
        // キャンバスの区画は透明にしておき、下のキャンバスを見せる
        pixmap.fill_rect(self.canvas_area, Rgba8::default());
        for id in PANEL_IDS {
            let area = self.panel(id).frame().rect.intersect(&dirty);
            if area.is_empty() {
                continue;
            }
            pixmap.set_clip(area);
            self.panel(id).render(state, pixmap);
        }
        // メニューの一覧とダイアログは、パネルやキャンバスの上に重ねる
        let menu_area = self.menu.overlay_rect().intersect(&dirty);
        if !menu_area.is_empty() {
            pixmap.set_clip(menu_area);
            self.menu.render_overlay(pixmap);
        }
        if let Some(guide) = &self.line_guide {
            let area = dirty.intersect(&self.canvas_area);
            if !area.is_empty() {
                pixmap.set_clip(area);
                guide.render(pixmap, area);
            }
        }
        if let Some(cursor) = &self.brush_cursor {
            // パネルの上には出さない
            pixmap.set_clip(cursor.bounds().intersect(&dirty).intersect(&self.canvas_area));
            cursor.render(pixmap, self.scale);
        }
        if let Some(dialog) = &self.dialog {
            pixmap.set_clip(dialog.rect().intersect(&dirty));
            dialog.render(pixmap);
        }
        if let Some(tooltip) = &self.tooltip {
            pixmap.set_clip(tooltip.rect.intersect(&dirty));
            pixmap.fill_rect(tooltip.rect, theme::colors().tooltip);
            pixmap.frame(tooltip.rect, theme::colors().border);
            let inner = Rect::new(
                tooltip.rect.x0 + (6.0 * self.scale) as i32,
                tooltip.rect.y0,
                tooltip.rect.x1,
                tooltip.rect.y1,
            );
            pixmap.text(inner, &tooltip.text, TOOLTIP_TEXT * self.scale, theme::colors().text, pixmap::Align::Left);
        }
    }

    pub fn copy_region(&self, region: Rect) -> Vec<Rgba8> {
        self.pixmap.copy_region(region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_diagonal_guide_redraws_only_along_the_line() {
        let guide = LineGuide { from: (100.0, 100.0), to: (1900.0, 1100.0), radius: 40.0 };
        let mut dirty = DirtyRegion::default();
        for piece in guide.pieces() {
            dirty.add(piece);
        }
        let rects = dirty.take();
        let area: i64 = rects.iter().map(|r| r.width() as i64 * r.height() as i64).sum();
        let bounds = rects.iter().fold(Rect::EMPTY, |all, r| all.union(r));
        assert!(area * 4 < bounds.width() as i64 * bounds.height() as i64, "外接矩形よりずっと狭い: {area}");
        // 下書きの線はすべて、描き直す範囲の中にある
        for segment in guide.segments() {
            let piece = LineGuide::piece_bounds(&segment);
            assert!(rects.iter().any(|r| r.intersect(&piece) == piece));
        }
    }

    pub(super) fn state() -> UiState {
        UiState {
            mode: CanvasMode::Paper,
            tools: vec![
                ToolInfo { name: "インクペン".into(), icon: ToolIcon::InkPen, usable: true },
                ToolInfo { name: "鉛筆".into(), icon: ToolIcon::Pencil, usable: true },
            ],
            current_tool: 0,
            pen_name: "インクペン".into(),
            variants: Vec::new(),
            current_variant: 0,
            pen_tip_shape: None,
            straight_line: false,
            smooth_rotate: false,
            pen_size: 12.0,
            pen_color: Some(Color::BLACK),
            pen_takes_paint: true,
            pen_paint_mode: None,
            pen_paint_amount: None,
            pen_opacity: None,
            curves: PressureCurve::presets(),
            current_curve: 1,
            sheets: vec![
                SheetInfo {
                    index: 1,
                    name: "用紙".into(),
                    material: SheetMaterial::Paper,
                    visible: true,
                    active: false,
                },
                SheetInfo {
                    index: 2,
                    name: "透明フィルム 1".into(),
                    material: SheetMaterial::ClearFilm,
                    visible: true,
                    active: true,
                },
            ],
            tabs: vec![
                TabInfo { name: "無題".into(), modified: false },
                TabInfo { name: "花.plim".into(), modified: true },
            ],
            current_tab: 1,
            scroll_h: ScrollSpan { start: 0.3, size: 0.4 },
            scroll_v: ScrollSpan { start: 0.0, size: 1.0 },
        }
    }

    #[test]
    fn brush_cursor_is_drawn_on_the_canvas_and_cleared_when_hidden() {
        let mut chrome = Chrome::new(None);
        chrome.resize(1440, 900, 1.0);
        let st = state();
        chrome.render(&st);
        let area = chrome.canvas_area();
        let (cx, cy) = ((area.x0 + area.x1) as f32 * 0.5, (area.y0 + area.y1) as f32 * 0.5);
        chrome.set_brush_cursor(Some(BrushCursor { x: cx, y: cy, radius: 20.0 }));
        chrome.render(&st);
        let ring = chrome.copy_region(Rect::new(cx as i32 + 20, cy as i32, cx as i32 + 21, cy as i32 + 1))[0];
        assert!(ring.a > 0, "輪郭が描かれる: {ring:?}");
        chrome.set_brush_cursor(None);
        assert!(!chrome.render(&st).is_empty());
        let cleared = chrome.copy_region(Rect::new(cx as i32 + 20, cy as i32, cx as i32 + 21, cy as i32 + 1))[0];
        assert_eq!(cleared, Rgba8::default(), "消すとキャンバスが見える");
    }

    #[test]
    fn line_guide_is_drawn_and_cleared() {
        let mut chrome = Chrome::new(None);
        chrome.resize(1440, 900, 1.0);
        let st = state();
        chrome.render(&st);
        let area = chrome.canvas_area();
        let (x0, y) = (area.x0 as f32 + 100.0, area.y0 as f32 + 200.0);
        chrome.set_line_guide(Some(LineGuide { from: (x0, y), to: (x0 + 300.0, y), radius: 40.0 }));
        chrome.render(&st);
        let edge = |chrome: &Chrome| {
            chrome.copy_region(Rect::new(x0 as i32 + 150, y as i32 + 40, x0 as i32 + 151, y as i32 + 41))[0]
        };
        assert!(edge(&chrome).a > 0, "筆の幅の縁が出る");
        chrome.set_line_guide(None);
        chrome.render(&st);
        assert_eq!(edge(&chrome), Rgba8::default());
    }

    #[test]
    fn panels_surround_the_canvas_without_overlap() {
        let mut chrome = Chrome::new(None);
        chrome.resize(1440, 900, 1.0);
        let area = chrome.canvas_area();
        assert!(area.width() > 600 && area.height() > 700, "{area:?}");
        for id in PANEL_IDS {
            let rect = chrome.panel(id).frame().rect;
            assert!(!rect.is_empty(), "{id:?}");
            assert!(rect.intersect(&area).is_empty(), "{id:?}");
            for other in PANEL_IDS.into_iter().filter(|&o| o != id) {
                assert!(rect.intersect(&chrome.panel(other).frame().rect).is_empty(), "{id:?} {other:?}");
            }
        }
        assert!(chrome.contains(5.0, 5.0));
        assert!(!chrome.contains(area.x0 as f32 + 10.0, area.y0 as f32 + 10.0));
    }

    #[test]
    fn small_window_does_not_panic() {
        let mut chrome = Chrome::new(None);
        chrome.resize(200, 100, 2.0);
        chrome.render(&state());
    }

    #[test]
    fn render_leaves_canvas_area_transparent() {
        let mut chrome = Chrome::new(None);
        chrome.resize(1280, 800, 1.0);
        let dirty = chrome.render(&state());
        assert_eq!(dirty, vec![Rect::from_size(1280, 800)]);
        let area = chrome.canvas_area();
        assert_eq!(chrome.pixmap.pixel(area.x0 + 5, area.y0 + 5).a, 0);
        assert_eq!(chrome.pixmap.pixel(2, 2).a, 255);
        assert!(chrome.render(&state()).is_empty(), "変更がなければ描き直さない");
    }

    #[test]
    fn dialog_takes_every_press_until_closed() {
        let mut chrome = Chrome::new(None);
        chrome.resize(1280, 800, 1.0);
        chrome.render(&state());
        chrome.open_new_document(CanvasMode::Paper);
        let area = chrome.canvas_area();
        assert!(chrome.contains(area.x0 as f32 + 5.0, area.y0 as f32 + 5.0));
        // ツールを押してもツールは変わらない
        let tools = chrome.tools.frame();
        let r = tools::ToolBar::button_rect(&tools, 1);
        assert_eq!(chrome.press((r.x0 + 2) as f32, (r.y0 + 2) as f32, PressKind::Primary, &state()), None);
        assert!(chrome.is_modal());
        assert!(matches!(chrome.confirm(), Some(Action::NewDocument { mode: CanvasMode::Paper, .. })));
        assert!(!chrome.is_modal());
        assert!(!chrome.render(&state()).is_empty(), "ダイアログを閉じたら、その跡を描き直す");
    }

    #[test]
    fn open_menu_is_drawn_over_the_canvas() {
        let mut chrome = Chrome::new(None);
        chrome.resize(1280, 800, 1.0);
        chrome.render(&state());
        assert_eq!(chrome.press(30.0, 10.0, PressKind::Primary, &state()), None);
        let overlay = chrome.menu.overlay_rect();
        assert!(!overlay.is_empty());
        chrome.render(&state());
        assert_eq!(chrome.pixmap.pixel(overlay.x0 + 3, overlay.y1 - 3).a, 255);
        assert!(chrome.cancel());
        chrome.render(&state());
        let area = chrome.canvas_area();
        assert_eq!(chrome.pixmap.pixel(area.x0 + 5, overlay.y1 - 3).a, 0, "閉じたらキャンバスが見える");
    }

    #[test]
    fn hovering_a_button_shows_its_name_and_pressing_hides_it() {
        let mut chrome = Chrome::new(None);
        chrome.resize(1280, 800, 1.0);
        chrome.render(&state());
        let tools = chrome.tools.frame();
        let r = tools::ToolBar::button_rect(&tools, 1);
        let (x, y) = ((r.x0 + 5) as f32, (r.y0 + 5) as f32);
        assert!(chrome.hover(x, y, &state()));
        assert_eq!(chrome.tooltip.as_ref().map(|t| t.text.as_str()), Some("鉛筆"));
        assert!(!chrome.hover(x + 1.0, y, &state()), "同じボタンの上なら変わらない");
        chrome.press(x, y, PressKind::Primary, &state());
        assert!(chrome.tooltip.is_none());
        chrome.release();
    }

    #[test]
    fn pressing_a_tool_selects_the_pen() {
        let mut chrome = Chrome::new(None);
        chrome.resize(1280, 800, 1.0);
        let tools = chrome.tools.frame();
        let center = |r: Rect| ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5);
        let (x, y) = center(tools::ToolBar::button_rect(&tools, 1));
        assert_eq!(chrome.press(x, y, PressKind::Primary, &state()), Some(Action::SelectTool(1)));
        assert_eq!(chrome.release(), None);
        assert!(!chrome.is_capturing());
    }
}
