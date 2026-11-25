use super::*;

const CLASS: &str = "MakerPreferencesWindow";
const THEME_ROW: i32 = 1;
const WRAP_ROW: i32 = 2;
const FONT_ROW: i32 = 3;
const SIZE_ROW: i32 = 4;
const INDENT_ROW: i32 = 5;
const SIDEBAR_ROW: i32 = 6;
const APPLY: i32 = 7;
const CLOSE: i32 = 8;
const GET_SELECTION: u32 = 0x0147;
const SET_SELECTION: u32 = 0x014e;

pub(super) fn register() -> io::Result<()> {
    register_class(CLASS, procedure)
}

pub(super) fn has_focus(window: Handle, target: Handle) -> bool {
    is_child(window, target)
}

pub(super) fn keyboard(window: Handle, key: i32) -> bool {
    if key == 0x1b || key == 13 {
        let focus = unsafe { GetFocus() };
        let id = unsafe { GetDlgCtrlID(focus) };
        if matches!(id, THEME_ROW | WRAP_ROW | INDENT_ROW | SIDEBAR_ROW)
            && send(focus, 0x0157, 0, 0) != 0
        {
            return false;
        }
        if key == 13 && matches!(id, APPLY | CLOSE) {
            return false;
        }
        let command = if key == 13 { APPLY } else { CLOSE };
        unsafe {
            PostMessageW(window, WM_COMMAND, command as usize, 0);
        }
        true
    } else if key == 9 {
        next_dialog_control(window, unsafe { GetKeyState(0x10) < 0 });
        true
    } else {
        false
    }
}

extern "system" fn procedure(window: Handle, message: u32, wp: usize, lp: isize) -> isize {
    match message {
        WM_ERASEBKGND => return 1,
        WM_PAINT => {
            buffered_paint(window, |dc, area| {
                if let Some(paint) = PAINT.get() {
                    fill(dc, area, paint.background);
                    let px = |n: i32| n * paint.dpi / 96;
                    draw_label(
                        dc,
                        Rect::from_size(px(24), px(15), area.width() - px(48), px(40)),
                        "설정",
                        paint.large_font,
                        paint.foreground,
                        false,
                    );
                    for (index, label) in [
                        "테마",
                        "자동 줄바꿈",
                        "편집기 글꼴",
                        "글자 크기",
                        "들여쓰기",
                        "탐색기 위치",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let y = px(67 + index as i32 * 38);
                        draw_label(
                            dc,
                            Rect::from_size(px(24), y, px(142), px(27)),
                            label,
                            paint.font,
                            paint.foreground,
                            false,
                        );
                    }
                }
            });
            return 0;
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            if let Some(paint) = PAINT.get() {
                unsafe {
                    SetTextColor(wp as Handle, paint.foreground);
                    SetBkColor(wp as Handle, paint.control_rgb);
                }
                return paint.control;
            }
        }
        WM_COMMAND if wp & 0xffff == APPLY as usize => {
            with_app(|app| {
                if let Err(error) = app.save_preferences(window) {
                    report_error(window, &error.to_string());
                }
            });
            return 0;
        }
        WM_COMMAND if wp & 0xffff == CLOSE as usize => {
            unsafe {
                DestroyWindow(window);
            }
            return 0;
        }
        WM_DESTROY => {
            with_app(|app| {
                if app.preferences_window == window {
                    app.preferences_window = 0;
                }
            });
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(window, message, wp, lp) }
}

fn field(window: Handle, id: i32) -> Handle {
    unsafe { GetDlgItem(window, id) }
}

fn set_combo(control: Handle, choices: &[&str], selected: usize) {
    for choice in choices {
        let text = choice.encode_utf16().chain([0]).collect::<Vec<_>>();
        send(control, 0x0143, 0, text.as_ptr() as isize);
    }
    send(control, SET_SELECTION, selected, 0);
}

impl App {
    pub(super) fn open_preferences(&mut self) -> io::Result<()> {
        if self.preferences_window != 0 {
            unsafe {
                SetForegroundWindow(self.preferences_window);
            }
            return Ok(());
        }
        self.close_menu(false);
        let window = tool_window(
            CLASS,
            "Maker 설정",
            self.window,
            Point {
                x: self.px(424),
                y: self.px(393),
            },
        )?;
        if let Err(error) = self.build_preferences(window) {
            unsafe {
                DestroyWindow(window);
            }
            return Err(error);
        }
        self.preferences_window = window;
        show_main_window(window, false);
        unsafe {
            SetFocus(field(window, THEME_ROW));
        }
        Ok(())
    }

    fn build_preferences(&self, window: Handle) -> io::Result<()> {
        let add = |id, class: &str, label: &str, style, rect: Rect| {
            let control = child(window, id as usize, class, label, style | WS_TABSTOP)?;
            place(
                control,
                self.px(rect.left),
                self.px(rect.top),
                self.px(rect.width()),
                self.px(rect.height()),
            );
            if let Some(paint) = PAINT.get() {
                set_font(control, paint.font);
            }
            show(control, true);
            Ok::<Handle, io::Error>(control)
        };
        let values = [
            (
                &["어두운 테마", "밝은 테마"][..],
                usize::from(!self.settings.dark),
            ),
            (&["끔", "켬"][..], usize::from(self.settings.wrap)),
            (&[][..], 0),
            (&[][..], 0),
            (
                &[
                    "탭 문자",
                    "공백 1칸",
                    "공백 2칸",
                    "공백 3칸",
                    "공백 4칸",
                    "공백 5칸",
                    "공백 6칸",
                    "공백 7칸",
                    "공백 8칸",
                ][..],
                self.settings.spaces,
            ),
            (
                &["왼쪽", "오른쪽"][..],
                usize::from(self.settings.sidebar_right),
            ),
        ];
        for (index, (choices, selected)) in values.into_iter().enumerate() {
            let id = index as i32 + 1;
            let y = 67 + index as i32 * 38;
            if choices.is_empty() {
                let value = if id == FONT_ROW {
                    self.settings.font.clone()
                } else {
                    self.settings.points.to_string()
                };
                let input = add(
                    id,
                    "EDIT",
                    &value,
                    WS_BORDER | ES_AUTOHSCROLL,
                    Rect::from_size(164, y, 214, 25),
                )?;
                send(input, 0x00c5, if id == FONT_ROW { 31 } else { 2 }, 0);
            } else {
                let list = add(
                    id,
                    "COMBOBOX",
                    "",
                    3 | WS_VSCROLL,
                    Rect::from_size(164, y, 214, 230),
                )?;
                set_combo(list, choices, selected);
            }
        }
        add(
            APPLY,
            "BUTTON",
            "적용",
            1,
            Rect::from_size(210, 311, 78, 29),
        )?;
        add(
            CLOSE,
            "BUTTON",
            "닫기",
            0,
            Rect::from_size(300, 311, 78, 29),
        )?;
        Ok(())
    }

    fn save_preferences(&mut self, window: Handle) -> io::Result<()> {
        let selected = |id| send(field(window, id), GET_SELECTION, 0, 0);
        let font = caption(field(window, FONT_ROW));
        let font = font.trim();
        if font.is_empty() || font.chars().any(char::is_control) {
            return Err(io::Error::other("글꼴 이름을 입력하세요."));
        }
        let size = caption(field(window, SIZE_ROW))
            .parse::<i32>()
            .ok()
            .filter(|size| (8..=36).contains(size))
            .ok_or_else(|| io::Error::other("글자 크기는 8~36 사이로 입력하세요."))?;
        let mut settings = self.settings.clone();
        settings.font = font.to_owned();
        settings.points = size;
        settings.dark = selected(THEME_ROW) == 0;
        settings.wrap = selected(WRAP_ROW) == 1;
        settings.spaces = selected(INDENT_ROW).clamp(0, 8) as usize;
        settings.sidebar_right = selected(SIDEBAR_ROW) == 1;
        settings.save()?;
        self.settings = settings;
        self.apply_preferences();
        repaint(window);
        for id in THEME_ROW..=CLOSE {
            if let Some(paint) = PAINT.get() {
                set_font(field(window, id), paint.font);
            }
            repaint(field(window, id));
        }
        Ok(())
    }
}
