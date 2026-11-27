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
const LANGUAGE_ROW: i32 = 9;
const GET_SELECTION: u32 = 0x0147;
const SET_SELECTION: u32 = 0x014e;
const SHOW_DROPDOWN: u32 = 0x014f;
const DROPDOWN_OPEN: u32 = 0x0157;
const GET_EDIT_SELECTION: u32 = 0x0140;
const SET_EDIT_SELECTION: u32 = 0x0142;
const CBN_EDITCHANGE: usize = 5;
const CBN_DROPDOWN: usize = 7;
const SELECT_ONLY: usize = 0;
const DROPDOWN_LIST: usize = 1;
const FONT_CHOICE: usize = 2;
const FONT_LIST: usize = 3;
const REPAINT_FONT_FRAME: u32 = 0x8000;

thread_local! {
    static FONT_NAMES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CHANGING_FONT_CHOICES: Cell<bool> = const { Cell::new(false) };
}

const ROWS: [(i32, i32, &str); 7] = [
    (LANGUAGE_ROW, 131, "settings.language"),
    (THEME_ROW, 170, "settings.theme"),
    (SIDEBAR_ROW, 209, "settings.sidebar"),
    (FONT_ROW, 297, "settings.font"),
    (SIZE_ROW, 336, "settings.size"),
    (WRAP_ROW, 375, "settings.wrap"),
    (INDENT_ROW, 414, "settings.indent"),
];

pub(super) fn register() -> io::Result<()> {
    register_class(CLASS, procedure)
}

pub(super) fn has_focus(window: Handle, target: Handle) -> bool {
    is_child(window, target) || is_child(field(window, FONT_ROW), target)
}

pub(super) fn keyboard(window: Handle, key: i32) -> bool {
    let focus = unsafe { GetFocus() };
    let font = field(window, FONT_ROW);
    let focus = if is_child(font, focus) { font } else { focus };
    if key == 9 {
        if focus == font && send(font, DROPDOWN_OPEN, 0, 0) != 0 {
            send(font, SHOW_DROPDOWN, 0, 0);
        }
        next_dialog_control(window, focus, unsafe { GetKeyState(0x10) < 0 });
        return true;
    }
    if !matches!(key, 0x1b | 13) {
        return false;
    }
    let id = unsafe { GetDlgCtrlID(focus) };
    if matches!(
        id,
        THEME_ROW | WRAP_ROW | INDENT_ROW | SIDEBAR_ROW | LANGUAGE_ROW | FONT_ROW
    ) && send(focus, DROPDOWN_OPEN, 0, 0) != 0
    {
        return false;
    }
    let command = if key == 0x1b || id == CLOSE {
        CLOSE
    } else {
        APPLY
    };
    unsafe {
        PostMessageW(window, WM_COMMAND, command as usize, 0);
    }
    true
}

fn paint_settings(dc: Handle, area: Rect, paint: Paint) {
    let px = |n: i32| n * paint.dpi / 96;
    let rect = |x, y, w, h| Rect::from_size(px(x), px(y), px(w), px(h));
    fill(dc, area, paint.background);
    fill(dc, rect(0, 0, 580, 79), paint.chrome);
    fill(dc, rect(28, 23, 3, 33), paint.accent_brush);
    draw_label(
        dc,
        rect(43, 12, 500, 35),
        i18n::text("settings.title"),
        paint.large_font,
        paint.foreground,
        false,
    );
    draw_label(
        dc,
        rect(44, 47, 500, 20),
        i18n::text("settings.subtitle"),
        paint.font,
        paint.muted,
        false,
    );
    for (top, height, key) in [
        (95, 154, "settings.appearance"),
        (261, 190, "settings.editor"),
    ] {
        fill_rounded(dc, rect(24, top, 515, height), px(5), paint.border_rgb);
        fill_rounded(
            dc,
            rect(25, top + 1, 513, height - 2),
            px(5),
            paint.panel_rgb,
        );
        draw_label(
            dc,
            rect(39, top + 8, 320, 25),
            i18n::text(key),
            paint.font,
            paint.accent,
            false,
        );
    }
    for (_, top, key) in ROWS {
        draw_label(
            dc,
            rect(42, top, 217, 28),
            i18n::text(key),
            paint.font,
            paint.foreground,
            false,
        );
    }
    fill(dc, rect(24, 466, 515, 1), paint.border);
}

fn combo_label(window: Handle, index: usize) -> String {
    let length = send(window, 0x0149, index, 0);
    if length <= 0 {
        return String::new();
    }
    let mut units = vec![0u16; length as usize + 1];
    send(window, 0x0148, index, units.as_mut_ptr() as isize);
    String::from_utf16_lossy(&units[..length as usize])
}

fn paint_choice(window: Handle, dc: Handle, rect: Rect, paint: Paint, editable: bool) {
    let px = |n: i32| n * paint.dpi / 96;
    let opened = send(window, DROPDOWN_OPEN, 0, 0) != 0;
    let focused = is_child(window, unsafe { GetFocus() });
    let border = if focused || opened {
        paint.accent_brush
    } else {
        paint.border
    };
    fill(dc, rect, border);
    let inside = Rect::from_size(1, 1, rect.width() - 2, rect.height() - 2);
    let edge = if editable {
        combo_info(window).button.left.max(inside.left)
    } else {
        (rect.right - px(27)).max(inside.left)
    };
    fill(
        dc,
        inside,
        if opened && !editable {
            paint.selected
        } else {
            paint.control
        },
    );
    fill(
        dc,
        Rect::from_size(edge, inside.top + px(5), 1, inside.height() - px(10)),
        paint.border,
    );
    let selected = send(window, GET_SELECTION, 0, 0);
    if !editable && selected >= 0 {
        draw_label(
            dc,
            Rect {
                left: px(8),
                right: edge - px(5),
                ..inside
            },
            &combo_label(window, selected as usize),
            paint.font,
            paint.foreground,
            false,
        );
    }
    let x = edge + (rect.right - edge) / 2;
    let y = rect.height() / 2;
    let color = if focused || opened {
        paint.accent
    } else {
        paint.muted
    };
    chrome::chevron(
        dc,
        Point { x, y },
        px(4),
        if opened { -1.0 } else { 1.0 },
        color,
    );
}

fn paint_font_list_frame(window: Handle, paint: Paint) -> bool {
    paint_window_frame_with_scroll(window, paint.border, |dc, rect| {
        if rect.width() < 3 || rect.height() < 3 {
            return;
        }
        fill(dc, rect, paint.menu);
        let step = rect.width().min(rect.height() / 3);
        let center = (rect.left + rect.right) / 2;
        for (y, direction) in [(rect.top + step / 2, -1.0), (rect.bottom - step / 2, 1.0)] {
            chrome::chevron(dc, Point { x: center, y }, step / 4, direction, paint.muted);
        }
        if let Some((start, length)) = chrome::vertical_thumb(
            window,
            rect.height() - step * 2,
            (18 * paint.dpi / 96).max(1),
        ) {
            let inset = (rect.width() / 3).max(1);
            fill(
                dc,
                Rect::from_size(
                    rect.left + inset,
                    rect.top + step + start,
                    rect.width() - inset * 2,
                    length,
                ),
                paint.thumb,
            );
        }
    })
}

extern "system" fn choice_proc(
    window: Handle,
    message: u32,
    wp: usize,
    lp: isize,
    _id: usize,
    kind: usize,
) -> isize {
    let field = matches!(kind, SELECT_ONLY | FONT_CHOICE);
    let editable = kind == FONT_CHOICE;
    if let Some(paint) = PAINT.get() {
        if kind == FONT_LIST && message == REPAINT_FONT_FRAME {
            if unsafe { IsWindowVisible(window) } != 0 {
                paint_font_list_frame(window, paint);
            }
            return 0;
        }
        if message == WM_NCPAINT {
            let painted = if kind == FONT_LIST {
                paint_font_list_frame(window, paint)
            } else {
                paint_window_frame(window, paint.border)
            };
            if painted {
                return 0;
            }
        }
        if field {
            match message {
                WM_ERASEBKGND => return 1,
                WM_PAINT => {
                    buffered_paint(window, |dc, rect| {
                        paint_choice(window, dc, rect, paint, editable)
                    });
                    return 0;
                }
                WM_PRINTCLIENT => {
                    paint_choice(window, wp as Handle, bounds(window), paint, editable);
                    return 0;
                }
                WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX if editable => {
                    return choice_colors(wp as Handle, paint);
                }
                _ => {}
            }
        }
    }
    let result = unsafe { DefSubclassProc(window, message, wp, lp) };
    if kind == FONT_LIST
        && matches!(
            message,
            WM_PAINT
                | WM_SHOWWINDOW
                | WM_NCMOUSEMOVE
                | WM_NCLBUTTONDOWN
                | WM_LBUTTONUP
                | WM_VSCROLL
                | WM_MOUSEWHEEL
                | WM_KEYDOWN
        )
        && unsafe { IsWindowVisible(window) } != 0
        && let Some(paint) = PAINT.get()
    {
        paint_font_list_frame(window, paint);
    }
    if field
        && matches!(
            message,
            WM_SETFOCUS | WM_KILLFOCUS | SHOW_DROPDOWN | SET_SELECTION
        )
    {
        repaint(window);
    }
    result
}

fn choice_colors(dc: Handle, paint: Paint) -> Handle {
    unsafe {
        SetTextColor(dc, paint.foreground);
        SetBkColor(dc, paint.control_rgb);
    }
    paint.control
}

extern "system" fn procedure(window: Handle, message: u32, wp: usize, lp: isize) -> isize {
    match message {
        WM_ERASEBKGND => return 1,
        WM_PAINT => {
            buffered_paint(window, |dc, area| {
                if let Some(paint) = PAINT.get() {
                    paint_settings(dc, area, paint);
                }
            });
            return 0;
        }
        WM_DRAWITEM => {
            if let Some(paint) = PAINT.get() {
                let item = unsafe { &*(lp as *const DrawItem) };
                let id = item.id as i32;
                let button = matches!(id, APPLY | CLOSE);
                if !button && item.state & 0x1000 != 0 {
                    // ODS_COMBOBOXEDIT
                    repaint(item.window);
                    return 1;
                }
                let selected = item.state & 1 != 0;
                let base = if selected || id == APPLY {
                    paint.selected_rgb
                } else {
                    paint.control_rgb
                };
                if button {
                    fill(item.dc, item.rect, paint.background);
                    fill_rounded(item.dc, item.rect, (4 * paint.dpi / 96).max(1), base);
                } else {
                    fill_color(item.dc, item.rect, base);
                }
                let rect = if button {
                    item.rect
                } else {
                    Rect {
                        left: item.rect.left + 7,
                        ..item.rect
                    }
                };
                let value = if button {
                    caption(item.window)
                } else {
                    combo_label(item.window, item.item as usize)
                };
                draw_label(
                    item.dc,
                    rect,
                    &value,
                    paint.font,
                    if id == APPLY || selected {
                        paint.accent
                    } else {
                        paint.foreground
                    },
                    button,
                );
            }
            return 1;
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            if let Some(paint) = PAINT.get() {
                return choice_colors(wp as Handle, paint);
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
        WM_COMMAND
            if wp & 0xffff == FONT_ROW as usize
                && matches!(wp >> 16, CBN_EDITCHANGE | CBN_DROPDOWN)
                && lp != 0 =>
        {
            let font = lp as Handle;
            let text = caption(combo_info(font).edit);
            update_font_choices(font, &text, wp >> 16 == CBN_EDITCHANGE);
            return 0;
        }
        WM_COMMAND if matches!(wp >> 16, 1 | 3 | 4 | CBN_DROPDOWN | 8) && lp != 0 => {
            // Selection, focus, and dropdown changes can affect the themed field.
            repaint(lp as Handle);
            return 0;
        }
        WM_DESTROY => {
            FONT_NAMES.with_borrow_mut(|names| *names = Vec::new());
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
    send(control, 0x014b, 0, 0);
    for choice in choices {
        add_choice(control, choice);
    }
    send(control, SET_SELECTION, selected, 0);
}

fn add_choice(control: Handle, choice: &str) {
    let text = choice.encode_utf16().chain([0]).collect::<Vec<_>>();
    send(control, 0x0143, 0, text.as_ptr() as isize);
}

fn update_font_choices(control: Handle, value: &str, filtering: bool) {
    if CHANGING_FONT_CHOICES.replace(true) {
        return;
    }
    let info = combo_info(control);
    let caret = send(control, GET_EDIT_SELECTION, 0, 0);
    let query = if filtering {
        value.trim().to_lowercase()
    } else {
        String::new()
    };
    let open = send(control, DROPDOWN_OPEN, 0, 0) != 0;
    let list_visible = unsafe { IsWindowVisible(info.list) != 0 };
    if list_visible {
        painting(info.list, false);
    }
    send(control, 0x014b, 0, 0);
    let mut count = 0;
    FONT_NAMES.with_borrow(|names| {
        for name in names
            .iter()
            .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
        {
            add_choice(control, name);
            count += 1;
        }
    });
    if list_visible {
        painting(info.list, true);
        repaint(info.list);
    }
    if filtering && open != (count > 0) {
        send(control, SHOW_DROPDOWN, usize::from(count > 0), 0);
    }
    title(info.edit, value);
    send(control, SET_EDIT_SELECTION, 0, caret);
    // The combo box may issue a native scrollbar paint while opening its popup.
    unsafe { PostMessageW(info.list, REPAINT_FONT_FRAME, 0, 0) };
    repaint(control);
    CHANGING_FONT_CHOICES.set(false);
}

impl App {
    pub(super) fn open_font_preferences(&mut self) -> io::Result<()> {
        self.open_preferences()?;
        let combo = field(self.preferences_window, FONT_ROW);
        unsafe { SetFocus(combo) };
        send(combo, SHOW_DROPDOWN, 1, 0);
        Ok(())
    }

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
            i18n::text("settings.window"),
            self.window,
            Point {
                x: self.px(572),
                y: self.px(555),
            },
        )?;
        if let Err(error) = self.build_preferences(window) {
            unsafe {
                DestroyWindow(window);
            }
            return Err(error);
        }
        self.preferences_window = window;
        FONT_NAMES.with_borrow_mut(|names| *names = font_families(window));
        self.refresh_preferences(window);
        show_main_window(window, false);
        unsafe {
            SetFocus(field(window, LANGUAGE_ROW));
        }
        Ok(())
    }

    fn build_preferences(&self, window: Handle) -> io::Result<()> {
        let add = |id, class: &str, style, rect: Rect| {
            let control = child(window, id as usize, class, "", style | WS_TABSTOP)?;
            place(
                control,
                self.px(rect.left),
                self.px(rect.top),
                self.px(rect.width()),
                self.px(rect.height()),
            );
            show(control, true);
            Ok::<_, io::Error>(control)
        };
        for (id, y, _) in ROWS {
            if id == SIZE_ROW {
                let control = add(
                    id,
                    "EDIT",
                    WS_BORDER | ES_AUTOHSCROLL,
                    Rect::from_size(284, y + 2, 238, 23),
                )?;
                send(control, 0x00c5, 2, 0);
                send(control, 0x00d3, 3, 6 | (6 << 16));
            } else {
                let editable = id == FONT_ROW;
                const WS_CLIPCHILDREN: u32 = 0x02000000;
                // Editable CBS_DROPDOWN or CBS_DROPDOWNLIST, both owner-drawn.
                let kind = if editable {
                    2 | WS_CLIPCHILDREN | WS_VSCROLL | ES_AUTOHSCROLL
                } else {
                    3
                };
                let style = kind | 0x10 | 0x200;
                let control = add(id, "COMBOBOX", style, Rect::from_size(284, y, 238, 245))?;
                send(control, 0x0153, usize::MAX, self.px(25) as isize);
                send(control, 0x0153, 0, self.px(24) as isize);
                subclass(
                    control,
                    choice_proc,
                    if editable { FONT_CHOICE } else { SELECT_ONLY },
                );
                let info = combo_info(control);
                if info.list != 0 {
                    subclass(
                        info.list,
                        choice_proc,
                        if editable { FONT_LIST } else { DROPDOWN_LIST },
                    );
                }
                if editable {
                    send(control, 0x0141, 31, 0); // CB_LIMITTEXT
                    send(info.edit, 0x00d3, 3, 5 | (5 << 16));
                }
            }
        }
        add(
            CLOSE,
            "BUTTON",
            BS_OWNERDRAW,
            Rect::from_size(328, 479, 90, 29),
        )?;
        add(
            APPLY,
            "BUTTON",
            BS_OWNERDRAW,
            Rect::from_size(432, 479, 90, 29),
        )?;
        Ok(())
    }

    fn refresh_preferences(&self, window: Handle) {
        title(window, i18n::text("settings.window"));
        let font = field(window, FONT_ROW);
        update_font_choices(font, &self.settings.font, false);
        edit_hint(combo_info(font).edit, i18n::text("settings.search_fonts"));
        title(field(window, SIZE_ROW), &self.settings.points.to_string());
        title(field(window, APPLY), i18n::text("settings.apply"));
        title(field(window, CLOSE), i18n::text("settings.close"));
        set_combo(
            field(window, LANGUAGE_ROW),
            &[
                i18n::text("settings.korean"),
                i18n::text("settings.english"),
            ],
            usize::from(self.settings.locale == i18n::Locale::English),
        );
        let themes = Theme::ALL.map(|theme| i18n::text(theme.label_key()));
        let current = Theme::ALL
            .iter()
            .position(|&theme| theme == self.settings.theme)
            .unwrap_or(0);
        set_combo(field(window, THEME_ROW), &themes, current);
        set_combo(
            field(window, WRAP_ROW),
            &[i18n::text("settings.off"), i18n::text("settings.on")],
            usize::from(self.settings.wrap),
        );
        set_combo(
            field(window, SIDEBAR_ROW),
            &[i18n::text("settings.left"), i18n::text("settings.right")],
            usize::from(self.settings.sidebar_right),
        );
        let spaces: Vec<_> = (1..=8)
            .map(|n| i18n::message("settings.spaces", &[n.to_string()]))
            .collect();
        let choices: Vec<_> = std::iter::once(i18n::text("settings.tab"))
            .chain(spaces.iter().map(String::as_str))
            .collect();
        set_combo(field(window, INDENT_ROW), &choices, self.settings.spaces);
        if let Some(paint) = PAINT.get() {
            title_theme(
                window,
                self.settings.dark(),
                paint.chrome_rgb,
                paint.foreground,
            );
            for id in THEME_ROW..=LANGUAGE_ROW {
                set_font(field(window, id), paint.font);
                repaint(field(window, id));
            }
        }
        repaint(window);
    }

    fn save_preferences(&mut self, window: Handle) -> io::Result<()> {
        let selected = |id| send(field(window, id), GET_SELECTION, 0, 0);
        let font = caption(combo_info(field(window, FONT_ROW)).edit);
        let font = font.trim();
        if font.is_empty() || font.chars().any(char::is_control) {
            return Err(io::Error::other(i18n::text("settings.invalid_font")));
        }
        let size = caption(field(window, SIZE_ROW))
            .parse::<i32>()
            .ok()
            .filter(|size| (8..=36).contains(size))
            .ok_or_else(|| io::Error::other(i18n::text("settings.invalid_size")))?;
        let mut settings = self.settings.clone();
        settings.locale = if selected(LANGUAGE_ROW) == 1 {
            i18n::Locale::English
        } else {
            i18n::Locale::Korean
        };
        settings.font = font.to_owned();
        settings.points = size;
        settings.theme = Theme::ALL
            .get(selected(THEME_ROW).max(0) as usize)
            .copied()
            .unwrap_or_default();
        settings.wrap = selected(WRAP_ROW) == 1;
        settings.spaces = selected(INDENT_ROW).clamp(0, 8) as usize;
        settings.sidebar_right = selected(SIDEBAR_ROW) == 1;
        settings.save()?;
        self.settings = settings;
        i18n::select(self.settings.locale);
        self.refresh_locale_labels();
        self.apply_preferences();
        self.refresh_preferences(window);
        Ok(())
    }
}
