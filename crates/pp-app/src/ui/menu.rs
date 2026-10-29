use std::cell::RefCell;

use pp_core::{PaletteBoard, PressureCurve, Rect};
use pp_file::{CopyFormat, ImageFormat};

use super::pixmap::Align;
use super::theme::UiTheme;
use super::{Action, Frame, Panel, Pixmap, PressKind, Response, UiState, ZOOM_STEP, theme};

/// メニューの 1 行。
enum Entry {
    Item {
        label: String,
        shortcut: &'static str,
        action: Action,
    },
    /// 横に開く子メニュー（「複製を保存」「書き出し」の形式など）。子は `Item` だけ。
    Submenu {
        label: &'static str,
        items: Vec<Entry>,
    },
    Separator,
}

struct Menu {
    title: &'static str,
    entries: Vec<Entry>,
}

fn item(label: &str, shortcut: &'static str, action: Action) -> Entry {
    Entry::Item { label: label.to_string(), shortcut, action }
}

/// メニューバーの中身。CLIP STUDIO の「ファイル」「編集」「表示」から、このアプリでできることだけを並べ、
/// アプリの設定は「オプション」にまとめる。
fn menus() -> Vec<Menu> {
    let export = ImageFormat::ALL
        .iter()
        .map(|&format| Entry::Item {
            label: format!("{}…", format.name()),
            shortcut: if format == ImageFormat::Png { "Ctrl+Shift+E" } else { "" },
            action: Action::Export(format),
        })
        .collect();
    // CLIP STUDIO と同じく「.psd（Photoshop ドキュメント）」のように拡張子を先に出す
    let copies = CopyFormat::ALL
        .iter()
        .map(|&format| Entry::Item {
            label: format!(".{}（{}）…", format.extension(), format.name()),
            shortcut: "",
            action: Action::SaveCopy(format),
        })
        .collect();
    let curves = PressureCurve::presets()
        .iter()
        .enumerate()
        .map(|(i, curve)| item(curve.name(), "", Action::SelectCurve(i)))
        .collect();
    let themes = UiTheme::ALL.iter().map(|&theme| item(theme.name(), "", Action::SetTheme(theme))).collect();
    let boards =
        PaletteBoard::ALL.iter().map(|&board| item(board.name(), "", Action::SetPaletteBoard(board))).collect();
    vec![
        Menu {
            title: "ファイル",
            entries: vec![
                item("新規…", "Ctrl+N", Action::ShowNewDocument),
                item("開く…", "Ctrl+O", Action::Open),
                item("閉じる", "Ctrl+W", Action::CloseCurrentTab),
                Entry::Separator,
                item("保存", "Ctrl+S", Action::Save),
                item("別名で保存…", "Ctrl+Shift+S", Action::SaveAs),
                Entry::Submenu { label: "複製を保存", items: copies },
                Entry::Separator,
                Entry::Submenu { label: "画像を統合して書き出し", items: export },
            ],
        },
        Menu {
            title: "表示",
            entries: vec![
                item("拡大", "Ctrl+^", Action::Zoom(ZOOM_STEP)),
                item("縮小", "Ctrl+-", Action::Zoom(1.0 / ZOOM_STEP)),
                item("100%", "Ctrl+Alt+0", Action::ActualSize),
                item("全体を表示", "Ctrl+0", Action::FitView),
                Entry::Separator,
                item("回転を戻す", "0", Action::ResetRotation),
            ],
        },
        Menu {
            title: "オプション",
            entries: vec![
                Entry::Submenu { label: "筆圧カーブ", items: curves },
                Entry::Submenu { label: "パレットの下地", items: boards },
                Entry::Submenu { label: "配色", items: themes },
            ],
        },
    ]
}

const TITLE_PAD: f32 = 10.0;
const DROPDOWN_WIDTH: f32 = 250.0;
const ITEM_HEIGHT: f32 = 22.0;
const SEPARATOR_HEIGHT: f32 = 9.0;

/// メニューを押したり、開いたメニューの上を動いたりした結果。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MenuResponse {
    pub action: Option<Action>,
    /// 表示が変わったか。
    pub changed: bool,
}

/// 画面最上段のメニューバー。押したメニューの下に項目の一覧を開く。
#[derive(Default)]
pub struct MenuBar {
    frame: Frame,
    /// 開いているメニューの番号。
    open: Option<usize>,
    /// ポインタが載っている項目（開いているメニューの中の番号）。
    hovered: Option<usize>,
    /// 開いている子メニュー（その親の項目の番号）。
    submenu: Option<usize>,
    /// ポインタが載っている子メニューの項目。
    hovered_child: Option<usize>,
    /// 最後に描いたときに測った、各メニュー名の幅（論理ピクセル）。押した位置を調べるのにも使う。
    title_widths: RefCell<Vec<f32>>,
}

impl MenuBar {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        *self =
            MenuBar { frame: self.frame, title_widths: RefCell::new(self.title_widths.take()), ..MenuBar::default() };
    }

    fn title_rects(&self) -> Vec<Rect> {
        let mut x = 4.0;
        let widths = self.title_widths.borrow();
        // 名前の左右の余白は、どのメニューも「編集」「表示」と同じにする。
        // 描く前でまだ測っていないときは、1 文字を全角とみなす
        menus()
            .iter()
            .enumerate()
            .map(|(i, menu)| {
                let text = widths.get(i).copied().unwrap_or(menu.title.chars().count() as f32 * theme::BODY_TEXT);
                let width = text.ceil() + TITLE_PAD * 2.0;
                let rect = self.frame.sub(x, 0.0, width, self.frame.rect.height() as f32 / self.frame.scale);
                x += width;
                rect
            })
            .collect()
    }

    fn title_at(&self, x: f32, y: f32) -> Option<usize> {
        self.title_rects().iter().position(|r| r.contains(x, y))
    }

    /// `entries` を `origin`（左上）から縦に並べたときの各行の矩形。
    fn rows(&self, entries: &[Entry], origin: (i32, i32)) -> Vec<Rect> {
        let mut y = origin.1;
        entries
            .iter()
            .map(|entry| {
                let height = self.frame.px(match entry {
                    Entry::Separator => SEPARATOR_HEIGHT,
                    _ => ITEM_HEIGHT,
                });
                let rect = Rect::new(origin.0, y, origin.0 + self.frame.px(DROPDOWN_WIDTH), y + height);
                y += height;
                rect
            })
            .collect()
    }

    /// 開いているメニューの一覧の行。
    fn dropdown_rows(&self, menus: &[Menu]) -> Vec<Rect> {
        let Some(open) = self.open else { return Vec::new() };
        let title = self.title_rects()[open];
        self.rows(&menus[open].entries, (title.x0, self.frame.rect.y1))
    }

    /// 開いている子メニューの行。
    fn submenu_rows(&self, menus: &[Menu]) -> Vec<Rect> {
        let (Some(open), Some(parent)) = (self.open, self.submenu) else { return Vec::new() };
        let Entry::Submenu { items, .. } = &menus[open].entries[parent] else { return Vec::new() };
        let anchor = self.dropdown_rows(menus)[parent];
        self.rows(items, (anchor.x1, anchor.y0))
    }

    /// 開いている一覧（子メニューを含む）が占める範囲。描き直しに使う。
    pub fn overlay_rect(&self) -> Rect {
        let menus = menus();
        let bound = |rows: Vec<Rect>| rows.into_iter().fold(Rect::EMPTY, |acc, r| acc.union(&r));
        let dropdown = bound(self.dropdown_rows(&menus));
        let submenu = bound(self.submenu_rows(&menus));
        dropdown.union(&submenu)
    }

    /// メニューが開いている間の押下。項目を選んだら閉じて、その操作を返す。一覧の外なら閉じる。
    pub fn press_open(&mut self, x: f32, y: f32) -> MenuResponse {
        let menus = menus();
        if let Some(index) = self.title_at(x, y) {
            if self.open == Some(index) {
                self.close();
            } else {
                self.open_menu(index);
            }
            return MenuResponse { action: None, changed: true };
        }
        let child_rows = self.submenu_rows(&menus);
        if let Some(row) = child_rows.iter().position(|r| r.contains(x, y)) {
            let (Some(open), Some(parent)) = (self.open, self.submenu) else { return MenuResponse::default() };
            if let Entry::Submenu { items, .. } = &menus[open].entries[parent] {
                return self.choose(&items[row]);
            }
        }
        let rows = self.dropdown_rows(&menus);
        if let (Some(open), Some(row)) = (self.open, rows.iter().position(|r| r.contains(x, y))) {
            return match &menus[open].entries[row] {
                Entry::Submenu { .. } => {
                    self.submenu = Some(row);
                    MenuResponse { action: None, changed: true }
                }
                Entry::Separator => MenuResponse::default(),
                entry => self.choose(entry),
            };
        }
        self.close();
        MenuResponse { action: None, changed: true }
    }

    fn choose(&mut self, entry: &Entry) -> MenuResponse {
        match entry {
            Entry::Item { action, .. } => {
                self.close();
                MenuResponse { action: Some(*action), changed: true }
            }
            _ => MenuResponse::default(),
        }
    }

    fn open_menu(&mut self, index: usize) {
        self.close();
        self.open = Some(index);
    }

    /// メニューが開いている間のポインタの移動。別のメニュー名に載ったらそちらを開き、子メニューは載ったら開く。
    pub fn hover_open(&mut self, x: f32, y: f32) -> bool {
        let before = (self.open, self.hovered, self.submenu, self.hovered_child);
        let menus = menus();
        if let Some(index) = self.title_at(x, y).filter(|&i| Some(i) != self.open) {
            self.open_menu(index);
        }
        self.hovered_child = self.submenu_rows(&menus).iter().position(|r| r.contains(x, y));
        if self.hovered_child.is_none() {
            self.hovered = self.dropdown_rows(&menus).iter().position(|r| r.contains(x, y));
            if let (Some(open), Some(row)) = (self.open, self.hovered) {
                self.submenu = matches!(menus[open].entries[row], Entry::Submenu { .. }).then_some(row);
            }
        }
        before != (self.open, self.hovered, self.submenu, self.hovered_child)
    }

    /// 開いている一覧を描く。パネルを描いた後に、その上へ重ねて描く。
    pub fn render_overlay(&self, pixmap: &mut Pixmap) {
        let Some(open) = self.open else { return };
        let menus = menus();
        let entries = &menus[open].entries;
        self.render_list(pixmap, entries, &self.dropdown_rows(&menus), self.hovered.or(self.submenu));
        if let Some(Entry::Submenu { items, .. }) = self.submenu.map(|i| &entries[i]) {
            self.render_list(pixmap, items, &self.submenu_rows(&menus), self.hovered_child);
        }
    }

    fn render_list(&self, pixmap: &mut Pixmap, entries: &[Entry], rows: &[Rect], lit: Option<usize>) {
        let all = rows.iter().fold(Rect::EMPTY, |acc, r| acc.union(r));
        pixmap.fill_rect(all, theme::colors().button);
        pixmap.frame(all, theme::colors().border);
        let pad = self.frame.px(12.0);
        let font = self.frame.font_px(theme::BODY_TEXT);
        for (i, (entry, row)) in entries.iter().zip(rows).enumerate() {
            let inner = Rect::new(row.x0 + pad, row.y0, row.x1 - pad, row.y1);
            match entry {
                Entry::Separator => {
                    let mid = (row.y0 + row.y1) / 2;
                    pixmap.fill_rect(Rect::new(row.x0 + 1, mid, row.x1 - 1, mid + 1), theme::colors().border);
                }
                Entry::Item { label, shortcut, .. } => {
                    if lit == Some(i) {
                        pixmap.fill_rect(Rect::new(row.x0 + 1, row.y0, row.x1 - 1, row.y1), theme::colors().selected);
                    }
                    pixmap.text(inner, label, font, theme::colors().text, Align::Left);
                    pixmap.text(inner, shortcut, font, theme::colors().text_disabled, Align::Right);
                }
                Entry::Submenu { label, .. } => {
                    if lit == Some(i) {
                        pixmap.fill_rect(Rect::new(row.x0 + 1, row.y0, row.x1 - 1, row.y1), theme::colors().selected);
                    }
                    pixmap.text(inner, label, font, theme::colors().text, Align::Left);
                    // 横に開くことを示す小さな三角
                    let size = self.frame.px(4.0) as f32;
                    let (tx, ty) = (inner.x1 as f32 - size, (row.y0 + row.y1) as f32 * 0.5);
                    pixmap.fill_polygon(
                        &[(tx - size, ty - size), (tx + size * 0.6, ty), (tx - size, ty + size)],
                        theme::colors().text,
                    );
                }
            }
        }
    }
}

impl Panel for MenuBar {
    fn place(&mut self, frame: Frame) {
        self.frame = frame;
    }

    fn frame(&self) -> Frame {
        self.frame
    }

    fn render(&self, _state: &UiState, pixmap: &mut Pixmap) {
        let r = self.frame.rect;
        pixmap.fill_rect(r, theme::colors().panel);
        pixmap.fill_rect(Rect::new(r.x0, r.y1 - 1, r.x1, r.y1), theme::colors().border);
        let font = self.frame.font_px(theme::BODY_TEXT);
        let measured: Option<Vec<f32>> =
            menus().iter().map(|menu| pixmap.measure_text(menu.title, font).map(|w| w / self.frame.scale)).collect();
        if let Some(widths) = measured {
            *self.title_widths.borrow_mut() = widths;
        }
        for (i, (menu, rect)) in menus().iter().zip(self.title_rects()).enumerate() {
            if self.open == Some(i) {
                pixmap.fill_rect(Rect::new(rect.x0, rect.y0, rect.x1, rect.y1 - 1), theme::colors().selected);
            }
            pixmap.text(rect, menu.title, font, theme::colors().text, Align::Center);
        }
    }

    fn press(&mut self, x: f32, y: f32, _kind: PressKind, _state: &UiState) -> Response {
        match self.title_at(x, y) {
            Some(index) => {
                self.open_menu(index);
                Response { action: None, redraw: true }
            }
            None => Response::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::state;

    fn bar() -> MenuBar {
        let mut bar = MenuBar::default();
        bar.place(Frame { rect: Rect::new(0, 0, 1000, 24), scale: 1.0 });
        bar
    }

    fn center(r: Rect) -> (f32, f32) {
        ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5)
    }

    #[test]
    fn choosing_an_item_returns_its_action_and_closes() {
        let mut bar = bar();
        let (x, y) = center(bar.title_rects()[0]);
        bar.press(x, y, PressKind::Primary, &state());
        assert!(bar.is_open());
        let rows = bar.dropdown_rows(&menus());
        let (x, y) = center(rows[4]);
        assert_eq!(bar.press_open(x, y).action, Some(Action::Save));
        assert!(!bar.is_open());
    }

    #[test]
    fn export_formats_are_in_a_submenu() {
        let mut bar = bar();
        bar.open_menu(0);
        let rows = bar.dropdown_rows(&menus());
        let (x, y) = center(*rows.last().unwrap());
        assert!(bar.hover_open(x, y));
        let child = bar.submenu_rows(&menus());
        assert_eq!(child.len(), ImageFormat::ALL.len());
        let (cx, cy) = center(child[0]);
        assert_eq!(bar.press_open(cx, cy).action, Some(Action::Export(ImageFormat::Png)));
    }

    #[test]
    fn save_copy_formats_are_in_a_submenu() {
        let mut bar = bar();
        bar.open_menu(0);
        let rows = bar.dropdown_rows(&menus());
        // 新規・開く・閉じる・区切り・保存・別名で保存の次
        let (x, y) = center(rows[6]);
        assert!(bar.hover_open(x, y));
        let child = bar.submenu_rows(&menus());
        assert_eq!(child.len(), CopyFormat::ALL.len());
        let (cx, cy) = center(child[0]);
        assert_eq!(bar.press_open(cx, cy).action, Some(Action::SaveCopy(CopyFormat::Psd)));
    }

    #[test]
    fn themes_are_chosen_from_the_options_menu() {
        let mut bar = bar();
        bar.open_menu(2);
        let rows = bar.dropdown_rows(&menus());
        // 筆圧カーブ・パレットの下地の次
        let (x, y) = center(rows[2]);
        assert!(bar.hover_open(x, y));
        let child = bar.submenu_rows(&menus());
        assert_eq!(child.len(), UiTheme::ALL.len());
        let (cx, cy) = center(child[1]);
        assert_eq!(bar.press_open(cx, cy).action, Some(Action::SetTheme(UiTheme::Dark)));
    }

    #[test]
    fn pressing_outside_closes() {
        let mut bar = bar();
        bar.open_menu(1);
        bar.press_open(900.0, 500.0);
        assert!(!bar.is_open());
    }

    #[test]
    fn hovering_another_title_switches_menus() {
        let mut bar = bar();
        bar.open_menu(0);
        let (x, y) = center(bar.title_rects()[2]);
        assert!(bar.hover_open(x, y));
        assert_eq!(bar.open, Some(2));
        assert!(!bar.overlay_rect().is_empty());
    }
}
