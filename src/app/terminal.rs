mod pty;
mod screen;

use super::*;
use crate::text::TerminalDock;
use motion::{Area, Effect, Key};
use pty::{Event, Session, Shell, Size};
use screen::{Ink, Screen, Style};

pub(super) const TOGGLE: usize = 140;
pub(super) const NEW: usize = 141;
pub(super) const NEW_CMD: usize = 142;
pub(super) const DOCK_BOTTOM: usize = 143;
pub(super) const DOCK_LEFT: usize = 144;
pub(super) const DOCK_RIGHT: usize = 145;
pub(super) const CLOSE_TAB: usize = 146;
pub(super) const MENU: usize = 147;
pub(super) const TIMER: usize = 3;
const HEADER: usize = 340;
const BODY: usize = 341;
const DIVIDER: usize = 342;
const CLASS: &str = "MakerTerminalPanel";
const WM_CHAR: u32 = 0x0102;
const WM_SETFOCUS: u32 = 7;
const WM_KILLFOCUS: u32 = 8;
const WM_RBUTTONUP: u32 = 0x0205;

type CellPos = (usize, usize);

pub(super) struct Tab {
    label: String,
    shell: Shell,
    directory: PathBuf,
    pending: bool,
    screen: Screen,
    session: Option<Session>,
    scroll: usize,
    selection: Option<(CellPos, CellPos)>,
}

#[derive(Default)]
pub(super) struct SavedPanel {
    pub(super) visible: bool,
    pub(super) active: usize,
    pub(super) tabs: Vec<(u8, PathBuf)>,
}

impl Tab {
    fn resize(&mut self, size: Size) {
        if self.screen.shape() == (size.cols as usize, size.rows as usize) {
            return;
        }
        self.screen.resize(size.cols as usize, size.rows as usize);
        self.scroll = self.scroll.min(self.screen.history());
        self.selection = None;
        if let Some(session) = &self.session {
            session.resize(size);
        }
    }

    fn send(&mut self, bytes: Vec<u8>) {
        if let Some(session) = &self.session {
            session.send(bytes);
            self.scroll = 0;
            self.selection = None;
        }
    }

    fn drain(&mut self) -> bool {
        let mut updated = false;
        // Bound the work per tab so sustained output does not block native input or painting.
        for _ in 0..12 {
            let Some(session) = &self.session else {
                break;
            };
            match session.output.try_recv() {
                Ok(Event::Data(bytes)) => {
                    let before = self.screen.history();
                    let response = self.screen.feed(&bytes);
                    if !response.is_empty() {
                        session.send(response);
                    }
                    let current = self.screen.history();
                    if self.scroll > 0 {
                        self.scroll = (self.scroll + current.saturating_sub(before)).min(current);
                    }
                    updated = true;
                }
                Ok(Event::Exit(code)) => {
                    self.screen
                        .note(&format!("셸이 종료되었습니다. (코드 {code})"));
                    self.session = None;
                    updated = true;
                }
                Ok(Event::Error(error)) => {
                    self.screen
                        .note(&format!("셸을 시작할 수 없습니다. {error}"));
                    self.session = None;
                    updated = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.screen.note("셸 연결이 종료되었습니다.");
                    self.session = None;
                    updated = true;
                }
            }
        }
        updated
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection?;
        if start == end {
            return None;
        }
        let (start, end) = (start.min(end), start.max(end));
        let mut content = Vec::new();
        for row in start.0..=end.0 {
            let Some(line) = self.screen.line(row) else {
                continue;
            };
            let left = if row == start.0 { start.1 } else { 0 }.min(line.len());
            let right = if row == end.0 { end.1 + 1 } else { line.len() }.min(line.len());
            let segment = if left < right {
                let left = if line[left].width == 0 {
                    left.saturating_sub(1)
                } else {
                    left
                };
                &line[left..right]
            } else {
                &[]
            };
            let mut text = String::new();
            for cell in segment.iter().filter(|cell| cell.width > 0) {
                text.push(cell.ch);
                text.push_str(&cell.marks);
            }
            content.push(text.trim_end_matches(' ').to_owned());
        }
        Some(content.join("\r\n"))
    }
}

#[derive(Clone, Copy)]
struct PanelDrag {
    anchor: Point,
    dock: TerminalDock,
    value: i32,
}

#[derive(Clone, Copy)]
enum BodyDrag {
    Text(CellPos),
    History,
}

pub(super) struct Panel {
    header: Handle,
    body: Handle,
    divider: Handle,
    pub(super) visible: bool,
    area: Rect,
    font: Handle,
    cell: Point,
    tabs: Vec<Tab>,
    active: usize,
    next_number: u64,
    first_tab: usize,
    hover: Option<Point>,
    tab_drag: Option<(usize, Point)>,
    divider_hover: bool,
    panel_drag: Option<PanelDrag>,
    body_drag: Option<BodyDrag>,
    high_surrogate: Option<u16>,
}

impl Drop for Panel {
    fn drop(&mut self) {
        if self.font != 0 {
            unsafe {
                DeleteObject(self.font);
            }
        }
    }
}

impl Panel {
    pub(super) fn saved(&self) -> SavedPanel {
        SavedPanel {
            visible: self.visible,
            active: self.active,
            tabs: self
                .tabs
                .iter()
                .take(500)
                .map(|tab| {
                    (
                        u8::from(matches!(tab.shell, Shell::CommandPrompt)),
                        tab.directory.clone(),
                    )
                })
                .collect(),
        }
    }
    pub(super) fn new(window: Handle) -> io::Result<Self> {
        Ok(Self {
            header: child(window, HEADER, CLASS, "터미널 탭", 0)?,
            body: child(window, BODY, CLASS, "터미널", WS_TABSTOP)?,
            divider: child(window, DIVIDER, CLASS, "터미널 경계", 0)?,
            visible: false,
            area: Rect::default(),
            font: 0,
            cell: Point { x: 8, y: 18 },
            tabs: Vec::new(),
            active: 0,
            next_number: 0,
            first_tab: 0,
            hover: None,
            tab_drag: None,
            divider_hover: false,
            panel_drag: None,
            body_drag: None,
            high_surrogate: None,
        })
    }

    pub(super) fn font(&mut self, dpi: i32) {
        let font = named_font("Consolas", 15 * dpi / 96);
        if self.font != 0 {
            unsafe {
                DeleteObject(self.font);
            }
        }
        self.font = font;
        let measured = cell_size(self.body, font);
        self.cell = Point {
            x: measured.x.max(1),
            y: (measured.y + dpi / 96).max(1),
        };
    }

    pub(super) fn body(&self) -> Handle {
        self.body
    }

    fn active(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }
    fn active_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    fn inner_size(&self, dpi: i32) -> Size {
        let rect = bounds(self.body);
        Size::new(
            (rect.width() - 26 * dpi / 96) / self.cell.x,
            (rect.height() - 12 * dpi / 96) / self.cell.y,
        )
    }

    fn controls(&self, rect: Rect, dpi: i32) -> (i32, i32, i32) {
        let action = 29 * dpi / 96;
        let name = if rect.width() >= 330 * dpi / 96 {
            68 * dpi / 96
        } else {
            0
        };
        let tabs = rect.width().saturating_sub(3 * action + name).max(0);
        (name, tabs, action)
    }

    fn tab_width(&self, rect: Rect, dpi: i32) -> (i32, usize) {
        let (_, room, _) = self.controls(rect, dpi);
        let width = (room / self.tabs.len().max(1) as i32).clamp(94 * dpi / 96, 146 * dpi / 96);
        (width, (room / width).max(1) as usize)
    }

    fn reveal_tab(&mut self, dpi: i32) {
        let (_, visible) = self.tab_width(bounds(self.header), dpi);
        if self.active < self.first_tab {
            self.first_tab = self.active;
        }
        if self.active >= self.first_tab + visible {
            self.first_tab = self.active + 1 - visible;
        }
        self.first_tab = self.first_tab.min(self.tabs.len().saturating_sub(visible));
    }
}

pub(super) fn register() -> io::Result<()> {
    register_class(CLASS, proc)
}

extern "system" fn proc(window: Handle, msg: u32, wp: usize, lp: isize) -> isize {
    let id = unsafe { GetDlgCtrlID(window) } as usize;
    match msg {
        WM_PAINT | WM_PRINTCLIENT => {
            let draw = |dc, rect| {
                if let Some(palette) = PAINT.get() {
                    fill(dc, rect, palette.background);
                    with_app(|app| app.paint_terminal(id, dc, rect, palette));
                }
            };
            if msg == WM_PAINT {
                if id == HEADER {
                    buffered_paint(window, draw);
                } else {
                    paint(window, draw);
                }
            } else {
                draw(wp as Handle, bounds(window));
            }
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_SETCURSOR if id != HEADER => {
            if id == BODY {
                cursor(32513);
            } else {
                let dock =
                    with_app(|app| app.settings.terminal_dock).unwrap_or(TerminalDock::Bottom);
                cursor(if dock == TerminalDock::Bottom {
                    32645
                } else {
                    32644
                });
            }
            return 1;
        }
        WM_CHAR if id == BODY => {
            with_app(|app| app.terminal_char(wp as u16));
            return 0;
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            redraw(window);
        }
        WM_MOUSEMOVE | WM_MOUSELEAVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK
        | WM_MBUTTONDOWN | WM_MOUSEWHEEL | WM_CAPTURECHANGED | WM_RBUTTONUP => {
            with_app(|app| app.terminal_mouse(id, window, msg, wp, Point::packed(lp)));
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(window, msg, wp, lp) }
}

impl App {
    pub(super) fn rebase_terminal_paths(&mut self, rebase: impl Fn(&Path) -> Option<PathBuf>) {
        for tab in &mut self.terminal.tabs {
            if let Some(path) = rebase(&tab.directory) {
                tab.directory = path;
            }
        }
    }
    pub(super) fn restore_terminal(&mut self, state: SavedPanel) {
        for (shell, directory) in state.tabs {
            let shell = if shell == 1 {
                Shell::CommandPrompt
            } else {
                Shell::PowerShell
            };
            if directory.is_dir() {
                self.push_terminal(shell, directory, true);
            }
        }
        if self.terminal.tabs.is_empty() {
            return;
        }
        self.terminal.visible = state.visible;
        let active = state.active.min(self.terminal.tabs.len() - 1);
        self.terminal.active = active;
        self.layout();
        self.activate_terminal(active);
    }

    pub(super) fn terminal_layout(&mut self, area: Rect) -> Rect {
        self.terminal.area = area;
        if !self.terminal.visible {
            for handle in [
                self.terminal.header,
                self.terminal.body,
                self.terminal.divider,
            ] {
                show(handle, false);
            }
            return area;
        }
        let divider = self.px(6);
        let header = self.px(31);
        let dock = self.settings.terminal_dock;
        let size = self.terminal_extent(dock, area);
        let fills_window = self.detached && self.documents.is_empty();
        let (panel, edge, editor) = match dock {
            _ if fills_window => (
                area,
                Rect::default(),
                Rect {
                    right: area.left,
                    bottom: area.top,
                    ..area
                },
            ),
            TerminalDock::Bottom => {
                let split = area.bottom - size;
                (
                    Rect { top: split, ..area },
                    Rect::from_size(area.left, split - divider, area.width(), divider),
                    Rect {
                        bottom: split - divider,
                        ..area
                    },
                )
            }
            TerminalDock::Left => {
                let split = area.left + size;
                (
                    Rect {
                        right: split,
                        ..area
                    },
                    Rect::from_size(split, area.top, divider, area.height()),
                    Rect {
                        left: split + divider,
                        ..area
                    },
                )
            }
            TerminalDock::Right => {
                let split = area.right - size;
                (
                    Rect {
                        left: split,
                        ..area
                    },
                    Rect::from_size(split - divider, area.top, divider, area.height()),
                    Rect {
                        right: split - divider,
                        ..area
                    },
                )
            }
        };
        place(
            self.terminal.header,
            panel.left,
            panel.top,
            panel.width(),
            header.min(panel.height()),
        );
        place(
            self.terminal.body,
            panel.left,
            panel.top + header,
            panel.width(),
            panel.height() - header,
        );
        show(self.terminal.header, true);
        show(self.terminal.body, panel.height() > header);
        if fills_window {
            show(self.terminal.divider, false);
        } else {
            show_overlay(self.terminal.divider, edge);
        }
        self.terminal.reveal_tab(self.dpi);
        let size = self.terminal.inner_size(self.dpi);
        if let Some(tab) = self.terminal.active_mut() {
            tab.resize(size);
        }
        self.redraw_terminal();
        editor
    }

    fn terminal_extent(&self, dock: TerminalDock, area: Rect) -> i32 {
        let height = dock == TerminalDock::Bottom;
        let extent = if height { area.height() } else { area.width() };
        let preferred = self.px(if height {
            self.settings.terminal_height
        } else {
            self.settings.terminal_width
        });
        let min = self.px(if height { 112 } else { 196 });
        let remaining = self.px(if height { 105 } else { 170 });
        let max = (extent - remaining).max(0).max(extent / 2);
        preferred.clamp(min.min(max), max)
    }

    pub(super) fn terminal_command(&mut self, id: usize) -> io::Result<()> {
        match id {
            TOGGLE if self.terminal.visible => self.hide_terminal(),
            TOGGLE if !self.terminal.tabs.is_empty() => {
                self.terminal.visible = true;
                self.layout();
                self.activate_terminal(self.terminal.active);
            }
            TOGGLE | NEW => self.add_terminal(Shell::PowerShell)?,
            NEW_CMD => self.add_terminal(Shell::CommandPrompt)?,
            CLOSE_TAB => self.close_terminal_tab(self.terminal.active),
            DOCK_BOTTOM | DOCK_LEFT | DOCK_RIGHT => {
                self.settings.terminal_dock = match id {
                    DOCK_LEFT => TerminalDock::Left,
                    DOCK_RIGHT => TerminalDock::Right,
                    _ => TerminalDock::Bottom,
                };
                self.layout();
                self.settings.save()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn add_terminal(&mut self, shell: Shell) -> io::Result<()> {
        let directory = self
            .root
            .clone()
            .or_else(|| {
                self.active
                    .and_then(|index| self.documents[index].path.as_deref())
                    .and_then(Path::parent)
                    .map(Path::to_owned)
            })
            .map_or_else(std::env::current_dir, Ok)?;
        self.terminal.visible = true;
        self.layout();
        self.push_terminal(shell, directory, true);
        self.activate_terminal(self.terminal.tabs.len() - 1);
        Ok(())
    }

    fn push_terminal(&mut self, shell: Shell, directory: PathBuf, pending: bool) {
        let size = self.terminal.inner_size(self.dpi);
        self.terminal.next_number += 1;
        self.terminal.tabs.push(Tab {
            label: format!("{} {}", shell.label(), self.terminal.next_number),
            shell,
            directory,
            pending,
            screen: Screen::new(size.cols as usize, size.rows as usize),
            session: None,
            selection: None,
            scroll: 0,
        });
    }

    fn activate_terminal(&mut self, index: usize) {
        let previous = self.terminal.active;
        self.terminal.active = index;
        self.terminal.high_surrogate = None;
        self.terminal.body_drag = None;
        self.terminal.reveal_tab(self.dpi);
        if previous != index {
            for tab in [previous, index] {
                if let Some(rect) = self.terminal_tab_cell(tab) {
                    motion::animate(
                        self.window,
                        Key::item(self.terminal.header, tab, Effect::Selected),
                        Area::Rect(rect),
                        tab == previous,
                        tab == index,
                    );
                }
            }
        }
        let size = self.terminal.inner_size(self.dpi);
        let visible = self.terminal.visible;
        if let Some(tab) = self.terminal.active_mut() {
            tab.resize(size);
            if visible && tab.pending {
                tab.pending = false;
                match Session::start(tab.shell, tab.directory.clone(), size) {
                    Ok(session) => {
                        tab.session = Some(session);
                        unsafe {
                            SetTimer(self.window, TIMER, 42, 0);
                        }
                    }
                    Err(error) => tab.screen.note(&error.to_string()),
                }
            }
        }
        self.focus_terminal();
        self.redraw_terminal();
        self.queue_session_save();
    }

    fn close_terminal_tab(&mut self, index: usize) {
        if index >= self.terminal.tabs.len() {
            return;
        }
        self.terminal.tabs.remove(index);
        self.after_terminal_removed(index);
    }

    fn detach_terminal(
        &mut self,
        index: usize,
        destination: Option<dock::DropTarget>,
    ) -> io::Result<()> {
        let sender = if destination.is_none() {
            Some(self.new_window_sender()?)
        } else {
            None
        };
        let tab = self.terminal.tabs.remove(index);
        let transfer = dock::Transfer::Terminal(Box::new(tab));
        let result = if let Some(target) = destination {
            target.send(transfer, None)
        } else {
            sender.unwrap().send(transfer).map_err(|error| error.0)
        };
        match result {
            Ok(()) => {
                self.after_terminal_removed(index);
                Ok(())
            }
            Err(dock::Transfer::Terminal(tab)) => {
                self.terminal.tabs.insert(index, *tab);
                Err(io::Error::other("터미널을 대상 창으로 옮길 수 없습니다."))
            }
            Err(_) => unreachable!("only terminal transfers sent here"),
        }
    }

    pub(super) fn accept_terminal(&mut self, tab: Tab) {
        self.terminal.visible = true;
        self.terminal.next_number += 1;
        self.terminal.tabs.push(tab);
        self.layout();
        self.activate_terminal(self.terminal.tabs.len() - 1);
        unsafe {
            SetTimer(self.window, TIMER, 30, 0);
        }
    }

    fn after_terminal_removed(&mut self, index: usize) {
        motion::clear(self.terminal.header);
        if self.terminal.tabs.is_empty() {
            self.terminal.active = 0;
            self.terminal.first_tab = 0;
            self.hide_terminal();
            unsafe {
                KillTimer(self.window, TIMER);
            }
        } else {
            let active = self.terminal.active;
            let active = if active > index { active - 1 } else { active };
            self.activate_terminal(active.min(self.terminal.tabs.len() - 1));
        }
        self.queue_session_save();
    }

    fn hide_terminal(&mut self) {
        self.terminal.visible = false;
        self.terminal.high_surrogate = None;
        self.layout();
        self.focus_editor();
        self.queue_session_save();
    }

    pub(super) fn focus_terminal(&self) {
        if self.terminal.visible {
            unsafe {
                SetFocus(self.terminal.body);
            }
        }
    }

    fn redraw_terminal(&self) {
        if self.terminal.visible {
            redraw(self.terminal.header);
            redraw(self.terminal.body);
            redraw(self.terminal.divider);
        }
    }

    pub(super) fn terminal_tick(&mut self) {
        let mut changed = false;
        let mut active = false;
        for tab in &mut self.terminal.tabs {
            changed |= tab.drain();
            active |= tab.session.is_some();
        }
        if changed {
            self.redraw_terminal();
        }
        if !active {
            unsafe {
                KillTimer(self.window, TIMER);
            }
        }
    }

    pub(super) fn terminal_key(&mut self, target: Handle, key: i32) -> bool {
        let down = |vk| unsafe { GetKeyState(vk) < 0 };
        let ctrl = down(0x11);
        let shift = down(0x10);
        let alt = down(0x12);
        if ctrl && !alt && key == 0xc0 {
            self.command(if shift { NEW } else { TOGGLE });
            return true;
        }
        if target != self.terminal.body {
            return false;
        }
        if ctrl && !alt {
            match key {
                0x73 => {
                    self.command(CLOSE_TAB);
                    return true;
                }
                9 if !self.terminal.tabs.is_empty() => {
                    let len = self.terminal.tabs.len();
                    let index = if shift {
                        self.terminal.active + len - 1
                    } else {
                        self.terminal.active + 1
                    };
                    self.activate_terminal(index % len);
                    return true;
                }
                0x43 if shift => {
                    self.copy_terminal();
                    return true;
                }
                0x56 => {
                    self.paste_terminal();
                    return true;
                }
                _ => {}
            }
        }
        if shift && matches!(key, 0x21 | 0x22) {
            let direction = if key == 0x21 { 1 } else { -1 };
            self.terminal_scroll(direction * 12);
            return true;
        }
        let Some(tab) = self.terminal.active_mut() else {
            return false;
        };
        let app = tab.screen.application_cursor;
        let sequence: Option<&[u8]> = match key {
            0x25 => Some(if app { b"\x1bOD" } else { b"\x1b[D" }),
            0x26 => Some(if app { b"\x1bOA" } else { b"\x1b[A" }),
            0x27 => Some(if app { b"\x1bOC" } else { b"\x1b[C" }),
            0x28 => Some(if app { b"\x1bOB" } else { b"\x1b[B" }),
            0x24 => Some(b"\x1b[H"),
            0x23 => Some(b"\x1b[F"),
            0x21 => Some(b"\x1b[5~"),
            0x22 => Some(b"\x1b[6~"),
            0x2d if shift => {
                self.paste_terminal();
                return true;
            }
            0x2d => Some(b"\x1b[2~"),
            0x2e => Some(b"\x1b[3~"),
            0x70 => Some(b"\x1bOP"),
            0x71 => Some(b"\x1bOQ"),
            0x72 => Some(b"\x1bOR"),
            0x73 => Some(b"\x1bOS"),
            0x74 => Some(b"\x1b[15~"),
            0x75 => Some(b"\x1b[17~"),
            0x76 => Some(b"\x1b[18~"),
            0x77 => Some(b"\x1b[19~"),
            0x78 => Some(b"\x1b[20~"),
            0x79 => Some(b"\x1b[21~"),
            0x7a => Some(b"\x1b[23~"),
            0x7b => Some(b"\x1b[24~"),
            9 if shift => Some(b"\x1b[Z"),
            _ => None,
        };
        if let Some(bytes) = sequence {
            tab.send(bytes.to_vec());
            redraw(self.terminal.body);
            return true;
        }
        false
    }

    fn terminal_char(&mut self, unit: u16) {
        if (0xd800..=0xdbff).contains(&unit) {
            self.terminal.high_surrogate = Some(unit);
            return;
        }
        let high = self.terminal.high_surrogate.take();
        let value = if let Some(high) = high.filter(|_| (0xdc00..=0xdfff).contains(&unit)) {
            char::from_u32(
                0x10000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(unit) - 0xdc00),
            )
        } else {
            char::from_u32(u32::from(unit))
        };
        if let Some(tab) = self.terminal.active_mut()
            && let Some(ch) = value
        {
            let ch = if ch == '\x08' { '\x7f' } else { ch };
            tab.send(ch.encode_utf8(&mut [0; 4]).as_bytes().to_vec());
            redraw(self.terminal.body);
        }
    }

    fn copy_terminal(&self) -> bool {
        let Some(text) = self.terminal.active().and_then(Tab::selected_text) else {
            return false;
        };
        copy_text(self.window, &text);
        true
    }

    fn paste_terminal(&mut self) {
        let Some(text) = clipboard_text(self.window) else {
            return;
        };
        let Some(tab) = self.terminal.active_mut() else {
            return;
        };
        let normalized = text
            .replace("\r\n", "\r")
            .replace('\n', "\r")
            .replace(['\x00', '\x1b'], "");
        let paste = if tab.screen.bracketed_paste {
            let normalized = normalized.replace('\r', "\n");
            format!("\x1b[200~{normalized}\x1b[201~")
        } else {
            normalized
        };
        tab.send(paste.into_bytes());
        redraw(self.terminal.body);
    }

    fn terminal_scroll(&mut self, delta: i32) {
        if let Some(tab) = self.terminal.active_mut() {
            tab.scroll = (tab.scroll as i32 + delta).clamp(0, tab.screen.history() as i32) as usize;
            redraw(self.terminal.body);
        }
    }

    fn terminal_mouse(&mut self, id: usize, window: Handle, msg: u32, wp: usize, point: Point) {
        if msg == WM_MOUSEMOVE {
            watch_mouse(window);
        }
        match id {
            DIVIDER => self.terminal_divider_mouse(window, msg, point),
            HEADER => self.terminal_header_mouse(msg, wp, point),
            BODY => self.terminal_body_mouse(msg, wp, point),
            _ => {}
        }
    }

    fn terminal_divider_mouse(&mut self, window: Handle, msg: u32, point: Point) {
        let previous = self.terminal.divider_hover || self.terminal.panel_drag.is_some();
        let point = map_point(window, self.window, point);
        let area = self.terminal.area;
        let dock = self.settings.terminal_dock;
        match msg {
            WM_LBUTTONDOWN => {
                self.terminal.panel_drag = Some(PanelDrag {
                    anchor: point,
                    dock,
                    value: self.terminal_extent(dock, area),
                });
                unsafe {
                    SetCapture(window);
                }
            }
            WM_MOUSEMOVE => {
                self.terminal.divider_hover = true;
                if let Some(drag) = self.terminal.panel_drag {
                    let x = point.x - area.left;
                    let y = point.y - area.top;
                    let target = if drag.dock != TerminalDock::Bottom && y > area.height() * 4 / 5 {
                        TerminalDock::Bottom
                    } else if drag.dock != TerminalDock::Left
                        && x < area.width() / 6
                        && (drag.dock != TerminalDock::Bottom
                            || (point.x - drag.anchor.x).abs() > self.px(72))
                    {
                        TerminalDock::Left
                    } else if drag.dock != TerminalDock::Right
                        && x > area.width() * 5 / 6
                        && (drag.dock != TerminalDock::Bottom
                            || (point.x - drag.anchor.x).abs() > self.px(72))
                    {
                        TerminalDock::Right
                    } else {
                        drag.dock
                    };
                    self.settings.terminal_dock = target;
                    if target == drag.dock {
                        let delta = match target {
                            TerminalDock::Bottom => drag.anchor.y - point.y,
                            TerminalDock::Left => point.x - drag.anchor.x,
                            TerminalDock::Right => drag.anchor.x - point.x,
                        };
                        let size = (drag.value + delta).max(0) * 96 / self.dpi;
                        match target {
                            TerminalDock::Bottom => {
                                self.settings.terminal_height = size.clamp(112, 1600)
                            }
                            _ => self.settings.terminal_width = size.clamp(196, 2400),
                        }
                    }
                    self.layout();
                }
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED if self.terminal.panel_drag.take().is_some() => {
                if msg == WM_LBUTTONUP {
                    unsafe {
                        ReleaseCapture();
                    }
                }
                self.save_sidebar_settings();
            }
            WM_LBUTTONDBLCLK => {
                let defaults = Settings::default();
                self.settings.terminal_height = defaults.terminal_height;
                self.settings.terminal_width = defaults.terminal_width;
                self.layout();
                self.save_sidebar_settings();
            }
            WM_MOUSELEAVE => self.terminal.divider_hover = false,
            _ => {}
        }
        let next = self.terminal.divider_hover || self.terminal.panel_drag.is_some();
        if previous != next {
            motion::animate(
                self.window,
                Key::control(window, Effect::Hover),
                Area::Control,
                previous,
                next,
            );
        }
    }

    fn terminal_tab_cell(&self, index: usize) -> Option<Rect> {
        let area = bounds(self.terminal.header);
        let (left, room, _) = self.terminal.controls(area, self.dpi);
        let (width, visible) = self.terminal.tab_width(area, self.dpi);
        let slot = index.checked_sub(self.terminal.first_tab)?;
        if slot >= visible || index >= self.terminal.tabs.len() {
            return None;
        }
        let x = left + slot as i32 * width;
        Some(Rect::from_size(
            x,
            0,
            width.min((left + room - x).max(0)),
            area.height(),
        ))
    }

    fn terminal_header_mouse(&mut self, msg: u32, wp: usize, point: Point) {
        let header = self.terminal.header;
        let rect = bounds(header);
        if let Some((index, origin)) = self.terminal.tab_drag {
            let moved = (point.x - origin.x).abs().max((point.y - origin.y).abs()) >= self.px(7);
            match msg {
                WM_MOUSEMOVE if moved => {
                    if rect.has(point) || self.transfer_target(header, point).is_some() {
                        self.clear_dock();
                    } else {
                        self.show_dock(dock::Destination::Window);
                    }
                }
                WM_LBUTTONUP => {
                    self.terminal.tab_drag = None;
                    self.clear_dock();
                    unsafe {
                        ReleaseCapture();
                    }
                    if moved
                        && !rect.has(point)
                        && index < self.terminal.tabs.len()
                        && let Err(error) =
                            self.detach_terminal(index, self.transfer_target(header, point))
                    {
                        self.show_error(error);
                    }
                    return;
                }
                WM_CAPTURECHANGED => {
                    self.terminal.tab_drag = None;
                    self.clear_dock();
                }
                _ => {}
            }
        }
        let (left, room, action) = self.terminal.controls(rect, self.dpi);
        let (width, visible) = self.terminal.tab_width(rect, self.dpi);
        match msg {
            WM_MOUSEMOVE | WM_MOUSELEAVE => {
                let hit = |point: Point| {
                    if !rect.has(point) {
                        return None;
                    }
                    if point.x >= rect.width() - 3 * action {
                        let slot = (point.x - (rect.width() - 3 * action)) / action;
                        let area = Rect::from_size(
                            rect.right - (3 - slot) * action,
                            0,
                            action,
                            rect.height(),
                        );
                        Some((usize::MAX - slot as usize, area))
                    } else if (left..left + room).contains(&point.x) {
                        let index = self.terminal.first_tab + ((point.x - left) / width) as usize;
                        self.terminal_tab_cell(index).map(|area| (index, area))
                    } else {
                        None
                    }
                };
                let previous = self.terminal.hover.and_then(hit);
                let next_point = (msg == WM_MOUSEMOVE).then_some(point);
                let next = next_point.and_then(hit);
                self.terminal.hover = next_point;
                if previous != next {
                    for (part, rect) in previous.into_iter().chain(next) {
                        let on = next.is_some_and(|(index, _)| index == part);
                        motion::animate(
                            self.window,
                            Key::item(self.terminal.header, part, Effect::Hover),
                            Area::Rect(rect),
                            !on,
                            on,
                        );
                    }
                }
                return;
            }
            WM_MOUSEWHEEL => {
                motion::clear(self.terminal.header);
                let direction = ((wp >> 16) as i16).signum() as isize;
                let max = self.terminal.tabs.len().saturating_sub(visible);
                self.terminal.first_tab = self
                    .terminal
                    .first_tab
                    .saturating_add_signed(-direction)
                    .min(max);
            }
            WM_LBUTTONDOWN | WM_MBUTTONDOWN => {
                if point.x >= rect.width() - 3 * action && msg == WM_LBUTTONDOWN {
                    let hit = (point.x - (rect.width() - 3 * action)) / action;
                    let id = match hit {
                        0 => NEW,
                        1 => match self.settings.terminal_dock {
                            TerminalDock::Bottom => DOCK_RIGHT,
                            TerminalDock::Right => DOCK_LEFT,
                            TerminalDock::Left => DOCK_BOTTOM,
                        },
                        _ => TOGGLE,
                    };
                    self.command(id);
                } else if (left..left + room).contains(&point.x) {
                    let relative = point.x - left;
                    let index = self.terminal.first_tab + (relative / width) as usize;
                    if index < self.terminal.tabs.len() {
                        if msg == WM_MBUTTONDOWN || relative % width >= width - self.px(25) {
                            self.close_terminal_tab(index);
                        } else {
                            self.activate_terminal(index);
                            self.terminal.tab_drag = Some((index, point));
                            unsafe {
                                SetCapture(header);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        repaint(self.terminal.header);
    }

    fn terminal_body_mouse(&mut self, msg: u32, wp: usize, point: Point) {
        let body = self.terminal.body;
        match msg {
            WM_MOUSEWHEEL => {
                self.terminal_scroll(((wp >> 16) as i16 as i32) * 3 / 120);
            }
            WM_RBUTTONUP => {
                if let Some(tab) = self.terminal.active_mut()
                    && tab.selection.is_some_and(|(a, b)| a != b)
                {
                    self.copy_terminal();
                } else {
                    self.paste_terminal();
                }
            }
            WM_LBUTTONDOWN => {
                self.focus_terminal();
                let scroll = point.x >= bounds(body).width() - self.px(13);
                if let Some(tab) = self.terminal.active_mut() {
                    tab.selection = None;
                }
                self.terminal.body_drag = if scroll {
                    Some(BodyDrag::History)
                } else {
                    self.terminal_cell(point).map(BodyDrag::Text)
                };
                unsafe {
                    SetCapture(body);
                }
                self.update_terminal_selection(point);
            }
            WM_MOUSEMOVE => self.update_terminal_selection(point),
            WM_LBUTTONUP | WM_CAPTURECHANGED
                if self.terminal.body_drag.take().is_some() && msg == WM_LBUTTONUP =>
            unsafe {
                ReleaseCapture();
            },
            _ => {}
        }
        redraw(body);
    }

    fn terminal_cell(&self, point: Point) -> Option<CellPos> {
        let tab = self.terminal.active()?;
        let (cols, rows) = tab.screen.shape();
        let row =
            ((point.y - self.px(6)) / self.terminal.cell.y).clamp(0, rows as i32 - 1) as usize;
        let col =
            ((point.x - self.px(10)) / self.terminal.cell.x).clamp(0, cols as i32 - 1) as usize;
        Some((tab.screen.history().saturating_sub(tab.scroll) + row, col))
    }

    fn update_terminal_selection(&mut self, point: Point) {
        match self.terminal.body_drag {
            Some(BodyDrag::Text(first)) => {
                if let Some(last) = self.terminal_cell(point)
                    && let Some(tab) = self.terminal.active_mut()
                {
                    tab.selection = Some((first, last));
                }
            }
            Some(BodyDrag::History) => {
                let height = bounds(self.terminal.body).height().max(1);
                if let Some(tab) = self.terminal.active_mut() {
                    let history = tab.screen.history();
                    tab.scroll =
                        history - (point.y.clamp(0, height) as usize * history / height as usize);
                }
            }
            None => {}
        }
    }

    fn paint_terminal(&self, id: usize, dc: Handle, rect: Rect, palette: Paint) {
        match id {
            HEADER => self.paint_terminal_header(dc, rect, palette),
            DIVIDER => {
                fill(dc, rect, palette.chrome);
                let tone = motion::value(
                    Key::control(self.terminal.divider, Effect::Hover),
                    self.terminal.divider_hover || self.terminal.panel_drag.is_some(),
                );
                let line = if self.settings.terminal_dock == TerminalDock::Bottom {
                    Rect::from_size(0, rect.height() / 2, rect.width(), 1)
                } else {
                    Rect::from_size(rect.width() / 2, 0, 1, rect.height())
                };
                fill_color(
                    dc,
                    line,
                    motion::mix(palette.border_rgb, palette.accent, tone),
                );
            }
            BODY => self.paint_terminal_body(dc, rect, palette),
            _ => {}
        }
    }

    fn paint_terminal_header(&self, dc: Handle, rect: Rect, palette: Paint) {
        fill(dc, rect, palette.chrome);
        let (left, room, action) = self.terminal.controls(rect, self.dpi);
        if left > 0 {
            draw_label(
                dc,
                Rect::from_size(self.px(10), 0, left - self.px(10), rect.height()),
                "터미널",
                palette.font,
                palette.muted,
                false,
            );
        }
        let (width, count) = self.terminal.tab_width(rect, self.dpi);
        for (offset, tab) in self
            .terminal
            .tabs
            .iter()
            .skip(self.terminal.first_tab)
            .take(count)
            .enumerate()
        {
            let index = offset + self.terminal.first_tab;
            let x = left + offset as i32 * width;
            let cell = Rect::from_size(x, 0, width.min((left + room - x).max(0)), rect.height());
            if cell.width() == 0 {
                continue;
            }
            let active = index == self.terminal.active;
            let hover = motion::value(
                Key::item(self.terminal.header, index, Effect::Hover),
                self.terminal.hover.is_some_and(|point| cell.has(point)),
            );
            let selection = motion::value(
                Key::item(self.terminal.header, index, Effect::Selected),
                active,
            );
            let base = motion::mix(palette.chrome_rgb, palette.control_rgb, hover);
            let background = motion::mix(base, palette.editor_rgb, selection);
            fill_color(dc, cell, background);
            if selection > 0.0 {
                fill_color(
                    dc,
                    Rect {
                        bottom: self.px(2),
                        ..cell
                    },
                    motion::mix(background, palette.accent, selection),
                );
            }
            draw_label(
                dc,
                Rect {
                    left: x + self.px(8),
                    right: cell.right - self.px(23),
                    ..cell
                },
                &tab.label,
                palette.font,
                if tab.session.is_some() {
                    palette.foreground
                } else {
                    palette.muted
                },
                false,
            );
            draw_label(
                dc,
                Rect {
                    left: cell.right - self.px(23),
                    ..cell
                },
                "×",
                palette.font,
                palette.muted,
                true,
            );
        }
        for (index, label) in ["+", "⇄", "−"].iter().enumerate() {
            let cell = Rect::from_size(
                rect.right - (3 - index as i32) * action,
                0,
                action,
                rect.height(),
            );
            let hover = motion::value(
                Key::item(self.terminal.header, usize::MAX - index, Effect::Hover),
                self.terminal.hover.is_some_and(|point| cell.has(point)),
            );
            if hover > 0.0 {
                fill_color(
                    dc,
                    cell,
                    motion::mix(palette.chrome_rgb, palette.control_rgb, hover),
                );
            }
            draw_label(
                dc,
                cell,
                label,
                palette.large_font,
                palette.foreground,
                true,
            );
        }
        fill(
            dc,
            Rect {
                top: rect.bottom - 1,
                ..rect
            },
            palette.border,
        );
    }

    fn paint_terminal_body(&self, dc: Handle, rect: Rect, palette: Paint) {
        fill(dc, rect, palette.background);
        let Some(tab) = self.terminal.active() else {
            return;
        };
        let cell = self.terminal.cell;
        let origin = Point {
            x: self.px(10),
            y: self.px(6),
        };
        let (cols, rows) = tab.screen.shape();
        let history = tab.screen.history();
        let first = history.saturating_sub(tab.scroll);
        let selection = tab
            .selection
            .filter(|(a, b)| a != b)
            .map(|(a, b)| (a.min(b), a.max(b)));
        let mut text = String::with_capacity(cols);
        for row in 0..rows {
            let y = origin.y + row as i32 * cell.y;
            if y + cell.y > rect.bottom {
                break;
            }
            let Some(line) = tab.screen.line(first + row) else {
                break;
            };
            let mut col = 0;
            while col < line.len().min(cols) {
                let current = &line[col];
                if current.width == 0 {
                    col += 1;
                    continue;
                }
                let picked = |column| {
                    selection.is_some_and(|(a, b)| (a..=b).contains(&(first + row, column)))
                };
                let selected = picked(col) || (current.width == 2 && picked(col + 1));
                let simple =
                    current.width == 1 && current.ch.is_ascii() && current.marks.is_empty();
                let mut end = col + current.width as usize;
                text.clear();
                text.push(current.ch);
                text.push_str(&current.marks);
                if simple {
                    while let Some(next) = line.get(end).filter(|_| end < cols) {
                        if next.width != 1
                            || !next.ch.is_ascii()
                            || !next.marks.is_empty()
                            || next.style != current.style
                            || picked(end) != selected
                        {
                            break;
                        }
                        text.push(next.ch);
                        end += 1;
                    }
                }
                let cell_rect = Rect::from_size(
                    origin.x + col as i32 * cell.x,
                    y,
                    (end - col) as i32 * cell.x,
                    cell.y,
                );
                let (fg, bg) = colors(current.style, palette, self.settings.dark);
                if selected {
                    fill(dc, cell_rect, palette.selected);
                } else if bg != palette.editor_rgb {
                    fill_color(dc, cell_rect, bg);
                }
                if text.bytes().any(|byte| byte != b' ') {
                    draw_label(dc, cell_rect, &text, self.terminal.font, fg, false);
                }
                if current.style.underline {
                    fill_color(
                        dc,
                        Rect {
                            top: cell_rect.bottom - 2,
                            bottom: cell_rect.bottom - 1,
                            ..cell_rect
                        },
                        fg,
                    );
                }
                col = end;
            }
        }
        if history > 0 {
            let track = rect.height() - origin.y * 2;
            let thumb = (track * rows as i32 / (rows + history) as i32)
                .max(self.px(16))
                .min(track);
            let y = origin.y + (track - thumb) * first as i32 / history as i32;
            fill(
                dc,
                Rect::from_size(rect.right - self.px(7), y, self.px(4), thumb),
                palette.thumb,
            );
        }
        if tab.screen.cursor
            && tab.session.is_some()
            && tab.scroll == 0
            && unsafe { GetFocus() } == self.terminal.body
        {
            let (row, col) = tab.screen.position();
            let point = Point {
                x: origin.x + col as i32 * cell.x,
                y: origin.y + row as i32 * cell.y,
            };
            fill(
                dc,
                Rect::from_size(point.x, point.y + 1, self.px(2), cell.y - 2),
                palette.accent_brush,
            );
        }
    }
}

fn colors(style: Style, palette: Paint, dark: bool) -> (u32, u32) {
    let foreground = match style.fg {
        Ink::Indexed(i @ 0..=7) if style.bright => ansi(i + 8, dark),
        ink => ink_rgb(ink, palette.foreground, dark),
    };
    let background = ink_rgb(style.bg, palette.editor_rgb, dark);
    if style.reverse {
        (background, foreground)
    } else {
        (foreground, background)
    }
}

fn ink_rgb(ink: Ink, fallback: u32, dark: bool) -> u32 {
    match ink {
        Ink::Default => fallback,
        Ink::Indexed(i) => ansi(i, dark),
        Ink::Rgb(r, g, b) => rgb(r, g, b),
    }
}

fn ansi(index: u8, dark: bool) -> u32 {
    const DARK: [(u8, u8, u8); 16] = [
        (31, 38, 45),
        (226, 123, 120),
        (130, 199, 155),
        (227, 187, 112),
        (128, 175, 237),
        (198, 151, 221),
        (101, 195, 194),
        (210, 220, 225),
        (125, 145, 151),
        (250, 151, 146),
        (160, 226, 180),
        (248, 213, 140),
        (154, 204, 255),
        (223, 182, 250),
        (138, 227, 223),
        (241, 246, 248),
    ];
    const LIGHT: [(u8, u8, u8); 16] = [
        (28, 39, 44),
        (169, 54, 57),
        (29, 111, 71),
        (130, 89, 24),
        (38, 93, 159),
        (123, 66, 147),
        (26, 109, 108),
        (214, 226, 226),
        (98, 117, 126),
        (151, 39, 47),
        (17, 101, 59),
        (114, 73, 7),
        (22, 79, 155),
        (101, 46, 145),
        (17, 99, 98),
        (247, 251, 250),
    ];
    let (r, g, b) = match index {
        0..=15 => {
            if dark {
                DARK[index as usize]
            } else {
                LIGHT[index as usize]
            }
        }
        16..=231 => {
            let n = index - 16;
            let component = |n| if n == 0 { 0 } else { n * 40 + 55 };
            (component(n / 36), component(n / 6 % 6), component(n % 6))
        }
        _ => {
            let gray = (index - 232) * 10 + 8;
            (gray, gray, gray)
        }
    };
    rgb(r, g, b)
}
