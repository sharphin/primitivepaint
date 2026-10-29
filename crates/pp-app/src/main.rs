// リリースビルドではコンソールウィンドウを出さない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod files;
mod renderer;
mod settings;
mod ui;
mod view;
mod work;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pp_core::{
    CanvasMode, Document, Eraser, InkPen, InputSample, KneadedEraser, OilPen, Pen, Pencil, PressureCurve, Rect,
    SheetMaterial, StraightLine, StrokeEngine, Watercolor,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Force, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use files::{FileFilter, IMPORT_FILTER, UnsavedChoice, WORK_FILTER};
use pp_file::{CopyFormat, ExportSettings, ImageFormat, OutputSize};
use renderer::Renderer;
use settings::Settings;
use theme::UiTheme;
use ui::{
    Action, BrushCursor, Chrome, DialogKey, ExportChoices, ExportSource, FieldKey, Font, LineGuide, PalettePanel,
    PanelId, PressKind, SheetInfo, TabInfo, ToolIcon, ToolInfo, UiState, WheelStep, ZOOM_STEP, theme,
};
use view::{PivotDrag, ScrollSpan, ViewTransform};
use work::Work;

const CANVAS_WIDTH: u32 = 3000;
const CANVAS_HEIGHT: u32 = 2000;

// `App::pens` の並び順。P・B・E は同じグループの中で順に切り替わる。
const PEN_INK: usize = 0;
const PEN_PENCIL: usize = 1;
const PEN_OIL: usize = 2;
const PEN_WATERCOLOR: usize = 3;
const PEN_ERASER: usize = 4;
const PEN_KNEADED_ERASER: usize = 5;
const PEN_GROUP: [usize; 2] = [PEN_INK, PEN_PENCIL];
const BRUSH_GROUP: [usize; 2] = [PEN_OIL, PEN_WATERCOLOR];
const ERASER_GROUP: [usize; 2] = [PEN_ERASER, PEN_KNEADED_ERASER];
/// ツール列の欄。1 つの欄に複数のペンがあるときは、ツールプロパティで選ぶ（消しゴムの消し方など）。
const TOOL_SLOTS: [&[usize]; 5] = [&[PEN_INK], &[PEN_PENCIL], &[PEN_OIL], &[PEN_WATERCOLOR], &ERASER_GROUP];
/// ペンの絵柄（`App::pens` と同じ並び）。
const PEN_ICONS: [ToolIcon; 6] = [
    ToolIcon::InkPen,
    ToolIcon::Pencil,
    ToolIcon::OilBrush,
    ToolIcon::WatercolorBrush,
    ToolIcon::Eraser,
    ToolIcon::KneadedEraser,
];
const PEN_SIZE_MAX: f32 = 1000.0;
/// 起動したときのブラシサイズ。描く道具どうし、消す道具どうしで共通にする。
const DRAW_SIZE: f32 = 12.0;
const ERASE_SIZE: f32 = 30.0;

/// 紙モードでキー操作したときの回転の刻み。
const PAPER_ROTATION_STEP: f32 = std::f32::consts::PI / 12.0;

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// タブで開いている作品。いつも 1 つ以上ある。
    works: Vec<Work>,
    /// 表に出ている作品（`works` の番号）。
    current: usize,
    /// これまでに作った無題の作品の数（「無題 2」の番号に使う）。
    untitled_count: usize,
    /// 数字キー（並び順がそのままキー番号）か、CLIP STUDIO と同じ P / B / E で切り替える。
    pens: Vec<Box<dyn Pen>>,
    current_pen: usize,
    /// ツール列の欄ごとに、最後に選んだペン（`TOOL_SLOTS` と同じ並び）。
    chosen: [usize; TOOL_SLOTS.len()],
    /// 最後に書き出したときの設定（大きさの決め方・倍率・品質など）。次の書き出しの初期値にする。
    export_choices: ExportChoices,
    /// ツールプロパティか `K` で切り替える筆圧カーブ。
    curves: Vec<PressureCurve>,
    current_curve: usize,
    /// 次の起動にも残す設定（配色など）。
    settings: Settings,
    stroke: StrokeEngine,
    /// 直線で引いている途中の線。
    line: Option<StraightLine>,
    /// 直線で引くか（ツール列の「直線」）。ペンを持ち替えても変わらない。
    straight_line: bool,
    modifiers: ModifiersState,
    /// 次のフレームで GPU に送る範囲。
    pending_upload: Rect,
    cursor: (f64, f64),
    /// ペン（WM_POINTER 経由の Touch イベント）で描いている間はマウスイベントを無視する。
    pen_down: bool,
    /// 筆圧付きのイベントを一度でも受け取ったか。受け取った後は筆圧なし = 0 とみなす。
    pressure_seen: bool,
    /// Space を押している間は、ドラッグで表示を動かす（CLIP STUDIO と同じく Shift+Space で回転、
    /// Ctrl+Space で押した所を拡大、Ctrl+Alt+Space で縮小）。
    space_held: bool,
    panning: bool,
    /// R を押している間は、左ドラッグで表示を回転する。
    rotate_held: bool,
    rotating: Option<RotateDrag>,
    /// 滑らかな回転（コマンドバー）が ON か。ON の間は描かず、押した所を軸に回す。紙モードだけ。
    smooth_rotate: bool,
    /// 滑らかな回転で、押したまま回している途中。
    pivoting: Option<PivotDrag>,
    /// OS のカーソルを隠して、ペン先の輪郭を出しているか。
    cursor_hidden: bool,
    /// キャンバスのまわりのパネル（ツール・パレット・紙の一覧など）。
    chrome: Chrome,
}

/// 回転ドラッグを始めたときの状態。
struct RotateDrag {
    /// 画面中心から見たポインタの角度。
    pointer_angle: f32,
    rotation: f32,
}

impl App {
    fn new(mode: CanvasMode) -> Self {
        let settings = Settings::load();
        // パネルを描く前に配色を決めておく
        theme::set_current(settings.theme);
        let doc = Document::new(CANVAS_WIDTH, CANVAS_HEIGHT, mode);
        let pending_upload = doc.bounds();
        let first = Work::new(doc, PalettePanel::blank_palette(), 1);
        let mut app = App {
            window: None,
            renderer: None,
            works: vec![first],
            current: 0,
            untitled_count: 1,
            pens: vec![
                Box::new(InkPen::default()),
                Box::new(Pencil::default()),
                Box::new(OilPen::default()),
                Box::new(Watercolor::default()),
                Box::new(Eraser::default()),
                Box::new(KneadedEraser::default()),
            ],
            current_pen: 0,
            chosen: TOOL_SLOTS.map(|slot| slot[0]),
            export_choices: ExportChoices::default(),
            curves: PressureCurve::presets(),
            current_curve: 1,
            settings,
            stroke: StrokeEngine::new(),
            line: None,
            straight_line: false,
            modifiers: ModifiersState::empty(),
            pending_upload,
            cursor: (0.0, 0.0),
            pen_down: false,
            pressure_seen: false,
            space_held: false,
            panning: false,
            rotate_held: false,
            rotating: None,
            smooth_rotate: false,
            pivoting: None,
            cursor_hidden: false,
            chrome: Chrome::new(Font::load_system()),
        };
        for pen in &mut app.pens {
            pen.set_size(if pen.erases() { ERASE_SIZE } else { DRAW_SIZE });
        }
        app.show_work(0, None);
        app
    }

    fn work(&self) -> &Work {
        &self.works[self.current]
    }

    fn work_mut(&mut self) -> &mut Work {
        &mut self.works[self.current]
    }

    /// タイトルに作品の名前を出す。保存していない変更があれば「*」を付ける。
    fn update_title(&self) {
        if let Some(window) = &self.window {
            let work = self.work();
            let mark = if work.is_modified() { " *" } else { "" };
            window.set_title(&format!("{}{mark} - primitivepaint（{}）", work.name(), work.doc.mode.name()));
        }
    }

    fn mark_modified(&mut self) {
        if !self.work().is_modified() {
            self.work_mut().mark_modified();
            self.chrome.invalidate(PanelId::Tabs);
            self.update_title();
        }
    }

    /// 新しい作品をタブで開く。
    fn new_work(&mut self, mode: CanvasMode, width: u32, height: u32) {
        self.untitled_count += 1;
        let doc = Document::new(width, height, mode);
        let work = Work::new(doc, PalettePanel::blank_palette(), self.untitled_count);
        self.add_work(work);
    }

    /// 作品をタブの最後に足して表に出す。
    fn add_work(&mut self, work: Work) {
        self.works.push(work);
        self.show_work(self.works.len() - 1, Some(self.current));
        self.fit_view();
    }

    fn switch_to(&mut self, index: usize) {
        if index < self.works.len() && index != self.current && !self.drawing() {
            self.show_work(index, Some(self.current));
        }
    }

    /// `index` 番の作品を表に出す。`outgoing` は今まで表に出ていた作品で、パレットをそこへしまう
    /// （閉じた作品なら `None` で、パレットは捨てる）。
    fn show_work(&mut self, index: usize, outgoing: Option<usize>) {
        if self.chrome.take_palette_edited()
            && let Some(previous) = outgoing
        {
            self.works[previous].mark_modified();
        }
        let incoming = self.works[index].unpark_palette().unwrap_or_else(PalettePanel::blank_palette);
        let shown = match self.chrome.palette_mut().replace_palette(incoming) {
            Ok(shown) => shown,
            // 大きさの合わないパレットは開くときに除いているので、ここには来ない
            Err(rejected) => rejected,
        };
        if let Some(previous) = outgoing.filter(|&p| p != index) {
            self.works[previous].park_palette(shown);
        }
        self.current = index;
        if !self.pens[self.current_pen].usable_on(self.work().doc.mode) {
            self.current_pen = PEN_INK;
        }
        let size = (self.work().doc.width, self.work().doc.height);
        if let Some(renderer) = &mut self.renderer {
            renderer.set_canvas_size(size);
        }
        self.pending_upload = self.work().doc.bounds();
        self.refresh_ui();
        self.update_title();
    }

    /// `index` 番のタブを閉じる。保存していない変更があれば聞く。閉じたら true。
    fn close_work(&mut self, index: usize) -> bool {
        if index >= self.works.len() || self.drawing() {
            return false;
        }
        // どの作品のことか分かるよう、聞く前に表に出す
        self.switch_to(index);
        if !self.confirm_discard() {
            return false;
        }
        self.works.remove(index);
        if self.works.is_empty() {
            // 作品が 1 つもない状態は作らず、白紙を 1 枚用意する
            self.untitled_count = 1;
            let doc = Document::new(CANVAS_WIDTH, CANVAS_HEIGHT, CanvasMode::Paper);
            let blank = Work::new(doc, PalettePanel::blank_palette(), self.untitled_count);
            self.works.push(blank);
        }
        self.current = index.min(self.works.len() - 1);
        self.show_work(self.current, None);
        self.fit_view();
        true
    }

    /// 上書き保存。まだ保存先がなければ、名前を付けて保存する。成功したら true。
    fn save(&mut self) -> bool {
        match self.work().file().map(Path::to_path_buf) {
            Some(path) => self.save_to(path),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        let name = format!("{}.{}", self.work().stem(), WORK_FILTER.extension());
        match files::pick_save("別名で保存", &WORK_FILTER, &name) {
            Some(path) => self.save_to(path.with_extension(WORK_FILTER.extension())),
            None => false,
        }
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        if self.drawing() {
            return false;
        }
        if let Err(err) = pp_file::plim::save_to_path(&path, &self.work().doc, self.chrome.palette().palette()) {
            files::show_error(&format!("保存できませんでした。\n{}\n{err}", path.display()));
            return false;
        }
        self.chrome.take_palette_edited();
        self.work_mut().mark_saved(path);
        self.chrome.invalidate(PanelId::Tabs);
        self.update_title();
        true
    }

    /// 見た目どおりに合成した画像を書き出す。作品の保存先や「保存済み」の状態は変えない。
    /// 書き出しの設定（倍率）を開く。決まったら `export` で書き出す。
    fn show_export(&mut self, format: ImageFormat) {
        if self.drawing() {
            return;
        }
        let doc = &self.work().doc;
        let source = ExportSource { width: doc.width, height: doc.height, dpi: doc.dpi };
        self.chrome.open_export(format, source, self.export_choices);
        self.request_redraw();
    }

    fn export(&mut self, format: ImageFormat, settings: ExportSettings) {
        if self.drawing() {
            return;
        }
        let filter = FileFilter { name: format.name(), extensions: format.extensions() };
        let name = format!("{}.{}", self.work().stem(), format.extension());
        let Some(picked) = files::pick_save("画像を統合して書き出し", &filter, &name) else {
            return;
        };
        let path = picked.with_extension(format.extension());
        if let Err(err) = format.export_to_path(&path, &self.work().doc, settings) {
            files::show_error(&format!("画像を書き出せませんでした。\n{}\n{err}", path.display()));
        }
    }

    /// ほかのソフトの形式で複製を保存する。書き出しと同じく、作品の保存先や「保存済み」の状態は変えない。
    fn save_copy(&mut self, format: CopyFormat) {
        if self.drawing() {
            return;
        }
        let filter = FileFilter { name: format.name(), extensions: format.extensions() };
        let name = format!("{}.{}", self.work().stem(), format.extension());
        let Some(picked) = files::pick_save("複製を保存", &filter, &name) else {
            return;
        };
        let path = picked.with_extension(format.extension());
        if let Err(err) = format.save_to_path(&path, &self.work().doc) {
            files::show_error(&format!("複製を保存できませんでした。\n{}\n{err}", path.display()));
        }
    }

    /// 表に出ている作品に保存していない変更があれば、保存するか聞く。続けてよければ true。
    fn confirm_discard(&mut self) -> bool {
        if self.chrome.take_palette_edited() {
            self.work_mut().mark_modified();
        }
        if !self.work().is_modified() {
            return true;
        }
        match files::ask_unsaved(&self.work().name()) {
            UnsavedChoice::Save => self.save(),
            UnsavedChoice::Discard => true,
            UnsavedChoice::Cancel => false,
        }
    }

    /// すべてのタブについて、保存していない変更を確かめる。アプリを閉じてよければ true。
    fn confirm_quit(&mut self) -> bool {
        // 閉じる操作中にポインタの Release が届かない場合でも、
        // 描画を確定してから全タブの未保存確認を行う。
        if self.drawing() {
            self.stroke_end();
        }
        for index in 0..self.works.len() {
            self.switch_to(index);
            if !self.confirm_discard() {
                return false;
            }
        }
        true
    }

    fn open(&mut self) {
        if self.drawing() {
            return;
        }
        let Some(path) = files::pick_open(&[&WORK_FILTER, &IMPORT_FILTER]) else {
            return;
        };
        let extension = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if IMPORT_FILTER.extensions.contains(&extension.as_str()) {
            self.import(&path);
            return;
        }
        // 開いているファイルなら、そのタブに切り替えるだけ
        if let Some(index) = self.works.iter().position(|w| w.file() == Some(path.as_path())) {
            self.switch_to(index);
            return;
        }
        let project = match pp_file::plim::load_from_path(&path) {
            Ok(project) => project,
            Err(err) => {
                files::show_error(&format!("開けませんでした。\n{}\n{err}", path.display()));
                return;
            }
        };
        let blank = PalettePanel::blank_palette();
        let palette = if (project.palette.width(), project.palette.height()) == (blank.width(), blank.height()) {
            project.palette
        } else {
            files::show_error("パレットの大きさが違うため、パレットは読み込みませんでした。");
            blank
        };
        let work = Work::opened(project.document, palette, path);
        if self.chrome.take_palette_edited() {
            self.work_mut().mark_modified();
        }
        if self.work().is_blank() {
            // 何も描いていない無題のタブは、開いた作品で置き換える
            self.works[self.current] = work;
            self.show_work(self.current, None);
            self.fit_view();
        } else {
            self.add_work(work);
        }
    }

    /// ほかのソフトの形式を読み込み、新しい作品として開く。元のファイルには保存しない（紙に変換して失った情報があるため）。
    fn import(&mut self, path: &Path) {
        let imported = match pp_file::psd::load_from_path(path) {
            Ok(imported) => imported,
            Err(err) => {
                files::show_error(&format!("読み込めませんでした。\n{}\n{err}", path.display()));
                return;
            }
        };
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let work = Work::imported(imported.document, PalettePanel::blank_palette(), stem);
        if self.chrome.take_palette_edited() {
            self.work_mut().mark_modified();
        }
        if self.work().is_blank() {
            self.works[self.current] = work;
            self.show_work(self.current, None);
            self.fit_view();
        } else {
            self.add_work(work);
        }
        self.chrome.invalidate(PanelId::Tabs);
        self.update_title();
        if !imported.notes.is_empty() {
            files::show_info(&import_report(&imported.notes));
        }
    }

    /// パネルの表示に使う今の状態をまとめる。
    fn ui_state(&self) -> UiState {
        let pen = &self.pens[self.current_pen];
        let base = self.work().doc.sheets().next().map(|(i, _)| i);
        let tool_info = |index: usize| ToolInfo {
            name: self.pens[index].name().to_string(),
            icon: PEN_ICONS[index],
            usable: self.pens[index].usable_on(self.work().doc.mode),
        };
        let slot = Self::slot_of(self.current_pen);
        let variants = TOOL_SLOTS[slot];
        UiState {
            mode: self.work().doc.mode,
            tools: self.chosen.iter().map(|&index| tool_info(index)).collect(),
            current_tool: slot,
            pen_name: pen.name().to_string(),
            variants: if variants.len() > 1 {
                variants.iter().map(|&index| tool_info(index)).collect()
            } else {
                Vec::new()
            },
            pen_tip_shape: pen.tip_shape(),
            straight_line: self.straight_line,
            smooth_rotate: self.smooth_rotating(),
            current_variant: variants.iter().position(|&i| i == self.current_pen).unwrap_or(0),
            pen_size: pen.size(),
            pen_color: pen.color(),
            pen_takes_paint: pen.takes_paint(),
            pen_paint_mode: pen.paint_mode(),
            pen_paint_amount: pen.paint_amount(),
            pen_opacity: pen.opacity(),
            curves: self.curves.clone(),
            current_curve: self.current_curve,
            sheets: self
                .work()
                .doc
                .sheets()
                .map(|(index, layer)| SheetInfo {
                    index,
                    // 一番下は、紙モードでは用紙、油彩ではキャンバスそのもの
                    name: match self.work().doc.mode {
                        CanvasMode::OilCanvas if Some(index) == base => "キャンバス".to_string(),
                        _ => layer.name.clone(),
                    },
                    material: layer.material,
                    visible: layer.visible,
                    active: index == self.work().doc.active_layer,
                })
                .collect(),
            tabs: self.works.iter().map(|w| TabInfo { name: w.name(), modified: w.is_modified() }).collect(),
            current_tab: self.current,
            scroll_h: self.scroll_span(false),
            scroll_v: self.scroll_span(true),
        }
    }

    fn scroll_span(&self, vertical: bool) -> ScrollSpan {
        let doc = &self.work().doc;
        self.work().view.scroll_span((doc.width, doc.height), self.chrome.canvas_area(), vertical)
    }

    /// パネルの上で押したならパネルで処理して true を返す。
    fn press_ui(&mut self, x: f64, y: f64, kind: PressKind) -> bool {
        if !self.chrome.contains(x as f32, y as f32) {
            return false;
        }
        let state = self.ui_state();
        let action = self.chrome.press(x as f32, y as f32, kind, &state);
        self.after_ui(action);
        true
    }

    /// 左ボタン・ペン先で押したときの扱い。Alt でスポイト、Shift でパレットナイフ（パレットの上だけ）。
    /// 右ボタンはいつもスポイト。
    fn press_kind(&self) -> PressKind {
        if self.modifiers.alt_key() {
            PressKind::Eyedropper
        } else if self.modifiers.shift_key() {
            PressKind::Knife
        } else {
            PressKind::Primary
        }
    }

    fn drag_ui(&mut self, x: f64, y: f64) {
        let state = self.ui_state();
        let action = self.chrome.drag(x as f32, y as f32, &state);
        self.after_ui(action);
    }

    fn release_ui(&mut self) {
        let action = self.chrome.release();
        self.after_ui(action);
        // パレットに描いたら「保存していない」印を付ける
        if self.chrome.take_palette_edited() {
            self.mark_modified();
        }
    }

    fn show_new_document(&mut self) {
        if !self.drawing() {
            self.chrome.open_new_document(self.work().doc.mode);
            self.request_redraw();
        }
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab で、隣のタブへ移る。
    fn cycle_tab(&mut self, forward: bool) {
        let count = self.works.len();
        let next = if forward { (self.current + 1) % count } else { (self.current + count - 1) % count };
        self.switch_to(next);
    }

    fn after_ui(&mut self, action: Option<Action>) {
        if let Some(action) = action {
            self.apply_action(action);
        }
        self.request_redraw();
    }

    fn apply_action(&mut self, action: Action) {
        match action {
            Action::SelectTool(slot) => self.select_tool(slot),
            Action::SelectVariant(index) => {
                if let Some(&pen) = TOOL_SLOTS[Self::slot_of(self.current_pen)].get(index) {
                    self.select_pen(pen);
                }
            }
            Action::SetPenSize(size) => self.set_pen_size(size),
            Action::SelectCurve(index) => {
                self.current_curve = index.min(self.curves.len() - 1);
                self.chrome.invalidate(PanelId::Property);
            }
            Action::SetTheme(ui_theme) => self.set_theme(ui_theme),
            Action::SetPaletteBoard(board) => self.chrome.palette_mut().set_board(board),
            Action::PickColor(color) => {
                self.pens[self.current_pen].set_color(color);
                self.chrome.invalidate(PanelId::Property);
            }
            Action::SetTipShape(shape) => {
                self.pens[self.current_pen].set_tip_shape(shape);
                self.chrome.invalidate(PanelId::Property);
            }
            Action::ToggleSmoothRotate => {
                if !self.drawing() {
                    self.smooth_rotate = !self.smooth_rotate;
                    self.pivoting = None;
                    self.chrome.invalidate(PanelId::Command);
                    self.update_cursor();
                }
            }
            Action::ToggleStraightLine => {
                // 引いている途中では切り替えない（線の種類が途中で変わらないように）
                if !self.drawing() {
                    self.straight_line = !self.straight_line;
                    self.chrome.invalidate(PanelId::Tools);
                }
            }
            Action::SetPaintMode(mode) => {
                self.pens[self.current_pen].set_paint_mode(mode);
                self.chrome.invalidate(PanelId::Property);
            }
            Action::SetPaintAmount(amount) => {
                self.pens[self.current_pen].set_paint_amount(amount);
                self.chrome.invalidate(PanelId::Property);
            }
            Action::SetOpacity(opacity) => {
                self.pens[self.current_pen].set_opacity(opacity);
                self.chrome.invalidate(PanelId::Property);
            }
            Action::RotateStep(direction) => self.rotate_by_step(direction),
            Action::ResetRotation => self.rotate_view(0.0),
            Action::Zoom(factor) => {
                let (cx, cy) = self.screen_center();
                self.works[self.current].view.zoom_at(cx, cy, factor);
                self.request_redraw();
            }
            Action::FitView => self.fit_view(),
            Action::ActualSize => self.actual_size(),
            Action::ScrollTo { vertical, start } => {
                let area = self.chrome.canvas_area();
                let work = &mut self.works[self.current];
                let size = (work.doc.width, work.doc.height);
                work.view.scroll_to(size, area, vertical, start);
                self.request_redraw();
            }
            Action::AddSheet(material) => self.edit_sheets(|doc| doc.add_sheet(material)),
            Action::RemoveSheet => self.edit_sheets(|doc| doc.remove_active_sheet()),
            Action::MoveSheet(up) => self.edit_sheets(|doc| doc.move_active_sheet(up)),
            Action::SelectSheet(index) => self.edit_sheets(|doc| doc.select_sheet_at(index)),
            Action::FlipSheet(index) => self.edit_sheets(|doc| doc.flip_sheet(index)),
            Action::ShowNewDocument => self.show_new_document(),
            Action::NewDocument { mode, width, height } => self.new_work(mode, width, height),
            Action::Open => self.open(),
            Action::Save => {
                self.save();
            }
            Action::SaveAs => {
                self.save_as();
            }
            Action::Export(format) => self.show_export(format),
            Action::ExportImage { format, width, height, dpi, choices } => {
                self.export_choices = choices;
                let settings = ExportSettings { size: OutputSize::new(width, height), quality: choices.quality, dpi };
                self.export(format, settings);
            }
            Action::SaveCopy(format) => self.save_copy(format),
            Action::SelectTab(index) => self.switch_to(index),
            Action::CloseTab(index) => {
                self.close_work(index);
            }
            Action::CloseCurrentTab => {
                self.close_work(self.current);
            }
        }
    }

    /// キャンバス全体を中央の区画に収める。
    fn fit_view(&mut self) {
        let area = self.chrome.canvas_area();
        let mut view = ViewTransform::fit(
            (self.work().doc.width, self.work().doc.height),
            (area.width().max(1), area.height().max(1)),
        );
        view.pan[0] += area.x0 as f32;
        view.pan[1] += area.y0 as f32;
        self.works[self.current].view = view;
        self.request_redraw();
    }

    /// パネルを全部描き直す。
    fn refresh_ui(&mut self) {
        self.chrome.invalidate_all();
        // ペンやタブが変わると、ペン先の輪郭の大きさや出すかどうかも変わる
        self.update_cursor();
    }

    /// 画面中心を軸に、等倍（100%）にする。
    fn actual_size(&mut self) {
        let (cx, cy) = self.screen_center();
        let factor = 1.0 / self.work().view.zoom;
        self.works[self.current].view.zoom_at(cx, cy, factor);
        self.update_cursor();
    }

    /// Space・R を押したまま、マウスの左ボタンかペン先で押した。表示の移動・回転・拡大縮小を始めたら true。
    fn press_view(&mut self) -> bool {
        if self.rotate_held || self.space_held && self.modifiers.shift_key() {
            self.rotating =
                Some(RotateDrag { pointer_angle: self.pointer_angle(), rotation: self.work().view.rotation });
        } else if self.space_held && self.modifiers.control_key() {
            let factor = if self.modifiers.alt_key() { 1.0 / ZOOM_STEP } else { ZOOM_STEP };
            self.works[self.current].view.zoom_at(self.cursor.0 as f32, self.cursor.1 as f32, factor);
            self.update_cursor();
        } else if self.space_held {
            self.panning = true;
        } else {
            return false;
        }
        true
    }

    /// 回転・移動のドラッグ中に、ポインタが `from` から今の位置へ動いた。
    fn drag_view(&mut self, from: (f64, f64)) {
        if let Some(drag) = &self.rotating {
            let angle = drag.rotation + self.pointer_angle() - drag.pointer_angle;
            self.rotate_view(angle);
        } else if self.panning {
            self.works[self.current].view.pan[0] += (self.cursor.0 - from.0) as f32;
            self.works[self.current].view.pan[1] += (self.cursor.1 - from.1) as f32;
            self.request_redraw();
        }
    }

    /// 回転・拡大の中心にする、キャンバス区画の中央。
    fn screen_center(&self) -> (f32, f32) {
        let area = self.chrome.canvas_area();
        ((area.x0 + area.x1) as f32 * 0.5, (area.y0 + area.y1) as f32 * 0.5)
    }

    fn pointer_angle(&self) -> f32 {
        let (cx, cy) = self.screen_center();
        (self.cursor.1 as f32 - cy).atan2(self.cursor.0 as f32 - cx)
    }

    /// 画面中心を軸に回転させる。角度はキャンバスモードの刻みに丸める。
    fn rotate_view(&mut self, angle: f32) {
        let snapped = self.work().doc.mode.snap_rotation(angle);
        let (cx, cy) = self.screen_center();
        self.works[self.current].view.rotate_to(cx, cy, snapped);
        self.request_redraw();
    }

    fn rotate_by_step(&mut self, direction: f32) {
        let step = self.work().doc.mode.rotation_step().unwrap_or(PAPER_ROTATION_STEP);
        self.rotate_view(self.work().view.rotation + step * direction);
    }

    /// 滑らかな回転を使っているか。油彩キャンバスの作品を表に出している間は使わない。
    fn smooth_rotating(&self) -> bool {
        self.smooth_rotate && self.work().doc.mode.rotation_step().is_none()
    }

    /// 滑らかな回転で、キャンバスを押した。
    fn pivot_begin(&mut self, x: f64, y: f64) {
        self.pivoting = Some(PivotDrag::new(x as f32, y as f32));
    }

    /// 押したまま (x, y) へ動かした。軸のまわりでポインタが回った分だけ、表示を回す。
    fn pivot_move(&mut self, x: f64, y: f64) {
        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32);
        let rotation = self.work().view.rotation;
        let Some(drag) = &mut self.pivoting else { return };
        let Some(angle) = drag.turn(x as f32, y as f32, rotation, scale) else { return };
        let (px, py) = drag.pivot();
        self.works[self.current].view.rotate_to(px, py, angle);
        self.request_redraw();
    }

    /// ポインタの位置に合わせて、キャンバスの上では OS のカーソルを隠し、代わりにペン先の輪郭（回転中などは十字）を出す。
    fn update_cursor(&mut self) {
        let (x, y) = (self.cursor.0 as f32, self.cursor.1 as f32);
        let over_canvas = self.chrome.canvas_area().contains(x, y) && !self.chrome.contains(x, y);
        let busy =
            self.space_held || self.panning || self.rotate_held || self.rotating.is_some() || self.smooth_rotating();
        // 回転や移動の間はペン先の大きさは要らないので、十字だけを出す
        let radius = if busy { 0.0 } else { self.pens[self.current_pen].size() * 0.5 * self.work().view.zoom };
        let brush = over_canvas.then_some(BrushCursor { x, y, radius });
        self.chrome.set_brush_cursor(brush);
        let hide = brush.is_some();
        if hide != self.cursor_hidden {
            self.cursor_hidden = hide;
            if let Some(window) = &self.window {
                window.set_cursor_visible(!hide);
            }
        }
        self.request_redraw();
    }

    fn sample_at(&self, x: f64, y: f64, pressure: f32) -> InputSample {
        let (cx, cy) = self.work().view.screen_to_canvas(x, y);
        InputSample { x: cx, y: cy, pressure }
    }

    fn stroke_begin(&mut self, s: InputSample) {
        let doc = &mut self.works[self.current].doc;
        let clip = doc.bounds();
        let layer = doc.active_layer_mut();
        let pen = self.pens[self.current_pen].as_mut();
        if self.straight_line {
            // 直線は離したときに描く。それまでは下書きの線だけを出す
            self.line = Some(StraightLine::new(s));
            self.update_line_guide();
            return;
        }
        let dirty = self.stroke.begin(layer, pen, s, clip);
        self.mark_dirty(dirty);
    }

    /// 引いている直線の下書きを、画面の位置に合わせて出す。
    fn update_line_guide(&mut self) {
        let Some(line) = self.line else { return };
        let view = &self.work().view;
        let (start, end) = (line.start(), line.end());
        let guide = LineGuide {
            from: view.canvas_to_screen(start.x, start.y),
            to: view.canvas_to_screen(end.x, end.y),
            // 描く太さと同じく、いままでで一番強い筆圧での大きさにする
            radius: self.pens[self.current_pen].size() * line.pressure() * 0.5 * view.zoom,
        };
        self.chrome.set_line_guide(Some(guide));
        self.request_redraw();
    }

    /// 線を引いている途中か（手描きでも直線でも）。
    fn drawing(&self) -> bool {
        self.stroke.is_active() || self.line.is_some()
    }

    fn stroke_extend(&mut self, s: InputSample) {
        if !self.drawing() {
            return;
        }
        let doc = &mut self.works[self.current].doc;
        let clip = doc.bounds();
        let layer = doc.active_layer_mut();
        let pen = self.pens[self.current_pen].as_mut();
        if let Some(line) = &mut self.line {
            line.set_end(s);
            self.update_line_guide();
            return;
        }
        let dirty = self.stroke.extend(layer, pen, s, clip);
        self.mark_dirty(dirty);
    }

    fn stroke_end(&mut self) {
        let drew = self.drawing();
        self.stroke.end();
        if let Some(line) = self.line.take() {
            let doc = &mut self.works[self.current].doc;
            let clip = doc.bounds();
            let dirty = line.draw(doc.active_layer_mut(), self.pens[self.current_pen].as_mut(), clip);
            self.mark_dirty(dirty);
            self.chrome.set_line_guide(None);
        }
        if drew {
            self.mark_modified();
            self.request_redraw();
        }
    }

    /// 紙の追加・削除・入れ替え・めくりを行い、変わったら全体を描き直す。描いている途中は触らない。
    fn edit_sheets(&mut self, edit: impl FnOnce(&mut Document) -> bool) {
        if self.drawing() || !edit(&mut self.works[self.current].doc) {
            return;
        }
        self.mark_modified();
        self.mark_dirty(self.work().doc.bounds());
        self.chrome.invalidate(PanelId::Sheets);
        self.request_redraw();
    }

    fn resize_pen(&mut self, factor: f32) {
        self.set_pen_size(self.pens[self.current_pen].size() * factor);
        self.request_redraw();
    }

    /// ブラシサイズを変える。描く道具はすべて同じ大きさで、消す道具（消しゴムの仲間）はそれとは別に共通。
    fn set_pen_size(&mut self, size: f32) {
        let clamped = size.clamp(1.0, PEN_SIZE_MAX);
        let erases = self.pens[self.current_pen].erases();
        for pen in self.pens.iter_mut().filter(|pen| pen.erases() == erases) {
            pen.set_size(clamped);
        }
        self.chrome.invalidate(PanelId::Property);
        self.update_cursor();
    }

    /// 配色を切り替えて、次の起動のために設定へ残す。
    fn set_theme(&mut self, ui_theme: UiTheme) {
        if self.settings.theme == ui_theme {
            return;
        }
        self.settings.theme = ui_theme;
        theme::set_current(ui_theme);
        self.chrome.invalidate_all();
        if let Err(err) = self.settings.save() {
            log::warn!("設定を保存できません: {err}");
        }
    }

    fn next_curve(&mut self) {
        self.current_curve = (self.current_curve + 1) % self.curves.len();
        self.chrome.invalidate(PanelId::Property);
        self.request_redraw();
    }

    /// グループ内の次の（このモードで使える）ペンに持ち替える。グループ外からなら先頭から。
    fn cycle_group(&mut self, group: &[usize]) {
        let start = group.iter().position(|&i| i == self.current_pen).map_or(0, |p| p + 1);
        let next = (0..group.len())
            .map(|offset| group[(start + offset) % group.len()])
            .find(|&i| self.pens[i].usable_on(self.work().doc.mode));
        if let Some(index) = next {
            self.select_pen(index);
        }
    }

    fn select_pen(&mut self, index: usize) {
        let usable = self.pens.get(index).is_some_and(|pen| pen.usable_on(self.work().doc.mode));
        if usable && !self.drawing() {
            self.current_pen = index;
            self.chosen[Self::slot_of(index)] = index;
            self.refresh_ui();
        }
    }

    /// ツール列の `slot` 番の欄を選ぶ。その欄で最後に使ったペンに持ち替える。
    fn select_tool(&mut self, slot: usize) {
        if let Some(&index) = self.chosen.get(slot) {
            self.select_pen(index);
        }
    }

    /// `pen` が入っているツール列の欄。
    fn slot_of(pen: usize) -> usize {
        TOOL_SLOTS.iter().position(|slot| slot.contains(&pen)).unwrap_or(0)
    }

    fn mark_dirty(&mut self, r: Rect) {
        if r.is_empty() {
            return;
        }
        self.pending_upload = self.pending_upload.union(&r);
        self.request_redraw();
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

/// 読み込んで変えたことの一覧。長くなりすぎないよう、多いときは残りの数だけ出す。
fn import_report(notes: &[String]) -> String {
    const SHOWN: usize = 15;
    let mut text = String::from(
        "primitivepaint の紙に合わせて、次のように読み込みました。\n保存すると primitivepaint の形式（.plim）になり、元のファイルは変わりません。\n",
    );
    for note in notes.iter().take(SHOWN) {
        text.push_str("\n・");
        text.push_str(note);
    }
    if notes.len() > SHOWN {
        text.push_str(&format!("\n…ほか {} 件", notes.len() - SHOWN));
    }
    text
}

/// winit は筆圧 0 を `None` で渡すため、筆圧対応デバイスでは `None` を 0 として扱う。
/// 筆圧を一度も報告しないデバイス（指タッチなど）は 1.0 で描く。
fn pressure_of(force: Option<Force>, pressure_seen: bool) -> f32 {
    match force {
        Some(f) => f.normalized() as f32,
        None if pressure_seen => 0.0,
        None => 1.0,
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("primitivepaint")
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("ウィンドウを作成できません"));
        let renderer = Renderer::new(window.clone(), (self.work().doc.width, self.work().doc.height));
        let (width, height) = renderer.surface_size();
        self.chrome.resize(width, height, window.scale_factor() as f32);
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.fit_view();
        self.update_title();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if self.confirm_quit() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return;
                }
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
                let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32);
                let before = self.chrome.canvas_area();
                self.chrome.resize(size.width, size.height, scale);
                // 区画の中心が動いた分だけキャンバスもずらす
                let after = self.chrome.canvas_area();
                self.works[self.current].view.pan[0] += ((after.x0 + after.x1) - (before.x0 + before.x1)) as f32 * 0.5;
                self.works[self.current].view.pan[1] += ((after.y0 + after.y1) - (before.y0 + before.y1)) as f32 * 0.5;
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let state = self.ui_state();
                let ui_dirty = self.chrome.render(&state);
                let Some(r) = &mut self.renderer else { return };
                if !self.pending_upload.is_empty() {
                    r.upload_canvas(&self.works[self.current].doc, self.pending_upload);
                    self.pending_upload = Rect::EMPTY;
                }
                for region in ui_dirty {
                    r.upload_ui(region, &self.chrome.copy_region(region));
                }
                r.render(&self.works[self.current].view);
            }

            // ペン（Windows では WM_POINTER が Touch として届き、筆圧は force に入る）
            WindowEvent::Touch(t) => {
                // ペンで描いている間はマウスの移動が届かないことがあるので、ここでもカーソルを隠す
                let from = self.cursor;
                self.cursor = (t.location.x, t.location.y);
                self.update_cursor();
                self.pressure_seen |= t.force.is_some();
                let raw_pressure = pressure_of(t.force, self.pressure_seen);
                let pressure = self.curves[self.current_curve].apply(raw_pressure);
                let s = self.sample_at(t.location.x, t.location.y, pressure);
                let kind = self.press_kind();
                match t.phase {
                    TouchPhase::Started => {
                        self.pen_down = true;
                        if !self.press_view() && !self.press_ui(t.location.x, t.location.y, kind) {
                            if self.smooth_rotating() {
                                self.pivot_begin(t.location.x, t.location.y);
                            } else {
                                self.stroke_begin(s);
                            }
                        }
                    }
                    TouchPhase::Moved if self.rotating.is_some() || self.panning => self.drag_view(from),
                    TouchPhase::Moved if self.pivoting.is_some() => self.pivot_move(t.location.x, t.location.y),
                    TouchPhase::Moved if self.chrome.is_capturing() => self.drag_ui(t.location.x, t.location.y),
                    TouchPhase::Moved => self.stroke_extend(s),
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.release_ui();
                        self.stroke_end();
                        self.pivoting = None;
                        self.rotating = None;
                        self.panning = false;
                        self.pen_down = false;
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let from = self.cursor;
                self.cursor = (position.x, position.y);
                if self.pen_down && (self.rotating.is_some() || self.panning) {
                    // ペンで動かしている間は Touch で扱う。ここでも動かすと二重に動く
                } else if self.rotating.is_some() {
                    self.drag_view(from);
                } else if self.pivoting.is_some() {
                    self.pivot_move(position.x, position.y);
                } else if self.chrome.is_capturing() && !self.pen_down {
                    self.drag_ui(position.x, position.y);
                } else if self.panning {
                    self.drag_view(from);
                } else if !self.pen_down {
                    let s = self.sample_at(position.x, position.y, 1.0);
                    self.stroke_extend(s);
                }
                if !self.drawing() && !self.chrome.is_capturing() {
                    let state = self.ui_state();
                    if self.chrome.hover(position.x as f32, position.y as f32, &state) {
                        self.request_redraw();
                    }
                }
                self.update_cursor();
            }
            WindowEvent::CursorLeft { .. } => {
                self.chrome.set_brush_cursor(None);
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Middle, ElementState::Pressed) => self.panning = true,
                (MouseButton::Left, ElementState::Pressed) if !self.pen_down && self.press_view() => {}
                (MouseButton::Left, ElementState::Pressed) if !self.pen_down => {
                    let kind = self.press_kind();
                    if !self.press_ui(self.cursor.0, self.cursor.1, kind) {
                        if self.smooth_rotating() {
                            self.pivot_begin(self.cursor.0, self.cursor.1);
                        } else {
                            let s = self.sample_at(self.cursor.0, self.cursor.1, 1.0);
                            self.stroke_begin(s);
                        }
                    }
                }
                (MouseButton::Right, ElementState::Pressed) if !self.pen_down => {
                    self.press_ui(self.cursor.0, self.cursor.1, PressKind::Eyedropper);
                }
                (MouseButton::Middle | MouseButton::Left | MouseButton::Right, ElementState::Released) => {
                    self.panning = false;
                    self.rotating = None;
                    if !self.pen_down {
                        self.pivoting = None;
                    }
                    if !self.pen_down {
                        self.release_ui();
                    }
                    if !self.pen_down {
                        self.stroke_end();
                    }
                }
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 120.0,
                };
                let step = if self.modifiers.control_key() {
                    WheelStep::Coarse
                } else if self.modifiers.shift_key() {
                    WheelStep::Fine
                } else {
                    WheelStep::Normal
                };
                // ダイアログの欄では値を段で上下する。1 段に満たない量（タッチパッド）も 1 段とみなす
                let notches = if steps > 0.0 { steps.ceil() } else { steps.floor() } as i32;
                if self.chrome.wheel(self.cursor.0 as f32, self.cursor.1 as f32, notches, step) {
                    self.request_redraw();
                    return;
                }
                self.works[self.current].view.zoom_at(self.cursor.0 as f32, self.cursor.1 as f32, 1.15f32.powf(steps));
                self.update_cursor();
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                self.chrome.set_shift(self.modifiers.shift_key());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let ctrl = self.modifiers.control_key();
                let shift = self.modifiers.shift_key();
                let alt = self.modifiers.alt_key();
                // メニューやダイアログが開いている間は、キーはダイアログの欄と Esc・Enter にだけ渡す
                if pressed && self.chrome.is_modal() || pressed && event.logical_key == Key::Named(NamedKey::Escape) {
                    match event.logical_key.as_ref() {
                        Key::Named(NamedKey::Escape) => {
                            self.chrome.cancel();
                        }
                        Key::Named(NamedKey::Enter) => {
                            let action = self.chrome.confirm();
                            self.after_ui(action);
                        }
                        Key::Named(NamedKey::Tab) => self.chrome.dialog_key(DialogKey::Tab),
                        Key::Named(named) => {
                            let key = match named {
                                NamedKey::Backspace => Some(FieldKey::Backspace),
                                NamedKey::Delete => Some(FieldKey::Delete),
                                NamedKey::ArrowLeft => Some(FieldKey::Left { select: shift }),
                                NamedKey::ArrowRight => Some(FieldKey::Right { select: shift }),
                                NamedKey::Home => Some(FieldKey::Home { select: shift }),
                                NamedKey::End => Some(FieldKey::End { select: shift }),
                                _ => None,
                            };
                            if let Some(key) = key {
                                self.chrome.dialog_key(DialogKey::Field(key));
                            }
                        }
                        Key::Character(text) if ctrl => {
                            if text.eq_ignore_ascii_case("a") {
                                self.chrome.dialog_key(DialogKey::Field(FieldKey::SelectAll));
                            }
                        }
                        Key::Character(text) => {
                            for c in text.chars() {
                                self.chrome.dialog_key(DialogKey::Field(FieldKey::Char(c)));
                            }
                        }
                        _ => {}
                    }
                    self.request_redraw();
                    return;
                }
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Space) => {
                        self.space_held = pressed;
                        self.update_cursor();
                    }
                    Key::Named(NamedKey::Tab) if pressed && ctrl => self.cycle_tab(!shift),
                    Key::Character(c) if pressed && ctrl => match c.to_ascii_lowercase().as_str() {
                        // CLIP STUDIO と同じ。画像の書き出しは CLIP STUDIO に既定のキーがないので Ctrl+Shift+E
                        "s" if shift => {
                            self.save_as();
                        }
                        "s" => {
                            self.save();
                        }
                        "o" => self.open(),
                        "n" => self.show_new_document(),
                        "w" => {
                            self.close_work(self.current);
                        }
                        "e" if shift => self.show_export(ImageFormat::Png),
                        // 日本語配列では「^」、英語配列では「=」「+」の位置
                        "^" | "=" | "+" | ";" => self.apply_action(Action::Zoom(ZOOM_STEP)),
                        "-" => self.apply_action(Action::Zoom(1.0 / ZOOM_STEP)),
                        "0" if alt => self.actual_size(),
                        "0" => self.fit_view(),
                        "[" => self.edit_sheets(|doc| doc.move_active_sheet(false)),
                        "]" => self.edit_sheets(|doc| doc.move_active_sheet(true)),
                        _ => {}
                    },
                    Key::Character("[") if pressed && alt => self.edit_sheets(|doc| doc.select_sheet(false)),
                    Key::Character("]") if pressed && alt => self.edit_sheets(|doc| doc.select_sheet(true)),
                    Key::Character("t" | "T") if pressed => {
                        self.edit_sheets(|doc| doc.add_sheet(SheetMaterial::TracingPaper))
                    }
                    Key::Character("f" | "F") if pressed => {
                        self.edit_sheets(|doc| doc.add_sheet(SheetMaterial::ClearFilm))
                    }
                    Key::Character("v" | "V") if pressed => self.edit_sheets(|doc| doc.flip_active_sheet()),
                    Key::Named(NamedKey::Delete) if pressed => self.edit_sheets(|doc| doc.remove_active_sheet()),
                    Key::Character("r" | "R") => {
                        self.rotate_held = pressed;
                        self.update_cursor();
                    }
                    Key::Character("-") if pressed => self.rotate_by_step(-1.0),
                    Key::Character("^" | "=") if pressed => self.rotate_by_step(1.0),
                    Key::Character("0") if pressed => self.rotate_view(0.0),
                    Key::Character("k" | "K") if pressed => self.next_curve(),
                    Key::Character("p" | "P") if pressed => self.cycle_group(&PEN_GROUP),
                    Key::Character("b" | "B") if pressed => self.cycle_group(&BRUSH_GROUP),
                    Key::Character("e" | "E") if pressed => self.cycle_group(&ERASER_GROUP),
                    Key::Character("[") if pressed => self.resize_pen(1.0 / 1.2),
                    Key::Character("]") if pressed => self.resize_pen(1.2),
                    Key::Character(c) if pressed => {
                        if let Some(n) = c.parse::<usize>().ok().filter(|n| *n >= 1) {
                            self.select_tool(n - 1);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().expect("イベントループを作成できません");
    event_loop.set_control_flow(ControlFlow::Wait);
    // UI ができるまでは、起動引数 `--oil` で油彩キャンバスを選ぶ
    let mode = if std::env::args().any(|a| a == "--oil") { CanvasMode::OilCanvas } else { CanvasMode::Paper };
    event_loop.run_app(&mut App::new(mode)).expect("イベントループが異常終了しました");
}
