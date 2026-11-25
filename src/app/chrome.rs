use super::*;
use motion::{Area, Effect, Key};

pub(super) const SURFACE: &str = "MakerSurface";
pub(super) const DIVIDER: usize = 310;
pub(super) const POPUP: usize = 311;
const FIRST_SCROLL: usize = 320;
const LAST_SCROLL: usize = 325;
const SCROLL_SYNC: u32 = 0x8001;
const EXPLORER_CLICK: u32 = 0x8002;
const TREE_ARROW: isize = 0x10;

thread_local! {
    static HOVERED_BUTTON: Cell<Handle> = const { Cell::new(0) };
    static PRESSED_BUTTON: Cell<Handle> = const { Cell::new(0) };
    static MODIFIED_LIST_CLICK: Cell<bool> = const { Cell::new(false) };
    static HOVERED_ROW: Cell<Option<(Handle, usize)>> = const { Cell::new(None) };
}

pub(super) fn row_is_hovered(window: Handle, row: usize) -> bool {
    HOVERED_ROW.get() == Some((window, row))
}

fn hover_row(window: Handle, next: Option<usize>) {
    let next = next.map(|row| (window, row));
    let old = HOVERED_ROW.replace(next);
    if old != next {
        for (list, row) in old.into_iter().chain(next) {
            let entering = next == Some((list, row));
            motion::animate(
                unsafe { GetParent(list) },
                Key::item(list, row, Effect::Hover),
                Area::Item,
                !entering,
                entering,
            );
        }
    }
}

pub(super) fn clear_file_hover(window: Handle) {
    if HOVERED_ROW.get().is_some_and(|(list, _)| list == window) {
        hover_row(window, None);
    }
}

pub(super) fn reset_file_motion(window: Handle) {
    clear_file_hover(window);
    motion::clear(window);
}

fn visible_selection(list: Handle) -> Vec<(usize, bool)> {
    let first = send(list, LB_GETTOPINDEX, 0, 0).max(0) as usize;
    let total = send(list, 0x018b, 0, 0).max(0) as usize;
    let row_height = send(list, 0x01a1, 0, 0).max(1) as i32;
    let visible = (bounds(list).height().max(0) / row_height + 1) as usize;
    (first..(first + visible).min(total))
        .map(|row| (row, send(list, 0x0187, row, 0) > 0))
        .collect()
}

fn transition_selection(list: Handle, parent: Handle, before: Vec<(usize, bool)>) {
    for (row, old) in before {
        let selected = send(list, 0x0187, row, 0) > 0;
        if old != selected {
            motion::animate(
                parent,
                Key::item(list, row, Effect::Selected),
                Area::Item,
                old,
                selected,
            );
        }
    }
}

pub(super) fn select_file_row(list: Handle, row: usize) {
    let previous = if motion::enabled() {
        visible_selection(list)
    } else {
        Vec::new()
    };
    list_select(list, row);
    transition_selection(list, unsafe { GetParent(list) }, previous);
}

pub(super) fn animate_folder(list: Handle, row: usize, open: bool) {
    motion::animate(
        unsafe { GetParent(list) },
        Key::item(list, row, Effect::Arrow),
        Area::Item,
        !open,
        open,
    );
}

#[derive(Clone, Copy, Default)]
pub(super) struct ScrollBar {
    handle: Handle,
    target: Handle,
    horizontal: bool,
    explorer: bool,
    visible: bool,
}

#[derive(Clone, Copy)]
enum Drag {
    Sidebar { x: i32, width: i32, right: bool },
    Scroll { id: usize, grab: i32 },
}

pub(super) struct Menu {
    owner: usize,
    rows: Vec<(usize, String)>,
    active: Option<usize>,
    scroll: i32,
    completion: Option<Suggestions>,
    previous_focus: Handle,
}

pub(super) struct Suggestions {
    pub editor: Handle,
    pub caret: i32,
    pub range: Selection,
    pub candidates: Vec<text::completion::Candidate>,
}

const SECOND_TABS: usize = 384;
const BREADCRUMB: usize = 385;
const SECOND_BREADCRUMB: usize = 386;

pub(super) struct Chrome {
    pub(super) divider: Handle,
    pub(super) popup: Handle,
    pub(super) tabs: Handle,
    second_tabs: Handle,
    pub(super) breadcrumbs: [Handle; 2],
    tab_offset: [i32; 2],
    tab_hover: [Option<Point>; 2],
    tab_drag: Option<(usize, Point, Option<usize>)>,
    divider_hover: bool,
    scroll_hover: Option<usize>,
    drag: Option<Drag>,
    pub(super) menu: Option<Menu>,
    closing_menu: Option<Menu>,
    closing_editor_tools: bool,
    popup_bounds: Cell<Option<Rect>>,
    bars: [ScrollBar; 6],
    scroll_clips: [(Handle, Rect); 4],
}

impl Chrome {
    pub(super) fn new(window: Handle) -> io::Result<Self> {
        let surface = |id, style| child(window, id, SURFACE, "", style);
        let tabs = surface(TABS, WS_TABSTOP)?;
        let divider = surface(DIVIDER, 0)?;
        let mut bars = [ScrollBar::default(); 6];
        for (index, bar) in bars.iter_mut().enumerate() {
            bar.handle = surface(FIRST_SCROLL + index, 0)?;
            bar.explorer = matches!(index, 0 | 5);
            bar.horizontal = matches!(index, 2 | 4);
        }
        Ok(Self {
            tabs,
            second_tabs: surface(SECOND_TABS, WS_TABSTOP)?,
            breadcrumbs: [surface(BREADCRUMB, 0)?, surface(SECOND_BREADCRUMB, 0)?],
            divider,
            bars,
            popup: surface(POPUP, 0)?,
            tab_offset: [0; 2],
            tab_hover: [None; 2],
            tab_drag: None,
            divider_hover: false,
            scroll_hover: None,
            drag: None,
            menu: None,
            closing_menu: None,
            closing_editor_tools: false,
            popup_bounds: Cell::new(None),
            scroll_clips: [(0, Rect::default()); 4],
        })
    }

    pub(super) fn tab_handle(&self, group: usize) -> Handle {
        if group == 0 {
            self.tabs
        } else {
            self.second_tabs
        }
    }

    pub(super) fn tab_group(&self, handle: Handle) -> Option<usize> {
        [self.tabs, self.second_tabs]
            .iter()
            .position(|&tab| tab == handle)
    }

    fn scroll_lit(&self, id: usize) -> bool {
        self.scroll_hover == Some(id)
            || matches!(self.drag, Some(Drag::Scroll { id: held, .. }) if held == id)
    }

    fn divider_lit(&self) -> bool {
        self.divider_hover || matches!(self.drag, Some(Drag::Sidebar { .. }))
    }
}

pub(super) fn register() -> io::Result<()> {
    register_class(SURFACE, surface_proc)
}

extern "system" fn surface_proc(window: Handle, message: u32, wp: usize, lp: isize) -> isize {
    let id = unsafe { GetDlgCtrlID(window) } as usize;
    match message {
        WM_PAINT => {
            if matches!(
                id,
                POPUP | TABS | SECOND_TABS | BREADCRUMB | SECOND_BREADCRUMB
            ) {
                buffered_paint(window, |dc, rect| draw_surface(id, dc, rect));
            } else {
                paint(window, |dc, rect| draw_surface(id, dc, rect));
            }
            return 0;
        }
        WM_PRINTCLIENT => {
            draw_surface(id, wp as Handle, bounds(window));
            return 0;
        }
        WM_ERASEBKGND => return 1,
        0x0084 if id == POPUP && with_app(|app| app.chrome.menu.is_none()).unwrap_or(false) => {
            return -1; // HTTRANSPARENT while a dismissed menu finishes its transition.
        }
        0x0084 if id == dock::GUIDE => return -1,
        WM_SETCURSOR if matches!(id, DIVIDER | dock::SPLITTER | dock::SIDEBAR_SPLITTER) => {
            let stacked = id == dock::SIDEBAR_SPLITTER
                || id == dock::SPLITTER && with_app(|app| app.dock.stacked).unwrap_or(false);
            cursor(if stacked { 32645 } else { 32644 });
            return 1;
        }
        WM_MOUSEMOVE | WM_MOUSELEAVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK
        | WM_MBUTTONDOWN | WM_MOUSEWHEEL | WM_CAPTURECHANGED => {
            with_app(|app| app.surface_mouse(window, id, message, wp, Point::packed(lp)));
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(window, message, wp, lp) }
}

fn draw_surface(id: usize, dc: Handle, rect: Rect) {
    if let Some(palette) = PAINT.get() {
        if id != POPUP {
            fill(dc, rect, palette.background);
        }
        let painted = with_app(|app| app.paint_surface(id, dc, rect, palette));
        if id == POPUP && painted.is_none() {
            fill(dc, rect, palette.menu);
        }
    }
}

thread_local! {
    static DELETING_TEXT: Cell<Handle> = const { Cell::new(0) };
}

pub(super) fn deleting_text(editor: Handle) -> bool {
    DELETING_TEXT.get() == editor
}

pub(super) fn watch_control(window: Handle, toolbar: bool) {
    subclass(window, control_proc, usize::from(toolbar));
}

extern "system" fn control_proc(
    window: Handle,
    message: u32,
    wp: usize,
    lp: isize,
    _id: usize,
    toolbar: usize,
) -> isize {
    let parent = unsafe { GetParent(window) };
    if toolbar != 0 {
        button_input(parent, window, message, wp);
        if matches!(message, WM_MOUSEMOVE | WM_MOUSELEAVE) && wp & 1 == 0 {
            return 0;
        }
    }
    let list = toolbar == 0 && unsafe { GetDlgCtrlID(window) } == LIST as i32;
    let browser_list = toolbar == 0 && unsafe { GetDlgCtrlID(window) } == browser::RESULTS as i32;
    let data_list = toolbar == 0 && unsafe { GetDlgCtrlID(window) } == data_view::TREE as i32;
    let row_list = list
        || browser_list
        || data_list
        || toolbar == 0 && unsafe { GetDlgCtrlID(window) } == git::GRAPH_LIST as i32;
    if row_list && message == WM_MOUSEMOVE {
        if HOVERED_ROW.get().is_none_or(|(list, _)| list != window) {
            watch_mouse(window);
        }
        hover_row(window, list_at(window, lp));
    }
    if row_list && message == WM_MOUSELEAVE {
        clear_file_hover(window);
    }
    if row_list && matches!(message, WM_MOUSEWHEEL | WM_VSCROLL) {
        reset_file_motion(window);
    }
    let previous = if row_list
        && motion::enabled()
        && matches!(message, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_KEYDOWN)
    {
        visible_selection(window)
    } else {
        Vec::new()
    };
    if (list || browser_list)
        && message == WM_LBUTTONDBLCLK
        && let Some(row) = list_at(window, lp)
        && with_app(|app| app.run_file_row(window, row)).unwrap_or(false)
    {
        return 0;
    }
    if browser_list && (message == WM_MBUTTONDOWN || message == WM_LBUTTONDOWN && wp & 8 != 0) {
        if let Some(row) = list_at(window, lp) {
            select_file_row(window, row);
            unsafe {
                SetFocus(window);
            }
            with_app(|app| app.post_browser_click(window, row, Point::packed(lp), true));
        }
        return 0;
    }
    if (list || browser_list)
        && matches!(
            message,
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MOUSEMOVE | WM_CAPTURECHANGED
        )
        && with_app(|app| app.list_drag(window, message, wp, lp)).unwrap_or(false)
    {
        transition_selection(window, parent, previous);
        return 0;
    }
    if list && matches!(message, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK) {
        let modifiers = wp & 0x0c;
        MODIFIED_LIST_CLICK.set(modifiers != 0);
        if modifiers != 0 {
            if let Some(row) = list_at(window, lp) {
                unsafe {
                    SetFocus(window);
                }
                list_modified_click(window, row, modifiers & 4 != 0, modifiers & 8 != 0);
                transition_selection(window, parent, previous);
                unsafe {
                    PostMessageW(parent, EXPLORER_CLICK, row, modifiers as isize);
                }
            }
            return 0;
        }
    }
    if list && message == WM_LBUTTONUP && MODIFIED_LIST_CLICK.replace(false) {
        return 0;
    }
    let deletes = toolbar == 0
        && matches!(
            (message, wp),
            (WM_KEYDOWN, 8 | 0x2e) | (0x0102, 8 | 0x7f) | (0x0300 | 0x0303, _)
        )
        && unsafe { GetDlgCtrlID(window) } == EDITOR as i32;
    if matches!(message, 0x0302 | 0x0440)
        && toolbar == 0
        && unsafe { GetDlgCtrlID(window) } == EDITOR as i32
    {
        with_app(|app| app.invalidate_editor_styles(window));
    }
    let previous_delete = if deletes {
        with_app(App::dismiss_completion);
        DELETING_TEXT.replace(window)
    } else {
        0
    };
    let result = unsafe { DefSubclassProc(window, message, wp, lp) };
    if deletes {
        DELETING_TEXT.set(previous_delete);
    }
    if data_list
        && message == WM_LBUTTONUP
        && let Some(row) = list_at(window, lp)
    {
        unsafe {
            PostMessageW(parent, data_view::TOGGLE, window as usize, row as isize);
        }
    }
    transition_selection(window, parent, previous);
    if list
        && message == WM_LBUTTONUP
        && let Some(item) = list_at(window, lp)
    {
        post_explorer_click(parent, window, item, Point::packed(lp));
    }
    if toolbar == 0
        && matches!(
            message,
            WM_MOUSEWHEEL
                | WM_HSCROLL
                | WM_VSCROLL
                | WM_KEYDOWN
                | WM_LBUTTONUP
                | WM_SIZE
                | 0x0102
                | EM_SETSCROLLPOS
                | EM_SCROLLCARET
        )
    {
        unsafe {
            PostMessageW(parent, SCROLL_SYNC, window as usize, 0);
        }
    }
    result
}

pub(super) fn post_explorer_click(parent: Handle, window: Handle, row: usize, point: Point) {
    let data = send(window, 0x0199, row, 0) as usize;
    let dpi = PAINT.get().map_or(96, |paint| paint.dpi);
    let depth = (data >> 8).min(40) as i32;
    let icon_left = 20 * dpi / 96 + depth * (14 * dpi / 96);
    let flag = if point.x < icon_left { TREE_ARROW } else { 0 };
    unsafe {
        PostMessageW(parent, EXPLORER_CLICK, row, flag);
    }
}

pub(super) fn handle_parent_message(message: u32, wp: usize, modifiers: isize) -> bool {
    match message {
        SCROLL_SYNC => {
            with_app(|app| {
                app.redraw_scroll_position(wp as Handle);
            });
        }
        EXPLORER_CLICK => {
            with_app(|app| {
                if (app.is_tree() || app.git.open)
                    && wp < app.list_items.len()
                    && modifiers & 0x0c == 0
                {
                    select_file_row(app.list, wp);
                    app.track_explorer_selection();
                    let opened = match app.list_items.get(wp) {
                        Some(ListItem::Archive { path, .. }) if modifiers == TREE_ARROW => {
                            app.archive_toggle(path.clone())
                        }
                        _ => app.open_selected(),
                    };
                    if let Err(error) = opened {
                        app.show_error(error);
                    } else {
                        unsafe {
                            SetFocus(app.list);
                        }
                    }
                } else if app.is_tree() {
                    app.track_explorer_selection();
                }
            });
        }
        _ => return false,
    }
    true
}

fn button_input(parent: Handle, window: Handle, message: u32, wp: usize) {
    let control = |effect, from, to| {
        motion::animate(
            parent,
            Key::control(window, effect),
            Area::Control,
            from,
            to,
        );
    };
    match message {
        WM_MOUSEMOVE => {
            let previous = HOVERED_BUTTON.replace(window);
            if previous != window {
                watch_mouse(window);
                if previous != 0 {
                    motion::animate(
                        parent,
                        Key::control(previous, Effect::Hover),
                        Area::Control,
                        true,
                        false,
                    );
                }
                control(Effect::Hover, false, true);
            }
        }
        WM_MOUSELEAVE if HOVERED_BUTTON.get() == window => {
            HOVERED_BUTTON.set(0);
            control(Effect::Hover, true, false);
        }
        0x0007 => control(Effect::Focus, false, true),
        0x0008 => {
            control(Effect::Focus, true, false);
            if PRESSED_BUTTON.get() == window {
                PRESSED_BUTTON.set(0);
                control(Effect::Press, true, false);
            }
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | 0x00f5 => {
            PRESSED_BUTTON.set(window);
            control(Effect::Press, false, true);
        }
        WM_KEYDOWN if matches!(wp, 13 | 32) && PRESSED_BUTTON.get() != window => {
            PRESSED_BUTTON.set(window);
            control(Effect::Press, false, true);
        }
        WM_CAPTURECHANGED | WM_LBUTTONUP | 0x0101 if PRESSED_BUTTON.get() == window => {
            PRESSED_BUTTON.set(0);
            control(Effect::Press, true, false);
        }
        _ => {}
    }
}

pub(super) fn button_tint(item: &DrawItem, palette: Paint, base: u32, active: bool) -> u32 {
    let level = |effect, steady| motion::value(Key::control(item.window, effect), steady);
    let hover = level(Effect::Hover, HOVERED_BUTTON.get() == item.window);
    let focus = level(Effect::Focus, item.state & 0x10 != 0);
    let pressed = level(Effect::Press, item.state & 1 != 0);
    let selection = level(Effect::Selected, active);
    let highlight = selection.max(hover * 0.85).max(focus * 0.7);
    let color = motion::mix(base, palette.selected_rgb, highlight);
    motion::mix(color, palette.accent, pressed * 0.24)
}

pub(super) fn file_row_tint(item: &DrawItem, palette: Paint) -> u32 {
    let index = item.item as usize;
    let key = |effect| Key::item(item.window, index, effect);
    let hover = motion::value(
        key(Effect::Hover),
        HOVERED_ROW.get() == Some((item.window, index)),
    );
    let selected = motion::value(key(Effect::Selected), item.state & 1 != 0);
    let color = motion::mix(palette.panel_rgb, palette.control_rgb, hover * 0.67);
    motion::mix(color, palette.selected_rgb, selected)
}

struct Thumb {
    start: i32,
    length: i32,
    travel: i32,
    min: i32,
    range: i32,
}

impl Thumb {
    fn measure(info: ScrollInfo, track: i32, minimum: i32) -> Option<Self> {
        let total = i64::from(info.max) - i64::from(info.min) + 1;
        let page = i64::from(info.page.max(1));
        if total <= page || track <= minimum {
            return None;
        }
        let range = (total - page).min(i64::from(i32::MAX)) as i32;
        let length = (i64::from(track) * page / total) as i32;
        let length = length.clamp(minimum, track);
        let travel = track - length;
        let offset = (i64::from(info.position) - i64::from(info.min)).clamp(0, i64::from(range));
        let start = (offset * i64::from(travel) / i64::from(range)) as i32;
        Some(Self {
            start,
            length,
            travel,
            min: info.min,
            range,
        })
    }

    fn position(&self, offset: i32) -> i32 {
        let value = i64::from(offset.clamp(0, self.travel)) * i64::from(self.range);
        self.min
            .saturating_add(((value + i64::from(self.travel) / 2) / i64::from(self.travel)) as i32)
    }
}

impl App {
    pub(super) fn sync_scrollbars(&mut self) {
        let second = self.other.and_then(|index| {
            let document = &self.documents[index];
            document.source_visible().then_some(document.editor)
        });
        let active = self.editor();
        let targets = [
            self.sidebar.then_some(self.list),
            active,
            active,
            second,
            second,
            self.graph_visible().then_some(self.git.graph.list),
        ];
        for (index, target) in targets.into_iter().enumerate() {
            let bar = &mut self.chrome.bars[index];
            let was_visible = bar.visible;
            bar.visible = false;
            bar.target = target.unwrap_or(0);
            if let Some(target) = target {
                let outer = relative_rect(target, self.window);
                let inner = bounds(target);
                let origin = map_point(target, self.window, Point::default());
                if !bar.horizontal {
                    let index = match index {
                        0 => 0,
                        1 => 1,
                        3 => 2,
                        _ => 3,
                    };
                    let clip = Rect::from_size(
                        origin.x - outer.left,
                        origin.y - outer.top,
                        inner.width(),
                        inner.height(),
                    );
                    if self.chrome.scroll_clips[index] != (target, clip)
                        && clip.width() > 0
                        && clip.height() > 0
                    {
                        clip_client(target, clip);
                        self.chrome.scroll_clips[index] = (target, clip);
                    }
                }
                let right = origin.x + inner.right;
                let bottom = origin.y + inner.bottom;
                let rect = if bar.horizontal {
                    Rect::from_size(
                        origin.x,
                        bottom,
                        outer.right - origin.x,
                        outer.bottom - bottom,
                    )
                } else {
                    Rect::from_size(right, origin.y, outer.right - right, bottom - origin.y)
                };
                if rect.width() > 2 && rect.height() > 2 {
                    if !was_visible || relative_rect(bar.handle, self.window) != rect {
                        show_overlay(bar.handle, rect);
                    }
                    repaint(bar.handle);
                    bar.visible = true;
                }
            }
            if was_visible && !bar.visible {
                show(bar.handle, false);
            }
        }
    }

    pub(super) fn redraw_scroll_position(&mut self, target: Handle) {
        for bar in &self.chrome.bars {
            if bar.visible && bar.target == target {
                repaint(bar.handle);
            }
        }
        if self.editor() == Some(target) {
            repaint(self.gutter);
        }
        self.schedule_viewport_highlight(target);
    }

    fn thumb(&self, bar: ScrollBar, rect: Rect) -> Option<Thumb> {
        let info = scrollbar(bar.target, bar.horizontal)?;
        Thumb::measure(
            info,
            if bar.horizontal {
                rect.width()
            } else {
                rect.height()
            },
            self.px(20),
        )
    }

    fn scroll_to(&mut self, id: usize, coord: i32, grab: i32) {
        let bar = self.chrome.bars[id - FIRST_SCROLL];
        if !bar.visible {
            return;
        }
        let Some(thumb) = self.thumb(bar, bounds(bar.handle)) else {
            return;
        };
        let position = thumb.position(coord.saturating_sub(grab));
        if bar.explorer {
            send(bar.target, LB_SETTOPINDEX, position as usize, 0);
        } else if thumb.range > u16::MAX.into() {
            let mut scroll = Point {
                x: scrollbar(bar.target, true).map_or(0, |bar| bar.position),
                y: scrollbar(bar.target, false).map_or(0, |bar| bar.position),
            };
            if bar.horizontal {
                scroll.x = position;
            } else {
                scroll.y = position;
            }
            send(bar.target, EM_SETSCROLLPOS, 0, &scroll as *const _ as isize);
        } else {
            set_scroll_thumb(bar.target, bar.horizontal, position);
            let message = if bar.horizontal {
                WM_HSCROLL
            } else {
                WM_VSCROLL
            };
            send(
                bar.target,
                message,
                5 | ((position as u16 as usize) << 16),
                0,
            );
        }
        if bar.explorer {
            reset_file_motion(bar.target);
        }
        self.redraw_scroll_position(bar.target);
    }

    fn paint_surface(&self, id: usize, dc: Handle, rect: Rect, palette: Paint) {
        match id {
            TABS | SECOND_TABS => {
                self.paint_tabs(usize::from(id == SECOND_TABS), dc, rect, palette)
            }
            BREADCRUMB | SECOND_BREADCRUMB => {
                self.paint_breadcrumb(usize::from(id == SECOND_BREADCRUMB), dc, rect, palette)
            }
            dock::GUIDE | dock::SPLITTER | dock::SIDEBAR_SPLITTER => {
                self.paint_dock(id, dc, rect, palette)
            }
            DIVIDER => {
                let emphasis = motion::value(
                    Key::control(self.chrome.divider, Effect::Hover),
                    self.chrome.divider_lit(),
                );
                fill(dc, rect, palette.panel);
                let thickness = if emphasis > 0.55 {
                    self.px(2)
                } else {
                    self.px(1)
                };
                fill_color(
                    dc,
                    Rect::from_size((rect.width() - thickness) / 2, 0, thickness, rect.height()),
                    motion::mix(palette.border_rgb, palette.accent, emphasis),
                );
            }
            POPUP => self.paint_menu(dc, rect, palette),
            FIRST_SCROLL..=LAST_SCROLL => {
                let bar = self.chrome.bars[id - FIRST_SCROLL];
                fill(
                    dc,
                    rect,
                    if bar.explorer {
                        palette.panel
                    } else {
                        palette.background
                    },
                );
                if let Some(thumb) = self.thumb(bar, rect) {
                    let inset = self.px(3).min(
                        if bar.horizontal {
                            rect.height()
                        } else {
                            rect.width()
                        } / 3,
                    );
                    let thumb_rect = if bar.horizontal {
                        Rect::from_size(thumb.start, inset, thumb.length, rect.height() - 2 * inset)
                    } else {
                        Rect::from_size(inset, thumb.start, rect.width() - 2 * inset, thumb.length)
                    };
                    let emphasis = motion::value(
                        Key::control(bar.handle, Effect::Hover),
                        self.chrome.scroll_lit(id),
                    );
                    fill_color(
                        dc,
                        thumb_rect,
                        motion::mix(palette.thumb_rgb, palette.thumb_hover_rgb, emphasis),
                    );
                }
            }
            _ => {}
        }
    }

    fn surface_mouse(&mut self, window: Handle, id: usize, message: u32, wp: usize, point: Point) {
        if message == WM_MOUSEMOVE {
            watch_mouse(window);
        }
        match id {
            DIVIDER => self.sidebar_mouse(window, message, point),
            TABS | SECOND_TABS => {
                self.tabs_mouse(usize::from(id == SECOND_TABS), message, wp, point)
            }
            dock::SPLITTER | dock::SIDEBAR_SPLITTER => self.splitter_mouse(window, message, point),
            POPUP => self.menu_mouse(message, wp, point),
            FIRST_SCROLL..=LAST_SCROLL => {
                let old = self.chrome.scroll_lit(id);
                let bar = self.chrome.bars[id - FIRST_SCROLL];
                let coord = if bar.horizontal { point.x } else { point.y };
                match message {
                    WM_MOUSEMOVE => {
                        self.chrome.scroll_hover = Some(id);
                        if let Some(Drag::Scroll { id: held, grab }) = self.chrome.drag
                            && held == id
                        {
                            self.scroll_to(id, coord, grab);
                        }
                    }
                    WM_MOUSELEAVE => self.chrome.scroll_hover = None,
                    WM_LBUTTONDOWN => {
                        if let Some(thumb) = self.thumb(bar, bounds(window)) {
                            let grab = if (thumb.start..thumb.start + thumb.length).contains(&coord)
                            {
                                coord - thumb.start
                            } else {
                                thumb.length / 2
                            };
                            self.chrome.drag = Some(Drag::Scroll { id, grab });
                            unsafe {
                                SetCapture(window);
                            }
                            self.scroll_to(id, coord, grab);
                        }
                    }
                    WM_LBUTTONUP | WM_CAPTURECHANGED => {
                        if matches!(self.chrome.drag, Some(Drag::Scroll { id: held, .. }) if held == id)
                        {
                            self.chrome.drag = None;
                            if message == WM_LBUTTONUP {
                                unsafe {
                                    ReleaseCapture();
                                }
                            }
                        }
                    }
                    WM_MOUSEWHEEL if bar.target != 0 => {
                        send(bar.target, WM_MOUSEWHEEL, wp, 0);
                    }
                    _ => {}
                }
                let next = self.chrome.scroll_lit(id);
                if old != next {
                    motion::animate(
                        self.window,
                        Key::control(window, Effect::Hover),
                        Area::Control,
                        old,
                        next,
                    );
                }
            }
            _ => {}
        }
    }

    pub(super) fn sidebar_size(&self, width: i32) -> i32 {
        self.clamp_sidebar(self.px(self.settings.sidebar_width), width)
    }

    fn clamp_sidebar(&self, desired: i32, width: i32) -> i32 {
        let max = (width - self.px(220)).max(0).min(width * 2 / 3);
        desired.clamp(self.px(144).min(max), max)
    }

    fn sidebar_mouse(&mut self, window: Handle, message: u32, point: Point) {
        let old = self.chrome.divider_lit();
        let width = bounds(self.window).width();
        let x = map_point(window, self.window, point).x;
        match message {
            WM_LBUTTONDOWN => {
                self.chrome.drag = Some(Drag::Sidebar {
                    x,
                    width: self.sidebar_size(width),
                    right: self.settings.sidebar_right,
                });
                unsafe {
                    SetCapture(window);
                }
            }
            WM_MOUSEMOVE => {
                self.chrome.divider_hover = true;
                if let Some(Drag::Sidebar {
                    x: start,
                    width: original,
                    right,
                }) = self.chrome.drag
                {
                    let cross = if right {
                        x < width / 4
                    } else {
                        x > width * 3 / 4
                    };
                    if self.settings.sidebar_right != right || cross {
                        let stays_crossed = if right {
                            x < width / 3
                        } else {
                            x > width * 2 / 3
                        };
                        self.settings.sidebar_right = if stays_crossed { !right } else { right };
                    }
                    let desired = if self.settings.sidebar_right != right {
                        original
                    } else if right {
                        original + start - x
                    } else {
                        original + x - start
                    };
                    self.settings.sidebar_width =
                        (self.clamp_sidebar(desired, width) * 96 / self.dpi).max(144);
                    self.layout();
                }
            }
            WM_MOUSELEAVE => self.chrome.divider_hover = false,
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                if matches!(self.chrome.drag, Some(Drag::Sidebar { .. })) {
                    self.chrome.drag = None;
                    if message == WM_LBUTTONUP {
                        unsafe {
                            ReleaseCapture();
                        }
                    }
                    self.save_sidebar_settings();
                }
            }
            WM_LBUTTONDBLCLK => {
                self.settings.sidebar_width = Settings::default().sidebar_width;
                self.layout();
                self.save_sidebar_settings();
            }
            _ => {}
        }
        let next = self.chrome.divider_lit();
        if old != next {
            motion::animate(
                self.window,
                Key::control(window, Effect::Hover),
                Area::Control,
                old,
                next,
            );
        }
    }

    pub(super) fn save_sidebar_settings(&self) {
        if let Err(error) = self.settings.save() {
            self.report(&format!("설정을 저장할 수 없습니다. {error}"));
        }
    }

    fn tab_width(&self, document: &Document) -> i32 {
        let letters = document
            .label
            .chars()
            .map(|ch| if ch.is_ascii() { 7 } else { 13 })
            .sum::<i32>();
        self.px((letters + 74).clamp(118, 240))
    }

    fn tab_limits(&self, group: usize, width: i32) -> (i32, i32) {
        let width = (width - self.px(32)).max(0);
        let total = self
            .documents
            .iter()
            .filter(|doc| doc.group == group)
            .map(|doc| self.tab_width(doc))
            .sum::<i32>();
        let viewport = if total > width {
            (width - self.px(58)).max(0)
        } else {
            width
        };
        (viewport, (total - viewport).max(0))
    }

    pub(super) fn tab_reveal(&mut self, index: usize) {
        let Some(doc) = self.documents.get(index) else {
            return;
        };
        let group = doc.group;
        let tabs = self.chrome.tab_handle(group);
        let (viewport, max) = self.tab_limits(group, bounds(tabs).width());
        let begin = self.documents[..index]
            .iter()
            .filter(|doc| doc.group == group)
            .map(|doc| self.tab_width(doc))
            .sum::<i32>();
        let end = begin + self.tab_width(doc);
        let offset = &mut self.chrome.tab_offset[group];
        if begin < *offset {
            *offset = begin;
        } else if end > *offset + viewport {
            *offset = end - viewport;
        }
        *offset = (*offset).clamp(0, max);
        repaint(tabs);
    }

    pub(super) fn clamp_tab_scroll(&mut self) {
        for group in 0..2 {
            let (_, max) = self.tab_limits(group, bounds(self.chrome.tab_handle(group)).width());
            self.chrome.tab_offset[group] = self.chrome.tab_offset[group].clamp(0, max);
        }
    }

    pub(super) fn clear_tab_motion(&mut self) {
        for group in 0..2 {
            motion::clear(self.chrome.tab_handle(group));
            self.chrome.tab_hover[group] = None;
        }
    }

    fn tab_target(&self, group: usize, point: Point, viewport: i32) -> Option<(usize, bool)> {
        if point.x >= viewport
            || point.x < 0
            || point.y < 0
            || point.y >= bounds(self.chrome.tab_handle(group)).height()
        {
            return None;
        }
        let mut x = -self.chrome.tab_offset[group];
        for (index, doc) in self
            .documents
            .iter()
            .enumerate()
            .filter(|(_, doc)| doc.group == group)
        {
            let end = x + self.tab_width(doc);
            if (x..end).contains(&point.x) {
                return Some((index, end <= viewport && point.x >= end - self.px(30)));
            }
            x = end;
        }
        None
    }

    fn tab_cell(&self, index: usize) -> Option<Rect> {
        let doc = self.documents.get(index)?;
        let group = doc.group;
        let view = bounds(self.chrome.tab_handle(group));
        let (viewport, _) = self.tab_limits(group, view.width());
        let offset = self.documents[..index]
            .iter()
            .filter(|doc| doc.group == group)
            .map(|doc| self.tab_width(doc))
            .sum::<i32>()
            - self.chrome.tab_offset[group];
        let cell = Rect {
            left: offset.max(0),
            right: (offset + self.tab_width(doc)).min(viewport),
            ..view
        };
        (cell.width() > 0).then_some(cell)
    }

    pub(super) fn animate_editor_tabs(&self, previous: Option<usize>, next: Option<usize>) {
        if previous == next {
            return;
        }
        for index in previous.into_iter().chain(next) {
            if let Some(rect) = self.tab_cell(index) {
                let group = self.documents[index].group;
                if Some(index) == previous && self.group_document(group) == Some(index) {
                    continue;
                }
                let selected = Some(index) == next;
                motion::animate(
                    self.window,
                    Key::item(self.chrome.tab_handle(group), index, Effect::Selected),
                    Area::Rect(rect),
                    !selected,
                    selected,
                );
            }
        }
    }

    fn paint_breadcrumb(&self, group: usize, dc: Handle, rect: Rect, palette: Paint) {
        fill(dc, rect, palette.background);
        let Some(doc) = self
            .group_document(group)
            .map(|index| &self.documents[index])
        else {
            return;
        };
        let name = if let Some(view) = &doc.browser {
            view.base().display().to_string()
        } else if let Some(path) = &doc.path {
            let shown = self
                .root
                .as_ref()
                .and_then(|root| path.strip_prefix(root).ok())
                .unwrap_or(path);
            shown
                .components()
                .map(|entry| entry.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("  ›  ")
        } else {
            doc.label.clone()
        };
        draw_label(
            dc,
            Rect {
                left: self.px(13),
                right: rect.right - self.px(8),
                ..rect
            },
            &name,
            palette.font,
            palette.muted,
            false,
        );
    }

    fn paint_tabs(&self, group: usize, dc: Handle, rect: Rect, palette: Paint) {
        let tabs = self.chrome.tab_handle(group);
        fill(dc, rect, palette.chrome);
        let (viewport, maximum) = self.tab_limits(group, rect.width());
        let mut start = -self.chrome.tab_offset[group];
        let hovered =
            self.chrome.tab_hover[group].and_then(|point| self.tab_target(group, point, viewport));
        for (index, document) in self
            .documents
            .iter()
            .enumerate()
            .filter(|(_, doc)| doc.group == group)
        {
            let end = start + self.tab_width(document);
            if start >= viewport {
                break;
            }
            if end > 0 {
                let active = self.group_document(group) == Some(index);
                let over = hovered.is_some_and(|(tab, _)| tab == index);
                let selected = motion::value(Key::item(tabs, index, Effect::Selected), active);
                let hover = motion::value(Key::item(tabs, index * 2, Effect::Hover), over);
                let cell = Rect {
                    left: start.max(0),
                    right: end.min(viewport),
                    ..rect
                };
                let idle = motion::mix(palette.chrome_rgb, palette.selected_rgb, hover);
                let background = motion::mix(idle, palette.editor_rgb, selected);
                fill_color(dc, cell, background);
                if selected > 0.0 {
                    fill_color(
                        dc,
                        Rect {
                            top: rect.bottom - self.px(2),
                            ..cell
                        },
                        motion::mix(background, palette.accent, selected),
                    );
                }
                icon(
                    dc,
                    Point {
                        x: start + self.px(11),
                        y: rect.height() / 2,
                    },
                    file_icon(document.path.as_deref()),
                    palette,
                    self.dpi,
                    cell,
                );
                let label = Rect {
                    left: (start + self.px(33)).max(0),
                    right: (end - self.px(32)).min(viewport),
                    ..rect
                };
                if label.width() > 0 {
                    draw_label(
                        dc,
                        label,
                        &document.label,
                        palette.font,
                        motion::mix(
                            palette.muted,
                            palette.foreground,
                            selected.max(0.45 + hover * 0.35),
                        ),
                        false,
                    );
                }
                if end <= viewport && end - self.px(26) >= 0 {
                    let close = Rect::from_size(
                        end - self.px(26),
                        self.px(5),
                        self.px(21),
                        rect.height() - self.px(10),
                    );
                    let over_close = motion::value(
                        Key::item(tabs, index * 2 + 1, Effect::Hover),
                        hovered == Some((index, true)),
                    );
                    if over_close > 0.0 {
                        fill_rounded(
                            dc,
                            close,
                            self.px(4),
                            motion::mix(background, palette.control_rgb, over_close),
                        );
                    }
                    let visibility = if document.dirty {
                        1.0
                    } else {
                        selected.max(0.55 + hover * 0.45)
                    };
                    if visibility > 0.0 {
                        draw_label(
                            dc,
                            close,
                            if document.dirty && !over { "●" } else { "×" },
                            palette.font,
                            motion::mix(
                                background,
                                if document.dirty {
                                    palette.accent
                                } else {
                                    palette.foreground
                                },
                                visibility,
                            ),
                            true,
                        );
                    }
                }
                if end > 0 && end <= viewport {
                    fill(
                        dc,
                        Rect::from_size(end - 1, rect.top, 1, rect.height()),
                        palette.border,
                    );
                }
            }
            start = end;
        }
        if maximum > 0 {
            for (index, arrow) in ["‹", "›"].into_iter().enumerate() {
                let cell = Rect::from_size(
                    viewport + index as i32 * self.px(29),
                    0,
                    self.px(29),
                    rect.height(),
                );
                if self.chrome.tab_hover[group].is_some_and(|point| cell.has(point)) {
                    fill(dc, cell, palette.control);
                }
                draw_label(
                    dc,
                    cell,
                    arrow,
                    palette.large_font,
                    palette.foreground,
                    true,
                );
            }
        }
        let new = Rect {
            left: (rect.right - self.px(32)).max(rect.left),
            ..rect
        };
        if self.chrome.tab_hover[group].is_some_and(|point| new.has(point)) {
            fill(dc, new, palette.control);
        }
        draw_label(dc, new, "+", palette.large_font, palette.foreground, true);
        fill(
            dc,
            Rect {
                top: rect.bottom - 1,
                ..rect
            },
            palette.border,
        );
    }

    fn tabs_mouse(&mut self, group: usize, message: u32, wp: usize, point: Point) {
        let tabs = self.chrome.tab_handle(group);
        let area = bounds(tabs);
        let (viewport, max) = self.tab_limits(group, area.width());
        if let Some((index, origin, previous)) = self.chrome.tab_drag {
            let moved = (point.x - origin.x).abs().max((point.y - origin.y).abs()) >= self.px(7);
            match message {
                WM_MOUSEMOVE if moved => {
                    if area.has(point)
                        || self.other_tab_at(tabs, point).is_some()
                        || self.transfer_target(tabs, point).is_some()
                    {
                        self.clear_dock();
                    } else {
                        let target = self.docking_at(tabs, point, false, false);
                        self.show_dock(target);
                    }
                }
                WM_LBUTTONUP => {
                    self.chrome.tab_drag = None;
                    self.clear_dock();
                    unsafe {
                        ReleaseCapture();
                    }
                    if moved && index < self.documents.len() {
                        if area.has(point) {
                            if let Some((to, _)) = self.tab_target(group, point, viewport) {
                                self.move_document(index, to);
                            }
                        } else if let Some(other_group) = self.other_tab_at(tabs, point) {
                            self.move_to_group(index, other_group);
                        } else if let Some(target) = self.transfer_target(tabs, point) {
                            if let Err(error) = self.move_document_to_window(index, target) {
                                self.show_error(error);
                            }
                        } else {
                            let target = self.docking_at(tabs, point, false, false);
                            if target == dock::Destination::Window {
                                if let Err(error) = self.detach_document(index) {
                                    self.show_error(error);
                                }
                            } else {
                                self.split_document(index, target, previous);
                            }
                        }
                    }
                    repaint(tabs);
                    return;
                }
                WM_CAPTURECHANGED => {
                    self.chrome.tab_drag = None;
                    self.clear_dock();
                }
                _ => {}
            }
        }
        match message {
            WM_MOUSEMOVE | WM_MOUSELEAVE => {
                let target = |point: Point| {
                    if point.x >= area.right - self.px(32) {
                        (None, Some(2))
                    } else if max > 0 && point.x >= viewport {
                        (None, Some(i32::from(point.x >= viewport + self.px(29))))
                    } else {
                        (self.tab_target(group, point, viewport), None)
                    }
                };
                let next = (message == WM_MOUSEMOVE).then_some(point);
                let old = self.chrome.tab_hover[group].map(target);
                let current = next.map(target);
                self.chrome.tab_hover[group] = next;
                if old == current {
                    return;
                }
                let old_tab = old.and_then(|hit| hit.0);
                let new_tab = current.and_then(|hit| hit.0);
                for (offset, filter) in [(0, false), (1, true)] {
                    let tab = |hit: Option<(usize, bool)>| {
                        hit.filter(|&(_, close)| !filter || close).map(|hit| hit.0)
                    };
                    let from = tab(old_tab);
                    let to = tab(new_tab);
                    if from == to {
                        continue;
                    }
                    for index in from.into_iter().chain(to) {
                        if let Some(rect) = self.tab_cell(index) {
                            let on = Some(index) == to;
                            motion::animate(
                                self.window,
                                Key::item(tabs, index * 2 + offset, Effect::Hover),
                                Area::Rect(rect),
                                !on,
                                on,
                            );
                        }
                    }
                }
                if old.and_then(|hit| hit.1) != current.and_then(|hit| hit.1) {
                    repaint_rect(
                        tabs,
                        Rect {
                            left: viewport,
                            ..bounds(tabs)
                        },
                    );
                }
                return;
            }
            WM_MOUSEWHEEL => {
                motion::clear(tabs);
                let delta = (wp >> 16) as i16 as i32;
                self.chrome.tab_offset[group] =
                    (self.chrome.tab_offset[group] - delta).clamp(0, max);
            }
            WM_LBUTTONDOWN | WM_MBUTTONDOWN => {
                if message == WM_LBUTTONDOWN && point.x >= area.right - self.px(32) {
                    if let Some(index) = self.group_document(group) {
                        self.activate(index, false);
                    }
                    self.command(browser::NEW_TAB);
                } else if point.x >= viewport && max > 0 {
                    motion::clear(tabs);
                    let step = self.px(132)
                        * if point.x < viewport + self.px(29) {
                            -1
                        } else {
                            1
                        };
                    self.chrome.tab_offset[group] =
                        (self.chrome.tab_offset[group] + step).clamp(0, max);
                } else if let Some((index, close)) = self.tab_target(group, point, viewport) {
                    if close || message == WM_MBUTTONDOWN {
                        self.close_document(index);
                    } else {
                        let previous = self.active;
                        self.activate(index, true);
                        self.chrome.tab_drag = Some((index, point, previous));
                        unsafe {
                            SetCapture(tabs);
                        }
                    }
                }
            }
            _ => return,
        }
        repaint(tabs);
    }

    fn other_tab_at(&self, from: Handle, point: Point) -> Option<usize> {
        let group = 1 - self.chrome.tab_group(from)?;
        let target = self.chrome.tab_handle(group);
        (self.other.is_some() && bounds(target).has(map_point(from, target, point)))
            .then_some(group)
    }

    fn move_to_group(&mut self, index: usize, group: usize) {
        let source = self.documents[index].group;
        let Some(existing) = self.group_document(group) else {
            return;
        };
        if let Some(neighbor) = self
            .documents
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, doc)| (i != index && doc.group == source).then_some(i))
        {
            self.documents[index].group = group;
            self.active = Some(index);
            self.active_right = group == 1;
            self.other = Some(neighbor);
            self.clear_tab_motion();
            self.activate(index, true);
        } else {
            self.collapse_editor_groups();
            self.activate(existing, false);
            self.activate(index, true);
        }
    }

    pub(super) fn open_menu(&mut self, owner: usize, rows: &[(usize, &str)]) {
        if self
            .chrome
            .menu
            .as_ref()
            .is_some_and(|menu| menu.owner == owner && menu.completion.is_none())
        {
            self.close_menu(true);
            return;
        }
        let previous_focus = self
            .chrome
            .menu
            .as_ref()
            .map_or_else(|| unsafe { GetFocus() }, |menu| menu.previous_focus);
        if let Some(previous) = self.menu_owner().filter(|&id| id != 0 && id != owner) {
            self.animate_menu_button(previous, false);
        }
        self.chrome.closing_menu = None;
        motion::clear(self.chrome.popup);
        self.chrome.menu = Some(Menu {
            owner,
            rows: rows
                .iter()
                .map(|&(id, text)| (id, text.to_owned()))
                .collect(),
            active: None,
            scroll: 0,
            completion: None,
            previous_focus,
        });
        self.position_menu();
        self.animate_menu_button(owner, true);
        self.animate_menu_reveal(true);
        unsafe {
            SetFocus(self.chrome.popup);
        }
    }

    pub(super) fn show_suggestions(&mut self, suggestions: Suggestions) {
        let opening = self.suggestion_editor() != Some(suggestions.editor);
        if opening {
            self.chrome.closing_menu = None;
            motion::clear(self.chrome.popup);
        }
        let selected = self.chrome.menu.as_ref().and_then(|menu| {
            let current = menu.completion.as_ref()?;
            if current.editor != suggestions.editor {
                return None;
            }
            let candidate = current.candidates.get(menu.active?)?;
            suggestions
                .candidates
                .iter()
                .position(|new| new.label == candidate.label && new.kind == candidate.kind)
        });
        let rows = suggestions
            .candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                (
                    index + 1,
                    format!("{}\t{}", candidate.label, candidate.kind.label()),
                )
            })
            .collect();
        self.chrome.menu = Some(Menu {
            owner: 0,
            rows,
            active: selected.or(Some(0)),
            scroll: 0,
            previous_focus: suggestions.editor,
            completion: Some(suggestions),
        });
        self.position_menu();
        if opening {
            self.animate_menu_reveal(true);
        }
        self.reveal_menu_row();
    }

    fn animate_menu_button(&self, id: usize, open: bool) {
        let window = self.toolbar_handle(id);
        motion::animate(
            self.window,
            Key::control(window, Effect::Selected),
            Area::Control,
            !open,
            open,
        );
    }

    fn animate_menu_reveal(&self, open: bool) {
        motion::animate(
            self.window,
            Key::control(self.chrome.popup, Effect::Reveal),
            Area::Control,
            !open,
            open,
        );
    }

    pub(super) fn suggestion_editor(&self) -> Option<Handle> {
        Some(self.chrome.menu.as_ref()?.completion.as_ref()?.editor)
    }

    fn menu_geometry(&self, menu: &Menu) -> (i32, i32) {
        let available = bounds(self.window);
        let width = self
            .px(if menu.completion.is_some() { 264 } else { 302 })
            .min(available.width());
        let height = self.menu_height(menu);
        (width, height)
    }

    fn menu_height(&self, menu: &Menu) -> i32 {
        self.px(8)
            + menu
                .rows
                .iter()
                .map(|row| self.px(if row.0 == 0 { 9 } else { 27 }))
                .sum::<i32>()
    }

    pub(super) fn position_menu(&self) {
        let Some(menu) = self.chrome.menu.as_ref() else {
            return;
        };
        let available = bounds(self.window);
        let (width, height) = self.menu_geometry(menu);
        let anchor = if let Some(suggestions) = &menu.completion {
            let editor = suggestions.editor;
            let mut caret = Point::default();
            send(
                editor,
                0x00d6,
                &mut caret as *mut _ as usize,
                suggestions.caret as isize,
            );
            let caret = map_point(editor, self.window, caret);
            Rect::from_size(caret.x, caret.y, 1, self.px(self.settings.points + 9))
        } else {
            relative_rect(self.toolbar_handle(menu.owner), self.window)
        };
        let below = (available.height() - anchor.bottom - self.px(3)).max(0);
        let above = (anchor.top - self.px(3)).max(0);
        let graph_picker = matches!(menu.owner, git::GRAPH_BRANCH | git::GRAPH_COMPARE);
        let opens_up =
            (graph_picker || menu.completion.is_some()) && height > below && above > below;
        let height = height.min(if opens_up { above } else { below }).min(
            if menu.completion.is_some() || graph_picker {
                self.px(224)
            } else {
                i32::MAX
            },
        );
        let top = if opens_up {
            anchor.top - height
        } else {
            anchor.bottom
        };
        let rect = Rect::from_size(
            anchor.left.clamp(0, available.width() - width),
            top.max(0),
            width,
            height,
        );
        if self.chrome.popup_bounds.replace(Some(rect)) != Some(rect) {
            show_overlay(self.chrome.popup, rect);
        }
    }

    pub(super) fn close_menu(&mut self, restore: bool) {
        if let Some(menu) = self.chrome.menu.take() {
            self.chrome.popup_bounds.set(None);
            if menu.completion.is_none() {
                self.animate_menu_button(menu.owner, false);
            }
            if restore && menu.previous_focus != 0 && menu.previous_focus != self.chrome.popup {
                unsafe {
                    SetFocus(menu.previous_focus);
                }
            } else if restore {
                self.focus_editor();
            }
            if motion::enabled() {
                self.chrome.closing_menu = Some(menu);
                self.animate_menu_reveal(false);
                self.finish_chrome_motion();
            } else {
                show(self.chrome.popup, false);
            }
        }
    }

    pub(super) fn animate_editor_tools(&mut self, open: bool) {
        let key = Key::control(self.editor_tools, Effect::Reveal);
        if open {
            self.cancel_editor_tools_motion();
        } else {
            for window in [
                self.mode_label,
                self.query,
                self.replacement,
                self.action,
                self.second,
                self.third,
                self.dismiss,
            ] {
                show(window, false);
            }
            self.chrome.closing_editor_tools = true;
        }
        motion::animate(self.window, key, Area::Control, !open, open);
        self.finish_chrome_motion();
    }

    pub(super) fn cancel_editor_tools_motion(&mut self) {
        if self.chrome.closing_editor_tools {
            self.chrome.closing_editor_tools = false;
            clear_clip(self.editor_tools);
        }
        motion::clear(self.editor_tools);
    }

    pub(super) fn finish_chrome_motion(&mut self) {
        if self.chrome.closing_menu.is_some()
            && !motion::active(Key::control(self.chrome.popup, Effect::Reveal))
        {
            self.chrome.closing_menu = None;
            show(self.chrome.popup, false);
        }
        if self.chrome.closing_editor_tools {
            let key = Key::control(self.editor_tools, Effect::Reveal);
            if motion::active(key) {
                let area = bounds(self.editor_tools);
                let height =
                    ((area.height() as f32 * motion::value(key, false)).round() as i32).max(1);
                clip_client(
                    self.editor_tools,
                    Rect {
                        top: area.bottom - height,
                        ..area
                    },
                );
            } else {
                show(self.editor_tools, false);
                self.cancel_editor_tools_motion();
            }
        }
    }

    pub(super) fn menu_owner(&self) -> Option<usize> {
        self.chrome.menu.as_ref().map(|menu| menu.owner)
    }

    pub(super) fn menu_key(&mut self, key: i32) -> bool {
        let Some(menu) = self.chrome.menu.as_mut() else {
            return false;
        };
        match key {
            0x1b => self.close_menu(true),
            13 | 32 | 9
                if key == 13
                    || key == 9 && menu.completion.is_some()
                    || key == 32 && menu.completion.is_none() =>
            {
                if let Some(index) = menu.active {
                    self.run_menu_index(index);
                }
            }
            0x24 | 0x23 | 0x26 | 0x28 => {
                let old = menu.active;
                let next = |index| {
                    menu.rows
                        .iter()
                        .enumerate()
                        .skip(index)
                        .find(|(_, row)| row.0 != 0)
                        .map(|(i, _)| i)
                };
                let prev = |end| {
                    menu.rows
                        .iter()
                        .enumerate()
                        .take(end)
                        .rev()
                        .find(|(_, row)| row.0 != 0)
                        .map(|(i, _)| i)
                };
                menu.active = match key {
                    0x24 => next(0),
                    0x23 => prev(menu.rows.len()),
                    0x26 => menu.active.and_then(prev).or_else(|| prev(menu.rows.len())),
                    _ => next(menu.active.map_or(0, |i| i + 1)).or_else(|| next(0)),
                };
                let next = menu.active;
                self.animate_menu_selection(old, next);
                self.reveal_menu_row();
            }
            0x25 | 0x27
                if menu.completion.is_none()
                    && BUTTONS.iter().any(|button| button.0 == menu.owner) =>
            {
                let index = BUTTONS
                    .iter()
                    .position(|button| button.0 == menu.owner)
                    .unwrap_or(0);
                let next = if key == 0x25 {
                    index + BUTTONS.len() - 1
                } else {
                    index + 1
                };
                self.command(BUTTONS[next % BUTTONS.len()].0);
            }
            _ => return false,
        }
        true
    }

    fn reveal_menu_row(&mut self) {
        let popup = self.chrome.popup;
        let viewport = bounds(popup).height();
        let dpi = self.dpi;
        let px = |value| value * dpi / 96;
        if let Some(menu) = self.chrome.menu.as_mut()
            && let Some(index) = menu.active
        {
            let top = px(4)
                + menu.rows[..index]
                    .iter()
                    .map(|row| px(if row.0 == 0 { 9 } else { 27 }))
                    .sum::<i32>();
            if top < menu.scroll + px(4) {
                menu.scroll = (top - px(4)).max(0);
            } else if top + px(27) > menu.scroll + viewport - px(4) {
                menu.scroll = top + px(31) - viewport;
            }
        }
        repaint(popup);
    }

    fn menu_mouse(&mut self, message: u32, wp: usize, point: Point) {
        let Some(menu) = self.chrome.menu.as_ref() else {
            return;
        };
        if message == WM_MOUSEWHEEL {
            let height = self.menu_height(menu);
            let max = (height - bounds(self.chrome.popup).height()).max(0);
            let delta = (wp >> 16) as i16 as i32;
            let scroll = (menu.scroll - self.px(delta) / 2).clamp(0, max);
            let menu = self.chrome.menu.as_mut().expect("visible menu");
            menu.active = None;
            menu.scroll = scroll;
            repaint(self.chrome.popup);
            return;
        }
        let mut y = self.px(4) - menu.scroll;
        let mut index = None;
        for (i, (id, _)) in menu.rows.iter().enumerate() {
            let h = self.px(if *id == 0 { 9 } else { 27 });
            if *id != 0 && (y..y + h).contains(&point.y) {
                index = Some(i);
                break;
            }
            y += h;
        }
        if message == WM_LBUTTONUP && bounds(self.chrome.popup).has(point) {
            if let Some(i) = index {
                self.run_menu_index(i);
            }
        } else if matches!(message, WM_MOUSEMOVE | WM_MOUSELEAVE) {
            let next = if message == WM_MOUSELEAVE {
                None
            } else {
                index
            };
            if next != menu.active {
                let previous = menu.active;
                self.chrome.menu.as_mut().expect("visible menu").active = next;
                self.animate_menu_selection(previous, next);
            }
        }
    }

    fn animate_menu_selection(&self, old: Option<usize>, next: Option<usize>) {
        if old == next {
            return;
        }
        for index in old.into_iter().chain(next) {
            let selected = Some(index) == next;
            motion::animate(
                self.window,
                Key::item(self.chrome.popup, index, Effect::Hover),
                Area::Control,
                !selected,
                selected,
            );
        }
    }

    fn run_menu_index(&mut self, index: usize) {
        let Some(menu) = self.chrome.menu.as_ref() else {
            return;
        };
        let id = menu.rows[index].0;
        if let Some(suggestions) = &menu.completion {
            let selection_now = selection(suggestions.editor);
            if selection_now.start != suggestions.caret
                || selection_now.end != suggestions.caret
                || self.completion_pending == Some(suggestions.editor)
            {
                self.complete(false);
                return;
            }
            let editor = suggestions.editor;
            let range = suggestions.range;
            let choice = &suggestions.candidates[index];
            let caret = range.start + text::wide_offset(&choice.insert, choice.caret);
            let value = choice.insert.clone();
            self.close_menu(false);
            self.dismiss_completion_request();
            select(editor, range.start, range.end);
            self.insert(editor, &value);
            select(editor, caret, caret);
            send(editor, EM_SCROLLCARET, 0, 0);
            self.focus_editor();
            self.update_status();
        } else {
            self.close_menu(true);
            self.command(id);
        }
    }

    fn paint_menu(&self, dc: Handle, rect: Rect, palette: Paint) {
        let Some(menu) = self
            .chrome
            .menu
            .as_ref()
            .or(self.chrome.closing_menu.as_ref())
        else {
            return;
        };
        let shown = motion::value(
            Key::control(self.chrome.popup, Effect::Reveal),
            self.chrome.menu.is_some(),
        );
        let background = motion::mix(palette.panel_rgb, palette.menu_rgb, shown);
        let text_tone = |color| motion::mix(background, color, shown);
        fill_color(dc, rect, text_tone(palette.border_rgb));
        fill_color(
            dc,
            Rect::from_size(1, 1, rect.width() - 2, rect.height() - 2),
            background,
        );
        let mut y = self.px(4) - menu.scroll - ((1.0 - shown) * self.px(4) as f32).round() as i32;
        for (index, (id, text)) in menu.rows.iter().enumerate() {
            let height = self.px(if *id == 0 { 9 } else { 27 });
            let row = Rect::from_size(self.px(4), y, rect.width() - 2 * self.px(4), height);
            if *id == 0 {
                fill_color(
                    dc,
                    Rect::from_size(self.px(9), y + height / 2, rect.width() - self.px(18), 1),
                    text_tone(palette.border_rgb),
                );
            } else {
                let selected = motion::value(
                    Key::item(self.chrome.popup, index, Effect::Hover),
                    menu.active == Some(index),
                );
                if selected > 0.0 {
                    fill_color(
                        dc,
                        row,
                        motion::mix(background, palette.selected_rgb, selected * shown),
                    );
                }
                let label = Rect {
                    left: self.px(14),
                    right: rect.right - self.px(14),
                    ..row
                };
                if let Some((name, shortcut)) = text.split_once('\t') {
                    let separator =
                        rect.right - self.px(if menu.completion.is_some() { 60 } else { 108 });
                    draw_label(
                        dc,
                        Rect {
                            right: separator,
                            ..label
                        },
                        name,
                        palette.font,
                        text_tone(palette.foreground),
                        false,
                    );
                    draw_label(
                        dc,
                        Rect {
                            left: separator,
                            ..label
                        },
                        shortcut,
                        palette.font,
                        text_tone(palette.muted),
                        false,
                    );
                } else {
                    draw_label(
                        dc,
                        label,
                        text,
                        palette.font,
                        text_tone(palette.foreground),
                        false,
                    );
                }
            }
            y += height;
        }
    }
}

pub(super) fn file_icon(path: Option<&Path>) -> usize {
    let Some(path) = path else {
        return 0;
    };
    let name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
    if name.eq_ignore_ascii_case("Cargo.toml") {
        return 15;
    }
    if name.eq_ignore_ascii_case("Cargo.lock")
        || matches!(name, "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock")
    {
        return 16;
    }
    if name.to_ascii_lowercase().starts_with("readme") {
        return 19;
    }
    match path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => 3,
        "js" | "jsx" | "mjs" | "cjs" => 4,
        "ts" | "tsx" | "mts" | "cts" => 5,
        "py" | "pyi" | "pyw" => 6,
        "json" | "jsonc" | "toml" | "yaml" | "yml" | "ini" | "lock" | "xml" => 7,
        "html" | "htm" | "svg" | "css" | "less" | "scss" => 8,
        "md" | "markdown" | "txt" | "rst" => 9,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "ico" | "bmp" => 10,
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" => 17,
        "exe" | "msi" | "dll" | "com" => 18,
        "sh" | "ps1" | "cmd" | "bat" => 11,
        "c" | "h" | "cpp" | "hpp" | "cs" | "java" | "go" | "rb" | "php" | "sql" => 12,
        _ if name.starts_with('.') || name.eq_ignore_ascii_case("Dockerfile") => 7,
        _ => 0,
    }
}

pub(super) fn folder_icon(path: &Path, open: bool) -> usize {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let type_id = match name.as_str() {
        "src" | "source" => 1,
        "app" | "apps" => 2,
        "target" | "build" | "dist" => 3,
        "debug" => 4,
        "release" => 5,
        "test" | "tests" | "__tests__" => 6,
        ".git" => 7,
        "assets" | "images" | "public" | "static" => 8,
        _ => 0,
    };
    32 + type_id * 2 + usize::from(open)
}

fn folder_shape(code: usize) -> bool {
    (32..=49).contains(&code)
}

pub(super) fn icon(dc: Handle, point: Point, code: usize, palette: Paint, dpi: i32, clip: Rect) {
    let size = 18 * dpi / 96;
    let rect = Rect::from_size(point.x, point.y - size / 2, size, size);
    if rect.left < clip.left || rect.right > clip.right {
        return;
    }
    let (glyph, tone) = match code {
        4 => ("JS", 2),
        5 => ("TS", 3),
        6 => ("Py", 3),
        7 => ("{}", 2),
        8 => ("<>", 1),
        9 => ("≡", 3),
        11 => (">_", 5),
        12 => ("# ", 4),
        13 => ("!", 1),
        _ => ("□", 0),
    };
    if folder_shape(code) {
        let folder = (code - 32) / 2;
        let open = !code.is_multiple_of(2);
        let colors = [
            rgb(141, 167, 179),
            rgb(123, 203, 131),
            rgb(239, 139, 138),
            rgb(125, 175, 213),
            rgb(246, 174, 85),
            rgb(226, 128, 164),
            rgb(178, 149, 226),
            rgb(238, 144, 103),
            rgb(93, 191, 185),
        ];
        let badge = ["", "‹›", "▪", "◎", "›", "↑", "✓", "•", "▧"][folder];
        let color = colors[folder];
        let p = |value| value * dpi / 96;
        let inner = Rect::from_size(rect.left + p(2), rect.top + p(6), p(14), p(9));
        fill_color(
            dc,
            Rect::from_size(inner.left, inner.top - p(2), p(6), p(2)),
            color,
        );
        fill_color(dc, inner, color);
        if open {
            fill_color(
                dc,
                Rect::from_size(inner.left + p(2), inner.top + p(2), p(13), p(1)),
                palette.editor_rgb,
            );
            fill_color(
                dc,
                Rect::from_size(inner.left + p(2), inner.top + p(3), p(13), p(5)),
                color,
            );
        }
        if !badge.is_empty() {
            draw_label(
                dc,
                Rect::from_size(inner.left + p(4), inner.top - p(1), p(10), p(11)),
                badge,
                palette.icon_font,
                rgb(27, 43, 51),
                true,
            );
        }
    } else if matches!(code, 50 | 51) {
        paint_file_mark(dc, rect, 17, palette, dpi);
    } else if matches!(code, 3 | 10 | 15..=19) {
        paint_file_mark(dc, rect, code, palette, dpi);
    } else {
        draw_label(
            dc,
            rect,
            glyph,
            palette.icon_font,
            palette.icons[tone],
            true,
        );
    }
}

fn paint_file_mark(dc: Handle, rect: Rect, code: usize, palette: Paint, dpi: i32) {
    let size = |value: i32| value * dpi / 96;
    let pixel = |x, y, width, height, color| {
        fill_color(
            dc,
            Rect::from_size(
                rect.left + size(x),
                rect.top + size(y),
                size(width).max(1),
                size(height).max(1),
            ),
            color,
        );
    };
    let background = palette.editor_rgb;
    match code {
        3 => {
            let orange = if palette.editor_rgb == rgb(24, 29, 35) {
                rgb(249, 155, 103)
            } else {
                rgb(186, 74, 26)
            };
            for (x, y, w, h) in [
                (5, 3, 8, 12),
                (3, 5, 12, 8),
                (8, 1, 2, 3),
                (8, 14, 2, 3),
                (1, 8, 3, 2),
                (14, 8, 3, 2),
                (3, 3, 3, 3),
                (12, 3, 3, 3),
                (3, 12, 3, 3),
                (12, 12, 3, 3),
            ] {
                pixel(x, y, w, h, orange);
            }
            pixel(5, 5, 8, 8, background);
            draw_label(dc, rect, "R", palette.icon_font, orange, true);
        }
        10 => {
            let green = rgb(74, 174, 163);
            pixel(3, 2, 12, 14, green);
            pixel(5, 4, 8, 10, background);
            pixel(6, 5, 2, 2, green);
            for (x, y, w, h) in [(6, 12, 6, 1), (7, 11, 5, 1), (9, 10, 3, 1), (10, 9, 2, 1)] {
                pixel(x, y, w, h, green);
            }
        }
        15 => {
            let coral = rgb(212, 119, 122);
            pixel(2, 2, 14, 14, coral);
            pixel(3, 3, 12, 12, background);
            draw_label(dc, rect, "T", palette.icon_font, palette.foreground, true);
        }
        16 => {
            let yellow = rgb(235, 179, 68);
            pixel(6, 3, 6, 2, yellow);
            pixel(5, 5, 2, 4, yellow);
            pixel(11, 5, 2, 4, yellow);
            pixel(4, 8, 10, 7, yellow);
            pixel(8, 10, 2, 3, rgb(50, 45, 33));
        }
        17 => {
            let olive = rgb(164, 175, 67);
            pixel(4, 2, 10, 14, olive);
            for y in 4..=12 {
                pixel(8 + y % 2, y, 1, 1, rgb(49, 55, 35));
            }
            pixel(8, 13, 3, 2, rgb(49, 55, 35));
        }
        18 => {
            let coral = rgb(229, 122, 104);
            pixel(2, 4, 14, 10, coral);
            pixel(3, 7, 12, 6, background);
            pixel(4, 5, 1, 1, background);
            pixel(6, 5, 1, 1, background);
        }
        19 => {
            let blue = rgb(95, 178, 243);
            pixel(5, 2, 8, 14, blue);
            pixel(3, 4, 12, 10, blue);
            pixel(8, 5, 2, 2, rgb(24, 43, 55));
            pixel(8, 8, 2, 5, rgb(24, 43, 55));
        }
        _ => {}
    }
}

pub(super) fn paint_file_row(item: &DrawItem, palette: Paint, dpi: i32) {
    let selected = motion::value(
        Key::item(item.window, item.item as usize, Effect::Selected),
        item.state & 1 != 0,
    );
    let background = file_row_tint(item, palette);
    fill_color(item.dc, item.rect, background);
    if item.item == u32::MAX {
        return;
    }
    let px = |n| n * dpi / 96;
    let depth = (item.data >> 8).min(40) as i32;
    let code = item.data & 255;
    if code == 14 {
        fill(item.dc, item.rect, palette.chrome);
        let label = list_label(item.window, item.item as usize);
        draw_label(
            item.dc,
            Rect {
                left: item.rect.left + px(9),
                right: item.rect.right - px(4),
                ..item.rect
            },
            &label,
            palette.font,
            palette.muted,
            false,
        );
        return;
    }
    if selected > 0.0 {
        fill_color(
            item.dc,
            Rect {
                right: item.rect.left + px(2),
                ..item.rect
            },
            motion::mix(background, palette.accent, selected),
        );
    }
    let x = item.rect.left + px(20) + depth * px(14);
    if depth > 0 {
        for level in 0..depth {
            let x = item.rect.left + px(15) + level * px(14);
            fill(
                item.dc,
                Rect::from_size(x, item.rect.top, 1, item.rect.height()),
                palette.border,
            );
        }
    }
    let center_y = (item.rect.top + item.rect.bottom) / 2;
    let tree = folder_shape(code) || matches!(code, 50 | 51);
    if tree {
        let open = motion::value(
            Key::item(item.window, item.item as usize, Effect::Arrow),
            !code.is_multiple_of(2),
        );
        chevron(
            item.dc,
            Point {
                x: x - px(10),
                y: center_y,
            },
            px(3).max(2),
            open,
            palette.muted,
        );
    }
    icon(
        item.dc,
        Point { x, y: center_y },
        code,
        palette,
        dpi,
        item.rect,
    );
    let label = list_label(item.window, item.item as usize);
    let cell = Rect {
        left: x + px(25),
        right: item.rect.right - px(5),
        ..item.rect
    };
    if cell.width() > 0 {
        draw_label(
            item.dc,
            cell,
            &label,
            palette.font,
            motion::mix(
                palette.muted,
                palette.foreground,
                if tree { 1.0 } else { selected },
            ),
            false,
        );
    }
}

pub(super) fn chevron(dc: Handle, center: Point, radius: i32, open: f32, color: u32) {
    let (sin, cos) = (open * std::f32::consts::FRAC_PI_2).sin_cos();
    let point = |x: f32, y: f32| Point {
        x: center.x + ((x * cos - y * sin) * radius as f32).round() as i32,
        y: center.y + ((x * sin + y * cos) * radius as f32).round() as i32,
    };
    let tip = point(0.7, 0.0);
    stroke_line(dc, point(-0.6, -1.0), tip, color);
    stroke_line(dc, point(-0.6, 1.0), tip, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumb_scales_to_both_limits() {
        let info = ScrollInfo::sample(0, 99999, 800, 49600);
        let thumb = Thumb::measure(info, 300, 20).unwrap();
        assert_eq!(thumb.start, 140);
        assert_eq!(thumb.position(0), 0);
        assert_eq!(thumb.position(280), 99200);
    }
}
