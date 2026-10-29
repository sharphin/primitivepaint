use super::icons::{self, Icon as CommandIcon};
use super::widgets;
use super::{Action, Frame, Panel, Pixmap, PressKind, Response, UiState, ZOOM_STEP, theme};
use pp_core::{CanvasMode, Rect};

/// 1 つのコマンド。名前は絵柄にポインタを載せたときに出す。
struct Command {
    icon: CommandIcon,
    name: &'static str,
    action: Action,
}

/// コマンドの並び。`None` は区切り。
const COMMANDS: [Option<Command>; 8] = [
    Some(Command { icon: CommandIcon::RotateLeft, name: "左に回転", action: Action::RotateStep(-1.0) }),
    Some(Command { icon: CommandIcon::RotateRight, name: "右に回転", action: Action::RotateStep(1.0) }),
    Some(Command { icon: CommandIcon::ResetRotation, name: "回転を戻す", action: Action::ResetRotation }),
    Some(Command { icon: CommandIcon::SmoothRotate, name: "滑らかに回転", action: Action::ToggleSmoothRotate }),
    None,
    Some(Command { icon: CommandIcon::ZoomOut, name: "縮小", action: Action::Zoom(1.0 / ZOOM_STEP) }),
    Some(Command { icon: CommandIcon::ZoomIn, name: "拡大", action: Action::Zoom(ZOOM_STEP) }),
    Some(Command { icon: CommandIcon::Fit, name: "全体を表示", action: Action::FitView }),
];

const BUTTON_SIZE: f32 = 22.0;
const GAP: f32 = 3.0;
const SEPARATOR: f32 = 12.0;

/// メニューバーの下のコマンドバー（回転・表示倍率）。CLIP STUDIO と同じく絵柄のボタンを並べ、
/// 名前はポインタを載せたときに説明として出す。
#[derive(Default)]
pub struct CommandBar {
    frame: Frame,
    /// ポインタが載っているコマンドの番号（`COMMANDS` の添字）。
    hovered: Option<usize>,
}

impl CommandBar {
    /// (`COMMANDS` の添字, ボタンの矩形)。
    fn layout(&self) -> Vec<(usize, Rect)> {
        let mut x = 8.0;
        let mut rects = Vec::new();
        for (i, command) in COMMANDS.iter().enumerate() {
            if command.is_none() {
                x += SEPARATOR;
                continue;
            }
            rects.push((i, self.frame.sub(x, 4.0, BUTTON_SIZE, BUTTON_SIZE)));
            x += BUTTON_SIZE + GAP;
        }
        rects
    }

    fn enabled(action: Action, state: &UiState) -> bool {
        match action {
            // 油彩キャンバスは 90° ずつしか回らない
            Action::ToggleSmoothRotate => state.mode == CanvasMode::Paper,
            _ => true,
        }
    }

    fn command_at(&self, x: f32, y: f32) -> Option<usize> {
        self.layout().into_iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| i)
    }
}

impl Panel for CommandBar {
    fn place(&mut self, frame: Frame) {
        self.frame = frame;
    }

    fn frame(&self) -> Frame {
        self.frame
    }

    fn render(&self, state: &UiState, pixmap: &mut Pixmap) {
        widgets::background(pixmap, &self.frame);
        for (i, rect) in self.layout() {
            let Some(command) = &COMMANDS[i] else { continue };
            let enabled = CommandBar::enabled(command.action, state);
            let on = command.action == Action::ToggleSmoothRotate && state.smooth_rotate;
            if enabled && (on || self.hovered == Some(i)) {
                pixmap.fill_rect(rect, theme::colors().selected);
                pixmap.frame(rect, theme::colors().selected_border);
            }
            let color = if enabled { theme::colors().text } else { theme::colors().text_disabled };
            icons::draw(pixmap, rect, command.icon, color);
        }
    }

    fn tooltip(&self, x: f32, y: f32, state: &UiState) -> Option<String> {
        let command = COMMANDS[self.command_at(x, y)?].as_ref()?;
        Some(match command.action {
            Action::ToggleSmoothRotate if state.mode != CanvasMode::Paper => {
                "滑らかに回転（油彩キャンバスでは使えません）".to_string()
            }
            Action::ToggleSmoothRotate if state.smooth_rotate => {
                "滑らかに回転：ON（描かずに、押した所を軸にして動かすと回る）".to_string()
            }
            Action::ToggleSmoothRotate => "滑らかに回転：OFF".to_string(),
            _ => command.name.to_string(),
        })
    }

    fn press(&mut self, x: f32, y: f32, _kind: PressKind, state: &UiState) -> Response {
        let action = self.command_at(x, y).and_then(|i| COMMANDS[i].as_ref()).map(|c| c.action);
        match action {
            Some(action) if CommandBar::enabled(action, state) => Response::action(action),
            _ => Response::default(),
        }
    }

    fn hover(&mut self, x: f32, y: f32) -> bool {
        let hovered = self.command_at(x, y);
        let changed = hovered != self.hovered;
        self.hovered = hovered;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::state;

    fn bar() -> CommandBar {
        let mut bar = CommandBar::default();
        bar.place(Frame { rect: Rect::new(0, 0, 1280, 34), scale: 1.0 });
        bar
    }

    fn center(r: Rect) -> (f32, f32) {
        ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5)
    }

    #[test]
    fn buttons_send_their_action_unless_disabled() {
        let mut bar = bar();
        let mut state = state();
        let layout = bar.layout();
        let (x, y) = center(layout[0].1);
        assert_eq!(bar.press(x, y, PressKind::Primary, &state).action, Some(Action::RotateStep(-1.0)));
        // 油彩キャンバスでは滑らかに回転できない
        state.mode = CanvasMode::OilCanvas;
        let (x, y) = center(layout[3].1);
        assert_eq!(bar.press(x, y, PressKind::Primary, &state).action, None);
        let (x, y) = center(layout.last().unwrap().1);
        assert_eq!(bar.press(x, y, PressKind::Primary, &state).action, Some(Action::FitView));
    }

    #[test]
    fn hover_reports_changes_only() {
        let mut bar = bar();
        let (x, y) = center(bar.layout()[2].1);
        assert!(bar.hover(x, y));
        assert!(!bar.hover(x, y));
        assert!(bar.hover(900.0, 10.0));
        assert_eq!(bar.hovered, None);
    }
}
