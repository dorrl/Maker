mod archive;
mod browser;
mod chrome;
mod data_view;
mod dock;
mod explorer;
mod file_actions;
mod git;
mod gutter;
mod highlight;
mod motion;
mod preferences;
mod session;
mod terminal;

use crate::{
    i18n,
    text::{self, Color, Encoding, Language, Settings},
    theme::Theme,
    win::*,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    ffi::OsStr,
    fs, io,
    mem::zeroed,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

const WINDOW_CLASS: &str = "MakerNativeWindow";
const COLOR_TIMER: usize = 1;
const COMPLETION_TIMER: usize = 5;
const VIEWPORT_TIMER: usize = 9;
const SEARCH_TIMER: usize = 2;
const EDITOR: usize = 300;
const TABS: usize = 301;
const LIST: usize = 302;
const QUERY: usize = 303;
const REPLACEMENT: usize = 304;
const GUTTER: usize = 305;
const OTHER_GUTTER: usize = 387;
const ACTION: usize = 306;
const SECOND: usize = 307;
const THIRD: usize = 308;
const DISMISS: usize = 309;
const HIGHLIGHT_LIMIT: usize = 160_000;
const HIGHLIGHT_WINDOW: i32 = 24_000;
const HIGHLIGHT_MARGIN: i32 = 256;
const BUTTONS: &[(usize, &str, i32)] = &[
    (FILE_MENU, "ui.file", 43),
    (EDIT_MENU, "ui.edit", 43),
    (TOOLS, "ui.view", 43),
    (terminal::MENU, "ui.terminal", 62),
    (PREFERENCES, "ui.settings", 43),
];

const NEW: usize = 100;
const OPEN: usize = 101;
const FOLDER: usize = 102;
const SAVE: usize = 103;
const SAVE_AS: usize = 104;
const SAVE_ALL: usize = 105;
const CLOSE: usize = 106;
const REOPEN: usize = 107;
const FIND: usize = 108;
const REPLACE: usize = 109;
const WORKSPACE: usize = 110;
const QUICK: usize = 111;
const GOTO: usize = 112;
const EXPLORER: usize = 113;
const REFRESH: usize = 114;
const CLOSE_FOLDER: usize = 115;
const PROBLEMS: usize = 116;
const TOOLS: usize = 117;
const PREFERENCES: usize = 118;
const THEME: usize = 119;
const FONT: usize = 120;
const BIGGER: usize = 121;
const SMALLER: usize = 122;
const RESET_FONT: usize = 123;
const WRAP: usize = 124;
const INDENT_TWO: usize = 125;
const INDENT_FOUR: usize = 126;
const INDENT_TAB: usize = 127;
const SPLIT: usize = 128;
const FILE_MENU: usize = 129;
const EDIT_MENU: usize = 130;
const DOCK_SIDEBAR: usize = 131;
const COLLAPSE_FOLDERS: usize = 132;
const FULLSCREEN: usize = 133;
const SIDEBAR_TITLE: usize = 312;
const STATUS: usize = 313;
const LOCATION: usize = 314;
const EDITOR_TOOLS: usize = 315;
const WELCOME: usize = 370;
const WELCOME_OPEN: usize = 371;
const WELCOME_FOLDER: usize = 372;
const WELCOME_NEW: usize = 373;
const NEW_WINDOW: usize = 374;

#[derive(Clone, Copy)]
struct Paint {
    foreground: u32,
    muted: u32,
    accent: u32,
    background: Handle,
    panel: Handle,
    chrome: Handle,
    menu: Handle,
    control: Handle,
    selected: Handle,
    border: Handle,
    accent_brush: Handle,
    thumb: Handle,
    font: Handle,
    gutter_font: Handle,
    gutter_cell_width: i32,
    gutter_width: i32,
    gutter_height: i32,
    large_font: Handle,
    icon_font: Handle,
    editor_rgb: u32,
    chrome_rgb: u32,
    panel_rgb: u32,
    menu_rgb: u32,
    control_rgb: u32,
    selected_rgb: u32,
    border_rgb: u32,
    thumb_rgb: u32,
    thumb_hover_rgb: u32,
    icons: [u32; 6],
    syntax_colors: [u32; 15],
    diff_backgrounds: [u32; 2],
    dpi: i32,
}

impl Paint {
    fn highlight(self, color: Color) -> u32 {
        self.syntax_colors[crate::theme::syntax_index(color)]
    }

    fn new(settings: &Settings, dpi: i32, window: Handle) -> Self {
        let colors = settings.theme.palette();
        let background = colors.get("background");
        let panel = colors.get("panel");
        let chrome = colors.get("chrome");
        let menu = colors.get("menu");
        let control = colors.get("control");
        let selected = colors.get("selected");
        let border = colors.get("border");
        let accent = colors.get("accent");
        let thumb = colors.get("thumb");
        let thumb_hover = colors.get("thumb_hover");
        let foreground = colors.get("foreground");
        let muted = colors.get("muted");
        let icons = colors.icons();
        let gutter_font = named_font(&settings.font, settings.points * dpi / 72);
        let gutter_cell = cell_size(window, gutter_font);
        Self {
            foreground,
            muted,
            accent,
            background: brush(background),
            panel: brush(panel),
            chrome: brush(chrome),
            menu: brush(menu),
            control: brush(control),
            selected: brush(selected),
            border: brush(border),
            accent_brush: brush(accent),
            thumb: brush(thumb),
            font: font(13 * dpi / 96),
            gutter_font,
            gutter_cell_width: gutter_cell.x,
            gutter_width: gutter_cell.x * 8 + 14 * dpi / 96,
            gutter_height: gutter_cell.y.max(1),
            large_font: font(22 * dpi / 96),
            icon_font: font(11 * dpi / 96),
            editor_rgb: background,
            chrome_rgb: chrome,
            panel_rgb: panel,
            menu_rgb: menu,
            control_rgb: control,
            selected_rgb: selected,
            border_rgb: border,
            thumb_rgb: thumb,
            thumb_hover_rgb: thumb_hover,
            icons,
            syntax_colors: colors.syntax(),
            diff_backgrounds: colors.diff_backgrounds(),
            dpi,
        }
    }

    fn release(self) {
        for object in [
            self.background,
            self.panel,
            self.chrome,
            self.menu,
            self.control,
            self.selected,
            self.border,
            self.accent_brush,
            self.thumb,
            self.font,
            self.gutter_font,
            self.large_font,
            self.icon_font,
        ] {
            unsafe { DeleteObject(object) };
        }
    }
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static PAINT: Cell<Option<Paint>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Find,
    Replace,
    Workspace,
    Quick,
    Goto,
    Rename,
}

impl Mode {
    fn floats(self) -> bool {
        matches!(self, Self::Find | Self::Replace | Self::Rename)
    }
}

struct Document {
    editor: Handle,
    group: usize,
    revision: u64,
    path: Option<PathBuf>,
    label: String,
    encoding: Encoding,
    language: Language,
    dirty: bool,
    highlighting: Option<highlight::Highlighting>,
    highlight_pending: bool,
    colored_view: Option<Selection>,
    git_diff: Option<(PathBuf, bool)>,
    archive_member: Option<(PathBuf, PathBuf)>,
    browser: Option<browser::Browser>,
    response: bool,
    response_url: Option<String>,
    data_view: Option<data_view::View>,
}

impl Document {
    fn data_mode(&self) -> data_view::ViewMode {
        self.data_view
            .as_ref()
            .map_or(data_view::ViewMode::Raw, data_view::View::mode)
    }

    fn source_visible(&self) -> bool {
        self.browser.is_none() && self.data_mode() == data_view::ViewMode::Raw
    }

    fn is_preview(&self) -> bool {
        self.git_diff.is_some() || self.archive_member.is_some() || self.response
    }
}

enum ListItem {
    Draft,
    File(PathBuf, Option<usize>),
    Folder {
        path: PathBuf,
        depth: usize,
        open: bool,
    },
    Archive {
        path: PathBuf,
        depth: usize,
        open: bool,
    },
    ArchiveMember {
        archive: PathBuf,
        path: PathBuf,
        depth: usize,
        directory: bool,
        open: bool,
    },
    Match {
        path: PathBuf,
        line: usize,
        column: usize,
        excerpt: String,
    },
    Issue {
        index: usize,
        line: usize,
        kind: text::BracketIssue,
    },
    Git(git::Change),
    Group(String),
}

struct Search {
    receiver: mpsc::Receiver<Vec<ListItem>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Search {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

struct App {
    window: Handle,
    toolbar: Vec<(Handle, i32)>,
    data_buttons: [[Handle; 4]; 2],
    chrome: chrome::Chrome,
    dock: dock::Dock,
    secondary_browser: Option<browser::Browser>,
    detached: bool,
    terminal: terminal::Panel,
    explorer: explorer::Explorer,
    git: git::Panel,
    sidebar_title: Handle,
    list: Handle,
    gutter: Handle,
    other_gutter: Handle,
    gutter_caret: Cell<Option<(Handle, usize)>>,
    gutter_scroll: [(Handle, isize, i32); 2],
    status: Handle,
    location: Handle,
    mode_label: Handle,
    editor_tools: Handle,
    welcome: Handle,
    welcome_actions: [Handle; 3],
    query: Handle,
    replacement: Handle,
    action: Handle,
    second: Handle,
    third: Handle,
    dismiss: Handle,
    documents: Vec<Document>,
    active: Option<usize>,
    other: Option<usize>,
    active_right: bool,
    window_title: RefCell<String>,
    untitled: usize,
    recently_closed: Vec<PathBuf>,
    root: Option<PathBuf>,
    files: Vec<PathBuf>,
    files_stale: bool,
    expanded: HashSet<PathBuf>,
    explorer_selection: Option<PathBuf>,
    explorer_top: usize,
    list_items: Vec<ListItem>,
    sidebar: bool,
    showing_issues: bool,
    mode: Option<Mode>,
    symbol_rename: Option<(Handle, text::symbols::Rename)>,
    completion_pending: Option<Handle>,
    last_find: String,
    search: Option<Search>,
    settings: Settings,
    pending_chord: bool,
    fresh_window: bool,
    fullscreen: Option<SavedWindow>,
    preferences_window: Handle,
    recent_file_list: Handle,
    dpi: i32,
}

pub fn run() -> io::Result<()> {
    let _ole = Ole::init()?;
    setup(WINDOW_CLASS, window_proc)?;
    chrome::register()?;
    terminal::register()?;
    preferences::register()?;
    let window = main_window(WINDOW_CLASS)?;
    let mut app = App::new(window)?;
    let mut args = std::env::args_os().skip(1).peekable();
    app.fresh_window = args.next_if(|arg| arg == "--new-window").is_some();
    let paths = args.map(PathBuf::from).collect::<Vec<_>>();
    let maximized = if paths.is_empty() && !app.fresh_window {
        app.restore_session()
    } else {
        app.open_paths(paths);
        false
    };
    let result = run_window(app, maximized);
    dock::wait_for_windows();
    result
}

fn run_window(mut app: App, maximized: bool) -> io::Result<()> {
    let window = app.window;
    let _registration = app.dock.register(window);
    let restore_fullscreen = app.fullscreen.is_some();
    let terminal_only = app.detached && app.terminal.visible && app.documents.is_empty();
    app.layout();
    APP.with_borrow_mut(|slot| *slot = Some(app));
    show_main_window(window, maximized);
    if restore_fullscreen {
        fullscreen(window, None);
    }
    unsafe {
        UpdateWindow(window);
    }
    with_app(|app| {
        if terminal_only {
            app.focus_terminal();
        } else {
            app.focus_editor();
        }
    });
    let mut message: Msg = unsafe { zeroed() };
    loop {
        match unsafe { GetMessageW(&mut message, 0, 0, 0) } {
            -1 => return Err(io::Error::last_os_error()),
            0 => break,
            _ => {}
        }
        if message.message == WM_MOUSEWHEEL
            && matches!(
                unsafe { GetDlgCtrlID(message.window) } as usize,
                EDITOR | data_view::PRETTY_EDITOR | GUTTER | OTHER_GUTTER
            )
        {
            merge_queued_wheel(&mut message);
        }
        if matches!(
            message.message,
            WM_LBUTTONDOWN | WM_MBUTTONDOWN | 0x0204 | 0x00a1
        ) {
            with_app(|app| {
                if app.menu_owner().is_some()
                    && message.window != app.chrome.popup
                    && !app
                        .toolbar
                        .iter()
                        .any(|&(handle, _)| handle == message.window)
                {
                    app.close_menu(true);
                }
                if ![
                    app.explorer.input,
                    app.explorer.frame,
                    app.explorer.file,
                    app.explorer.folder,
                    app.list,
                ]
                .contains(&message.window)
                {
                    app.cancel_explorer_edit(false);
                }
            });
        }
        let used = if matches!(message.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
            with_app(|app| {
                if preferences::has_focus(app.preferences_window, message.window) {
                    preferences::keyboard(app.preferences_window, message.wparam as i32)
                } else {
                    app.keyboard(message.window, message.wparam as i32)
                }
            })
            .unwrap_or(false)
        } else {
            false
        };
        if !used {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if matches!(message.message, WM_LBUTTONUP | WM_MBUTTONDOWN | 0x0204)
                && with_app(|app| message.window == app.list && app.explorer_editing())
                    .unwrap_or(false)
            {
                unsafe { PostMessageW(window, explorer::CANCEL_EDIT, 0, 0) };
            }
        }
    }
    APP.with_borrow_mut(|slot| {
        slot.take();
    });
    PAINT.with(|paint| {
        if let Some(old) = paint.take() {
            old.release();
        }
    });
    Ok(())
}

fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|slot| slot.try_borrow_mut().ok()?.as_mut().map(f))
}

extern "system" fn window_proc(window: Handle, message: u32, wp: usize, lp: isize) -> isize {
    if chrome::handle_parent_message(message, wp, lp) {
        return 0;
    }
    match message {
        data_view::READY => {
            with_app(|app| app.complete_data_view(wp as Handle));
            return 0;
        }
        data_view::TOGGLE => {
            with_app(|app| app.toggle_data_row(wp as Handle, lp as usize));
            return 0;
        }
        dock::WINDOW_DROP => {
            with_app(App::receive_window_drops);
            return 0;
        }
        dock::WINDOW_RECEIPT => {
            with_app(|app| app.finish_window_move(wp as Handle, lp != 0));
            return 0;
        }
        dock::WINDOW_PREVIEW => {
            with_app(|app| app.receive_tab_preview(wp, Point::packed(lp)));
            return 0;
        }
        browser::SEARCH_READY => {
            with_app(|app| app.complete_browser_search(wp as Handle));
            return 0;
        }
        browser::HTTP_READY => {
            with_app(|app| app.complete_http(wp as Handle));
            return 0;
        }
        browser::SELECT => {
            with_app(|app| {
                if let Err(error) =
                    app.browse_row(wp as Handle, lp as usize >> 2, lp & 2 != 0, lp & 1 != 0)
                {
                    app.show_error(error);
                }
            });
            return 0;
        }
        git::READY => {
            with_app(App::git_tick);
            return 0;
        }
        explorer::CANCEL_EDIT => {
            with_app(|app| app.cancel_explorer_edit(false));
            return 0;
        }
        0x0006 if wp & 0xffff == 0 => {
            with_app(|app| app.close_menu(false));
        }
        WM_DESTROY => {
            with_app(App::clear_dock);
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        WM_CLOSE => {
            if with_app(App::prepare_session_exit).unwrap_or(false) {
                unsafe {
                    DestroyWindow(window);
                }
            }
            return 0;
        }
        WM_SIZE => {
            with_app(|app| {
                if wp != 1 {
                    app.layout();
                }
                app.queue_session_save();
            });
            return 0;
        }
        WM_MOVE => {
            with_app(|app| app.queue_session_save());
            return 0;
        }
        WM_DPICHANGED => {
            let rect = unsafe { &*(lp as *const Rect) };
            unsafe {
                SetWindowPos(
                    window,
                    0,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    0x14,
                );
            }
            with_app(|app| {
                app.dpi = (wp & 0xffff) as i32;
                app.apply_preferences();
            });
            return 0;
        }
        WM_COMMAND => {
            let id = wp & 0xffff;
            let event = (wp >> 16) as u16;
            with_app(|app| app.on_command(id, event, lp));
            return 0;
        }
        WM_NOTIFY => {
            let notice = unsafe { &*(lp as *const Notify) };
            with_app(|app| app.on_notify(notice));
            return 0;
        }
        WM_TIMER => {
            if wp == motion::TIMER {
                motion::tick(window);
                with_app(|app| {
                    app.finish_chrome_motion();
                });
                return 0;
            }
            with_app(|app| match wp {
                COLOR_TIMER => app.repaint_editor(),
                VIEWPORT_TIMER => app.repaint_viewport(),
                chrome::SIDEBAR_RESIZE_TIMER => app.flush_sidebar_resize(),
                terminal::RESIZE_TIMER => app.flush_terminal_resize(),
                COMPLETION_TIMER => app.tick_completion(),
                SEARCH_TIMER => app.check_search(),
                browser::TIMER => app.start_browser_searches(),
                terminal::TIMER => app.terminal_tick(),
                archive::TIMER => app.archive_tick(),
                session::TIMER => {
                    let _ = app.save_session();
                }
                git::TIMER => app.git_tick(),
                _ => {}
            });
            return 0;
        }
        WM_DROPFILES => {
            let paths = dropped_files(wp as Handle);
            with_app(|app| app.open_paths(paths));
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_PAINT => {
            buffered_paint(window, |dc, rect| {
                let Some(palette) = PAINT.get() else {
                    return;
                };
                let px = |n| n * palette.dpi / 96;
                fill(dc, rect, palette.background);
                fill(
                    dc,
                    Rect {
                        bottom: px(34),
                        ..rect
                    },
                    palette.chrome,
                );
                fill(
                    dc,
                    Rect {
                        top: px(33),
                        bottom: px(34),
                        ..rect
                    },
                    palette.border,
                );
                let bottom = (rect.bottom - px(26)).max(0);
                fill(
                    dc,
                    Rect {
                        top: bottom,
                        ..rect
                    },
                    palette.chrome,
                );
                fill(
                    dc,
                    Rect {
                        top: bottom,
                        bottom: bottom + 1,
                        ..rect
                    },
                    palette.border,
                );
                with_app(|app| {
                    if app.sidebar {
                        let side = app.sidebar_size(rect.width());
                        let left = if app.settings.sidebar_right {
                            rect.right - side
                        } else {
                            0
                        };
                        fill(
                            dc,
                            Rect::from_size(left, px(34), side, bottom - px(34)),
                            palette.panel,
                        );
                    }
                });
            });
            return 0;
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
            if let Some(palette) = PAINT.get() {
                let id = unsafe { GetDlgCtrlID(lp) } as usize;
                let (brush, background, foreground) = match id {
                    SIDEBAR_TITLE | LIST | git::GRAPH_LIST | browser::RESULTS => {
                        (palette.panel, palette.panel_rgb, palette.foreground)
                    }
                    git::EXPLORER_VIEW | git::CHANGES_VIEW => {
                        (palette.chrome, palette.chrome_rgb, palette.foreground)
                    }
                    explorer::NEW_FILE
                    | explorer::NEW_FOLDER
                    | git::FETCH
                    | git::PULL
                    | git::PUSH => (palette.panel, palette.panel_rgb, palette.foreground),
                    _ if message == WM_CTLCOLORBTN => {
                        (palette.control, palette.control_rgb, palette.foreground)
                    }
                    browser::QUERY_ID => (palette.control, palette.control_rgb, palette.foreground),
                    STATUS | LOCATION => (palette.chrome, palette.chrome_rgb, palette.muted),
                    _ => (palette.background, palette.editor_rgb, palette.foreground),
                };
                unsafe {
                    SetTextColor(wp as Handle, foreground);
                    SetBkColor(wp as Handle, background);
                }
                return brush;
            }
        }
        WM_DRAWITEM => {
            if let Some(palette) = PAINT.get() {
                let item = unsafe { &*(lp as *const DrawItem) };
                match item.id as usize {
                    data_view::TREE => with_app(|app| app.paint_data_row(item, palette))
                        .unwrap_or_else(|| fill(item.dc, item.rect, palette.background)),
                    browser::RESULTS => chrome::paint_file_row(item, palette, palette.dpi),
                    browser::FIELD_FRAME | browser::HERO => {
                        with_app(|app| app.paint_browser(item, palette));
                    }
                    explorer::NEW_FILE | explorer::NEW_FOLDER => {
                        explorer::paint_button(item, palette)
                    }
                    git::EXPLORER_VIEW | git::CHANGES_VIEW => {
                        git::paint_sidebar_tab(item, palette);
                    }
                    git::FETCH | git::PULL | git::PUSH => git::paint_remote_button(item, palette),
                    EDITOR_TOOLS => {
                        let shade = motion::value(
                            motion::Key::control(item.window, motion::Effect::Reveal),
                            true,
                        );
                        let opening =
                            with_app(|app| app.mode.is_some_and(Mode::floats)).unwrap_or(true);
                        let border = if opening {
                            motion::mix(palette.accent, palette.border_rgb, shade)
                        } else {
                            palette.border_rgb
                        };
                        fill_color(item.dc, item.rect, border);
                        fill(
                            item.dc,
                            Rect {
                                left: item.rect.left + 1,
                                top: item.rect.top + 1,
                                right: item.rect.right - 1,
                                bottom: item.rect.bottom - 1,
                            },
                            palette.panel,
                        );
                    }
                    WELCOME => {
                        fill(item.dc, item.rect, palette.background);
                        let left = item.rect.left + 2;
                        let px = |n| n * palette.dpi / 96;
                        draw_label(
                            item.dc,
                            Rect::from_size(left, item.rect.top, item.rect.width(), px(36)),
                            "Maker",
                            palette.large_font,
                            palette.foreground,
                            false,
                        );
                        draw_label(
                            item.dc,
                            Rect::from_size(
                                left,
                                item.rect.top + px(40),
                                item.rect.width(),
                                px(24),
                            ),
                            crate::i18n::text("ui.code.editor.for.windows"),
                            palette.font,
                            palette.muted,
                            false,
                        );
                    }
                    LIST => chrome::paint_file_row(item, palette, palette.dpi),
                    git::GRAPH_LIST => {
                        with_app(|app| app.paint_graph_row(item, palette))
                            .unwrap_or_else(|| fill(item.dc, item.rect, palette.panel));
                    }
                    id => {
                        let toolbar = BUTTONS.iter().any(|button| button.0 == id);
                        let mode = (data_view::FIRST_MODE..=data_view::LAST_MODE).contains(&id);
                        let data_button = mode
                            || (data_view::OPEN_WEB_FIRST..=data_view::OPEN_WEB_LAST).contains(&id);
                        let active = with_app(|app| {
                            if mode {
                                app.data_mode_selected(id)
                            } else {
                                app.menu_owner() == Some(id)
                            }
                        })
                        .unwrap_or(false);
                        let base = if toolbar {
                            palette.chrome_rgb
                        } else {
                            palette.control_rgb
                        };
                        let tint = chrome::button_tint(item, palette, base, active);
                        if matches!(
                            id,
                            browser::WORK_FOLDER | browser::OPEN_FILE | browser::RECENT_FILE
                        ) {
                            fill(
                                item.dc,
                                item.rect,
                                if toolbar {
                                    palette.chrome
                                } else {
                                    palette.background
                                },
                            );
                            fill_rounded(item.dc, item.rect, (5 * palette.dpi / 96).max(1), tint);
                        } else {
                            fill_color(item.dc, item.rect, tint);
                            if data_button {
                                fill(
                                    item.dc,
                                    Rect {
                                        right: item.rect.left + 1,
                                        ..item.rect
                                    },
                                    palette.border,
                                );
                            }
                        }
                        draw_label(
                            item.dc,
                            item.rect,
                            &caption(item.window),
                            palette.font,
                            if active {
                                palette.accent
                            } else {
                                palette.foreground
                            },
                            true,
                        );
                    }
                }
                return 1;
            }
        }
        WM_SETTINGCHANGE => {
            motion::refresh(window);
            with_app(|app| {
                app.finish_chrome_motion();
                app.layout();
            });
        }
        _ => {}
    }
    unsafe { DefWindowProcW(window, message, wp, lp) }
}

impl App {
    fn new(window: Handle) -> io::Result<Self> {
        let settings = Settings::load();
        i18n::select(settings.locale);
        let static_label =
            |id, label| child(window, id, "STATIC", label, SS_CENTERIMAGE | SS_NOPREFIX);
        let button = |id, label| -> io::Result<_> {
            let control = child(window, id, "BUTTON", label, BS_OWNERDRAW | WS_TABSTOP)?;
            chrome::watch_control(control, true);
            Ok(control)
        };
        let input = |id| {
            child(
                window,
                id,
                "EDIT",
                "",
                WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
            )
        };
        let toolbar = BUTTONS
            .iter()
            .map(|&(id, label, size)| Ok((button(id, i18n::text(label))?, size)))
            .collect::<io::Result<Vec<_>>>()?;
        let mut app = Self {
            window,
            toolbar,
            data_buttons: data_view::pane_buttons(window)?,
            chrome: chrome::Chrome::new(window)?,
            dock: dock::Dock::new(window)?,
            secondary_browser: None,
            detached: false,
            terminal: terminal::Panel::new(window)?,
            explorer: explorer::Explorer::new(window)?,
            git: git::Panel::new(window)?,
            sidebar_title: static_label(SIDEBAR_TITLE, crate::i18n::text("ui.explorer"))?,
            list: child(
                window,
                LIST,
                "LISTBOX",
                "",
                LBS_NOTIFY
                    | LBS_EXTENDEDSEL
                    | LBS_OWNERDRAWFIXED
                    | LBS_HASSTRINGS
                    | LBS_NOINTEGRALHEIGHT
                    | LBS_DISABLENOSCROLL
                    | WS_TABSTOP
                    | WS_VSCROLL,
            )?,
            gutter: child(window, GUTTER, chrome::SURFACE, "", 0)?,
            other_gutter: child(window, OTHER_GUTTER, chrome::SURFACE, "", 0)?,
            gutter_caret: Cell::new(None),
            gutter_scroll: [(0, 0, 0); 2],
            status: static_label(
                STATUS,
                crate::i18n::text("ui.open.a.file.or.choose.a.workspace"),
            )?,
            location: static_label(LOCATION, "")?,
            mode_label: static_label(0, "")?,
            editor_tools: child(window, EDITOR_TOOLS, "STATIC", "", 0x000d)?,
            welcome: child(window, WELCOME, "STATIC", "Maker", 0x000d)?,
            welcome_actions: [
                button(WELCOME_OPEN, crate::i18n::text("ui.open.file.ctrl.o"))?,
                button(
                    WELCOME_FOLDER,
                    crate::i18n::text("ui.open.folder.ctrl.k.ctrl.o"),
                )?,
                button(WELCOME_NEW, crate::i18n::text("ui.new.file.ctrl.n"))?,
            ],
            query: input(QUERY)?,
            replacement: input(REPLACEMENT)?,
            action: button(ACTION, "")?,
            second: button(SECOND, "")?,
            third: button(THIRD, "")?,
            dismiss: button(DISMISS, crate::i18n::text("ui.close"))?,
            documents: Vec::new(),
            active: None,
            other: None,
            active_right: false,
            window_title: RefCell::new("Maker".into()),
            untitled: 0,
            recently_closed: Vec::new(),
            root: None,
            files: Vec::new(),
            files_stale: false,
            expanded: HashSet::new(),
            explorer_selection: None,
            explorer_top: 0,
            list_items: Vec::new(),
            sidebar: true,
            showing_issues: false,
            mode: None,
            symbol_rename: None,
            completion_pending: None,
            last_find: String::new(),
            search: None,
            settings,
            pending_chord: false,
            fresh_window: false,
            fullscreen: None,
            preferences_window: 0,
            recent_file_list: 0,
            dpi: unsafe { GetDpiForWindow(window) }.max(96) as i32,
        };
        unsafe {
            DragAcceptFiles(window, 1);
        }
        chrome::watch_control(app.list, false);
        app.refresh_locale_labels();
        app.apply_preferences();
        Ok(app)
    }

    fn px(&self, unit: i32) -> i32 {
        unit * self.dpi / 96
    }

    fn layout(&mut self) {
        self.layout_geometry();
        self.refresh_tab();
        repaint(self.gutter);
        repaint(self.other_gutter);
        repaint(self.window);
    }

    fn layout_geometry(&mut self) {
        let rect = bounds(self.window);
        batch_child_layout(self.window, || self.layout_primary_geometry(rect));
        self.clamp_tab_scroll();
        self.update_terminal_geometry();

        // These overlays use the actual client coordinates of their anchors.
        batch_child_layout(self.window, || {
            self.sync_scrollbars();
            self.layout_explorer_edit();
            self.layout_input(rect.width(), self.px(34) + self.px(6));
            self.position_menu();
        });
    }

    fn layout_primary_geometry(&mut self, rect: Rect) {
        let (width, height) = (rect.width(), rect.height());
        let bar = self.px(34);
        let step = self.px(6);
        let mut x = self.px(8);
        for &(button, size) in &self.toolbar {
            place(button, x, self.px(4), self.px(size), self.px(25));
            show(button, true);
            x += self.px(size + 2);
        }
        let mut top = bar;
        if self.mode.is_some_and(|mode| !mode.floats()) {
            top += self.px(30) + 2 * step;
        }
        let bottom = (height - self.px(26)).max(top);
        let side = if self.sidebar {
            self.sidebar_size(width)
        } else {
            0
        };
        let dock_right = self.settings.sidebar_right;
        let side_left = if dock_right { width - side } else { 0 };
        let divider = if self.sidebar { self.px(7) } else { 0 };
        let edge = if dock_right {
            side_left - divider
        } else {
            side
        };
        let editor_left = if dock_right { 0 } else { side + divider };
        let editor_width = (width - side - divider).max(0);
        let activity = if self.sidebar {
            self.px(36).min(side / 3)
        } else {
            0
        };
        let content_left = if dock_right {
            side_left
        } else {
            side_left + activity
        };
        let content_width = (side - activity).max(0);
        let activity_left = if dock_right {
            side_left + content_width
        } else {
            side_left
        };
        self.layout_sidebar_tabs(Rect::from_size(activity_left, top, activity, bottom - top));
        place(
            self.sidebar_title,
            content_left + self.px(9),
            top,
            content_width
                - self.px(if self.git.open {
                    38
                } else if self.root.is_some() && self.is_tree() {
                    72
                } else {
                    18
                }),
            bar,
        );
        let sidebar_content = Rect::from_size(content_left, top, content_width, bottom - top);
        let list_top = self.layout_git_controls(sidebar_content, bar);
        let changes_bottom = self.layout_git_graph(sidebar_content, list_top);
        self.dock.sidebar_area =
            Rect::from_size(content_left, list_top, content_width, bottom - list_top);
        self.dock.terminal_sidebar = Rect::from_size(side_left, top, side, bottom - top);
        let sidebar_middle = list_top + (bottom - list_top) * self.dock.sidebar_ratio / 100;
        let can_split_sidebar = self.sidebar && !self.git.open && bottom - list_top >= self.px(120);
        self.dock.sidebar = if can_split_sidebar {
            Rect::from_size(
                content_left,
                sidebar_middle,
                content_width,
                bottom - sidebar_middle,
            )
        } else {
            Rect::default()
        };
        let sidebar_split = can_split_sidebar && self.secondary_browser.is_some();
        place(
            self.list,
            content_left,
            list_top,
            content_width,
            if sidebar_split {
                sidebar_middle - list_top
            } else {
                changes_bottom - list_top
            },
        );
        place(
            self.dock.sidebar_splitter,
            content_left,
            sidebar_middle,
            content_width,
            step,
        );
        show(self.dock.sidebar_splitter, sidebar_split);
        show(self.sidebar_title, self.sidebar);
        show(self.list, self.sidebar);
        self.layout_explorer_buttons(Rect::from_size(content_left, top, content_width, bar));
        if self.sidebar {
            show_overlay(
                self.chrome.divider,
                Rect::from_size(edge, top, divider, bottom - top),
            );
        } else {
            show(self.chrome.divider, false);
        }

        let editor_area = self.terminal_layout(Rect::from_size(
            editor_left,
            top,
            editor_width,
            bottom - top,
        ));
        self.dock.area = editor_area;
        let split = self.other.is_some();
        let (first, second, splitter) = self.dock.panes(step);
        place(
            self.dock.splitter,
            splitter.left,
            splitter.top,
            splitter.width(),
            splitter.height(),
        );
        show(self.dock.splitter, split);
        for (group, pane) in [if split { first } else { editor_area }, second]
            .into_iter()
            .enumerate()
        {
            let visible = pane.width() > 0 && pane.height() >= bar && (group == 0 || split);
            let tabs = self.chrome.tab_handle(group);
            let breadcrumb = self.chrome.breadcrumbs[group];
            place(tabs, pane.left, pane.top, pane.width(), bar);
            let breadcrumb_rect =
                Rect::from_size(pane.left, pane.top + bar, pane.width(), self.px(23));
            let breadcrumb_width = self.layout_data_buttons(group, breadcrumb_rect, visible);
            place(
                breadcrumb,
                pane.left,
                pane.top + bar,
                breadcrumb_width,
                self.px(23),
            );
            show(tabs, visible);
            show(breadcrumb, visible && self.group_document(group).is_some());
        }
        self.layout_welcome(self.pane_content(0));
        let gutter_width = PAINT.get().map_or(self.px(52), |paint| paint.gutter_width);
        let inset = |pane: Rect| gutter_width.min(pane.width() / 2).max(0);
        for (group, gutter) in [self.gutter, self.other_gutter].into_iter().enumerate() {
            let pane = self.pane_content(group);
            let has_editor = self
                .group_document(group)
                .is_some_and(|i| self.documents[i].source_visible());
            place(gutter, pane.left, pane.top, inset(pane), pane.height());
            show(gutter, has_editor && inset(pane) > 0 && pane.height() > 0);
        }
        for (index, document) in self.documents.iter().enumerate() {
            let visible =
                Some(index) == self.group_document(document.group) && document.source_visible();
            if visible {
                let pane = self.pane_content(document.group);
                let inset = inset(pane);
                place(
                    document.editor,
                    pane.left + inset,
                    pane.top,
                    pane.width() - inset,
                    pane.height(),
                );
            }
            show(document.editor, visible);
        }
        self.layout_browsers();
        self.layout_data_panels();
        let location_width = self.px(244).min(width / 2);
        let margin = self.px(10);
        place(
            self.status,
            margin,
            bottom + 1,
            width - location_width - margin,
            self.px(25),
        );
        place(
            self.location,
            width - location_width,
            bottom + 1,
            location_width - margin,
            self.px(25),
        );
        show(self.status, true);
        show(self.location, true);
    }

    fn group_document(&self, group: usize) -> Option<usize> {
        if group == usize::from(self.active_right) {
            self.active
        } else {
            self.other
        }
    }

    fn pane_content(&self, group: usize) -> Rect {
        let (first, second, _) = self.dock.panes(self.px(6));
        let pane = if self.other.is_none() {
            self.dock.area
        } else if group == 0 {
            first
        } else {
            second
        };
        let header = self.px(if self.group_document(group).is_some() {
            57
        } else {
            34
        });
        Rect {
            top: (pane.top + header).min(pane.bottom),
            ..pane
        }
    }

    fn layout_welcome(&self, area: Rect) {
        let width = self.px(260).min(area.width().saturating_sub(self.px(24)));
        let left = area.left + (area.width() - width) / 2;
        let top = area.top + ((area.height() - self.px(186)) / 2).max(self.px(12));
        let visible = self.documents.is_empty() && width > 0 && area.height() >= self.px(70);
        place(self.welcome, left, top, width, self.px(66));
        show(self.welcome, visible);
        for (index, action) in self.welcome_actions.into_iter().enumerate() {
            let y = top + self.px(75 + index as i32 * 34);
            place(action, left, y, width, self.px(29));
            show(action, visible && y + self.px(29) <= area.bottom);
        }
    }

    fn layout_input(&self, width: i32, top: i32) {
        let Some(mode) = self.mode else {
            for handle in [
                self.mode_label,
                self.query,
                self.replacement,
                self.action,
                self.second,
                self.third,
                self.dismiss,
                self.editor_tools,
            ] {
                show(handle, false);
            }
            return;
        };
        if mode.floats() {
            self.layout_editor_tools(mode);
            return;
        }
        show(self.editor_tools, false);
        let replaced = mode == Mode::Replace;
        let find = mode == Mode::Find;
        let step = self.px(6);
        let margin = self.px(10);
        let height = self.px(30);
        let label_width = self.px(95);
        let button_width = self.px(70);
        let button_count = if replaced {
            4
        } else if find {
            3
        } else {
            2
        };
        let available =
            (width - 2 * margin - label_width - button_count * (button_width + step) - step)
                .max(70);
        let query_width = if replaced {
            (available - step) / 2
        } else {
            available
        };
        let mut x = margin;
        place(self.mode_label, x, top, label_width, height);
        x += label_width + step;
        place(self.query, x, top, query_width, height);
        x += query_width + step;
        if replaced {
            place(self.replacement, x, top, query_width, height);
            x += query_width + step;
        }
        place(self.action, x, top, button_width, height);
        x += button_width + step;
        if find || replaced {
            place(self.second, x, top, button_width, height);
            x += button_width + step;
        }
        if replaced {
            place(self.third, x, top, button_width, height);
            x += button_width + step;
        }
        place(self.dismiss, x, top, button_width, height);
        for handle in [self.mode_label, self.query, self.action, self.dismiss] {
            show(handle, true);
        }
        show(self.replacement, replaced);
        show(self.second, find || replaced);
        show(self.third, replaced);
    }

    fn layout_editor_tools(&self, mode: Mode) {
        let anchor = self
            .editor()
            .map(|editor| relative_rect(editor, self.window))
            .unwrap_or_else(|| {
                let mut tabs = relative_rect(self.chrome.tabs, self.window);
                tabs.top = tabs.bottom;
                tabs
            });
        let margin = self.px(9);
        let gap = self.px(4);
        let row = self.px(23);
        let inset = self.px(5);
        let width = self.px(384).min((anchor.width() - margin * 2).max(0));
        let left = anchor.right - margin - width;
        let top = anchor.top + margin;
        let replace = mode == Mode::Replace;
        let lines = if replace { 2 } else { 1 };
        show_overlay(
            self.editor_tools,
            Rect::from_size(
                left,
                top,
                width,
                inset * 2 + row * lines + gap * (lines - 1),
            ),
        );
        let left = left + inset;
        let top = top + inset;
        let right = left + width - 2 * inset;
        let compact = self.px(23);
        let close = right - compact;
        show_overlay(self.dismiss, Rect::from_size(close, top, compact, row));
        let show_at =
            |handle, x, y, w: i32| show_overlay(handle, Rect::from_size(x, y, w.max(0), row));
        if mode == Mode::Rename {
            let label = self.px(67).min(width / 4);
            let change = self.px(43);
            let button = close - gap - change;
            show_at(self.mode_label, left, top, label);
            show_at(self.query, left + label, top, button - left - label - gap);
            show_at(self.action, button, top, change);
            show(self.second, false);
            show(self.third, false);
            show(self.replacement, false);
        } else {
            show(self.mode_label, false);
            let next = close - gap - compact;
            show_at(self.action, next, top, compact);
            if replace {
                show_at(self.query, left, top, next - left - gap);
                let y = top + row + gap;
                let button = self.px(49);
                let all = right - button;
                let one = all - gap - button;
                show_at(self.replacement, left, y, one - left - gap);
                show_at(self.second, one, y, button);
                show_at(self.third, all, y, button);
            } else {
                let previous = next - gap - compact;
                show_at(self.query, left, top, previous - left - gap);
                show_at(self.second, previous, top, compact);
                show(self.third, false);
                show(self.replacement, false);
            }
        }
    }

    fn refresh_locale_labels(&mut self) {
        for ((handle, width), &(_, key, minimum)) in self.toolbar.iter_mut().zip(BUTTONS) {
            let text = i18n::text(key);
            title(*handle, text);
            *width = minimum.max(
                13 + text
                    .chars()
                    .map(|ch| if ch.is_ascii() { 7 } else { 13 })
                    .sum::<i32>(),
            );
        }
        for (handle, key) in self.welcome_actions.into_iter().zip([
            "ui.open.file.ctrl.o",
            "ui.open.folder.ctrl.k.ctrl.o",
            "ui.new.file.ctrl.n",
        ]) {
            title(handle, i18n::text(key));
        }
        for (handle, key) in [
            (self.dismiss, "ui.close"),
            (self.explorer.file, "ui.new.file"),
            (self.explorer.folder, "ui.new.folder"),
            (self.git.explorer, "ui.explorer"),
            (self.git.changes, "ui.source.control"),
        ] {
            title(handle, i18n::text(key));
        }
        self.refresh_git_locale();
        for buttons in self.data_buttons {
            for (button, key) in buttons.into_iter().zip([
                "ui.source",
                "data.pretty",
                "ui.tree",
                "ui.open.in.browser",
            ]) {
                title(button, i18n::text(key));
            }
        }
        for doc in &mut self.documents {
            if doc
                .browser
                .as_ref()
                .is_some_and(|view| view.folder().is_none())
            {
                doc.label = i18n::text("ui.new.tab").to_owned();
            }
        }
        if self.documents.is_empty() {
            self.report(i18n::text("ui.open.a.file.or.choose.a.workspace"));
        } else {
            self.report("");
        }
        if self.git.open {
            self.show_git_items();
        } else if self.mode.is_none() && !self.showing_issues {
            self.show_files("");
        }
        self.update_status();
    }

    fn apply_preferences(&mut self) {
        self.terminal.font(self.dpi);
        let paint = Paint::new(&self.settings, self.dpi, self.window);
        title_theme(
            self.window,
            self.settings.dark(),
            paint.chrome_rgb,
            paint.foreground,
        );
        send(self.list, LB_SETITEMHEIGHT, 0, self.px(22) as isize);
        let previous = PAINT.replace(Some(paint));
        for control in self
            .git
            .controls()
            .into_iter()
            .chain(self.data_buttons.into_iter().flatten())
        {
            set_font(control, paint.font);
            repaint(control);
        }
        self.git.graph.fonts(paint, self.px(23));
        for control in self.welcome_actions {
            set_font(control, paint.font);
        }
        for (control, _) in &self.toolbar {
            set_font(*control, paint.font);
        }
        for control in [
            self.sidebar_title,
            self.explorer.input,
            self.list,
            self.status,
            self.location,
            self.mode_label,
            self.query,
            self.replacement,
            self.action,
            self.second,
            self.third,
            self.dismiss,
        ] {
            set_font(control, paint.font);
            redraw(control);
        }
        if let Some(old) = previous {
            old.release();
        }
        for index in 0..self.documents.len() {
            if let Some(data) = &self.documents[index].data_view {
                data.apply_style(&self.settings);
            }
            if let Some(view) = &self.documents[index].browser {
                view.apply_font();
            } else {
                self.format(index, true);
            }
        }
        if let Some(view) = &self.secondary_browser {
            view.apply_font();
        }
        self.layout();
        redraw(self.window);
    }

    fn report(&self, message: &str) {
        title(self.status, message);
    }
    fn show_error(&self, error: io::Error) {
        report_error(self.window, &error.to_string());
    }
    fn editor(&self) -> Option<Handle> {
        self.active.and_then(|index| {
            let doc = &self.documents[index];
            doc.source_visible().then_some(doc.editor)
        })
    }
    fn focus_editor(&self) {
        let target = self
            .active
            .and_then(|index| {
                self.documents[index]
                    .browser
                    .as_ref()
                    .map(|view| view.query)
            })
            .or_else(|| {
                self.active.and_then(|index| {
                    let doc = &self.documents[index];
                    let mode = doc.data_mode();
                    if mode == data_view::ViewMode::Raw {
                        return None;
                    }
                    doc.data_view
                        .as_ref()
                        .and_then(data_view::View::focus)
                        .or(Some(self.data_buttons[doc.group][mode as usize]))
                })
            })
            .or_else(|| self.editor())
            .unwrap_or(self.welcome_actions[0]);
        unsafe {
            SetFocus(target);
        }
    }

    fn blank_name(&mut self) -> String {
        self.untitled += 1;
        crate::i18n::message("ui.untitled", &[(self.untitled).to_string()])
    }

    fn new_document(&mut self) -> io::Result<()> {
        let label = self.blank_name();
        self.add_document(None, label, "", Encoding::default())
    }

    fn add_document(
        &mut self,
        path: Option<PathBuf>,
        label: String,
        code: &str,
        encoding: Encoding,
    ) -> io::Result<()> {
        let editor = child(
            self.window,
            EDITOR,
            "RICHEDIT50W",
            "",
            WS_TABSTOP
                | WS_VSCROLL
                | WS_HSCROLL
                | ES_MULTILINE
                | ES_NOHIDESEL
                | ES_AUTOHSCROLL
                | ES_AUTOVSCROLL
                | ES_WANTRETURN
                | ES_DISABLENOSCROLL,
        )?;
        chrome::watch_control(editor, false);
        send(editor, EM_EXLIMITTEXT, 0, 64 * 1024 * 1024);
        send(editor, EM_SETUNDOLIMIT, 128, 0);
        set_editor_text(editor, code);
        send(editor, EM_EMPTYUNDOBUFFER, 0, 0);
        send(editor, EM_SETMODIFY, 0, 0);
        send(
            editor,
            EM_SETEVENTMASK,
            0,
            ENM_CHANGE | ENM_SCROLL | ENM_SELCHANGE,
        );
        let language = path
            .as_deref()
            .map(Language::from_path)
            .unwrap_or(Language::Plain);
        let index = self.documents.len();
        self.documents.push(Document {
            editor,
            group: usize::from(self.active_right),
            revision: 0,
            path,
            label,
            encoding,
            language,
            dirty: false,
            highlighting: None,
            highlight_pending: false,
            colored_view: None,
            git_diff: None,
            archive_member: None,
            browser: None,
            response: false,
            response_url: None,
            data_view: None,
        });
        self.format(index, true);
        self.activate(index, true);
        Ok(())
    }

    fn open_path(&mut self, path: &Path) -> io::Result<()> {
        let path = std::path::absolute(path)?;
        if archive::supported(&path) {
            return self.archive_extract(path);
        }
        if let Some(index) = self.documents.iter().position(|document| {
            document
                .path
                .as_ref()
                .is_some_and(|open| same_path(open, &path))
        }) {
            self.activate(index, true);
            return Ok(());
        }
        let (code, encoding) = text::read(&path)?;
        if code.len() > 64 * 1024 * 1024 {
            return Err(io::Error::other(crate::i18n::text(
                "ui.maker.opens.text.files.up.to.64",
            )));
        }
        let label = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        self.add_document(Some(path), label, &code, encoding)
    }

    fn open_paths(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            let result = if path.is_dir() {
                self.load_folder(path)
            } else {
                self.open_path(&path)
            };
            if let Err(error) = result {
                self.show_error(error);
            }
        }
    }

    fn activate(&mut self, index: usize, focus: bool) {
        let previous = self.active;
        if previous.is_none() {
            self.report("");
        }
        if self.active != Some(index) {
            self.dismiss_completion();
        }
        if self.showing_issues && self.active != Some(index) {
            self.show_files("");
        }
        if self.other.is_some() && self.documents[index].group != usize::from(self.active_right) {
            self.other = self.active;
            self.active_right = !self.active_right;
        }
        self.active = Some(index);
        self.layout();
        if self.documents[index].highlight_pending {
            self.queue_highlight();
        }
        self.tab_reveal(index);
        self.animate_editor_tabs(previous, Some(index));
        self.update_status();
        if focus {
            self.focus_editor();
        }
        self.queue_session_save();
    }

    fn refresh_tab(&self) {
        for group in 0..2 {
            repaint(self.chrome.tab_handle(group));
            repaint(self.chrome.breadcrumbs[group]);
        }
    }

    fn update_status(&self) {
        let label = if let Some(document) = self.active.map(|index| &self.documents[index]) {
            if let Some(view) = &document.browser {
                title(self.location, &view.base().display().to_string());
            } else if document.data_mode() != data_view::ViewMode::Raw {
                let mode = if document.data_mode() == data_view::ViewMode::Pretty {
                    i18n::text("data.pretty")
                } else {
                    crate::i18n::text("ui.tree")
                };
                title(
                    self.location,
                    &format!("{} · {mode}", document.language.label()),
                );
            } else {
                self.update_caret_status();
            }
            format!(
                "{}{} - Maker",
                if document.dirty { "● " } else { "" },
                document.label
            )
        } else {
            title(self.location, "");
            "Maker".to_owned()
        };
        let mut current = self.window_title.borrow_mut();
        if *current != label {
            title(self.window, &label);
            *current = label;
        }
    }

    fn update_caret_status(&self) {
        let Some(editor) = self.editor() else {
            return;
        };
        let document = &self.documents[self.active.unwrap()];
        let (line, column) = caret_position(editor, self.settings.wrap);
        let current = Some((editor, line + 1));
        if self.gutter_caret.replace(current) != current {
            repaint(self.gutter);
            if self.other.is_some() {
                repaint(self.other_gutter);
            }
        }
        let newline = if document.encoding.crlf { "CRLF" } else { "LF" };
        title(
            self.location,
            &format!(
                "{}:{column}   ·   {}   ·   {newline}",
                line + 1,
                document.language.label()
            ),
        );
    }

    fn format(&mut self, index: usize, full: bool) {
        let document = &mut self.documents[index];
        let editor = document.editor;
        if full {
            let padding = (7 * self.dpi / 96) as isize;
            send(editor, EM_SETMARGINS, 3, padding | padding << 16);
            send(
                editor,
                EM_SETBKGNDCOLOR,
                0,
                PAINT.get().expect("paint initialized").editor_rgb as isize,
            );
            send(
                editor,
                EM_SETTARGETDEVICE,
                0,
                isize::from(!self.settings.wrap),
            );
        }
        if !full && document.language == Language::Plain {
            return;
        }
        let size = editor_size(editor);
        let viewport = size > HIGHLIGHT_LIMIT && document.language != Language::Plain;
        let region = if viewport {
            let area = bounds(editor);
            let start = editor_character_at(editor, Point { x: 0, y: 0 });
            let end = editor_character_at(
                editor,
                Point {
                    x: if self.settings.wrap {
                        0
                    } else {
                        area.right.saturating_sub(12)
                    },
                    y: area.bottom.saturating_sub(1),
                },
            )
            .max(start.saturating_add(1));
            let limit = i32::try_from(size).unwrap_or(i32::MAX);
            if !full
                && !document.highlight_pending
                && document.colored_view.is_some_and(|range| {
                    range.start <= (start - HIGHLIGHT_MARGIN).max(0)
                        && range.end >= end.saturating_add(HIGHLIGHT_MARGIN).min(limit)
                })
            {
                return;
            }
            let line = send(editor, 0x00c9, start as usize, 0).max(0) as usize;
            let end_line = send(editor, 0x00c9, end as usize, 0).max(0) as usize;
            let context_start = send(editor, 0x00bb, line.saturating_sub(60), 0).max(0) as i32;
            let context_end = send(editor, 0x00bb, end_line + 65, 0);
            let from = context_start
                .max(start.saturating_sub(HIGHLIGHT_WINDOW / 3))
                .max(0);
            let to = if context_end < 0 {
                limit
            } else {
                context_end as i32
            };
            Some(Selection {
                start: from,
                end: to.min(from.saturating_add(HIGHLIGHT_WINDOW)).min(limit),
            })
        } else {
            None
        };
        let current_view = document.colored_view;
        if !full && !document.highlight_pending && region == current_view {
            return;
        }
        // A viewport move by itself may leave styled text elsewhere in the editor.
        let same_start = region.map(|range| range.start) == current_view.map(|range| range.start);
        let previous = document
            .highlighting
            .as_ref()
            .filter(|_| !full && same_start && document.highlight_pending);
        let next = (document.language != Language::Plain).then(|| {
            let code = region.map_or_else(
                || editor_text(editor),
                |range| editor_range(editor, range.start, range.end),
            );
            highlight::Highlighting::new(code, document.language)
        });
        let reset = full || previous.is_none();
        let offset = region.map_or(0, |range| range.start);
        let mut runs = next
            .as_ref()
            .into_iter()
            .flat_map(|next| next.runs(previous))
            .filter(|(_, color)| !reset || color.is_some())
            .peekable();
        if !reset && runs.peek().is_none() {
            drop(runs);
            document.highlighting = next;
            document.colored_view = region;
            document.highlight_pending = false;
            return;
        }
        let Some(hold) = pause_formatting(editor) else {
            return;
        };
        let saved_selection = selection(editor);
        let modified = send(editor, EM_GETMODIFY, 0, 0);
        let mut scroll = Point::default();
        send(editor, EM_GETSCROLLPOS, 0, &mut scroll as *mut _ as isize);
        let visible = unsafe { IsWindowVisible(editor) } != 0;
        if visible {
            painting(editor, false);
        }
        let palette = PAINT.get().expect("paint has been initialized");
        let base = palette.foreground;
        let background = (document.language == Language::Diff).then_some(palette.editor_rgb);
        let range_color = hold.range_color();
        let set_color = |start, end, color, background| {
            if !range_color
                .as_ref()
                .is_some_and(|target| target.set(start, end, color, background))
            {
                select(editor, start, end);
                char_format(editor, 1, color, background, None);
            }
        };
        if full || reset && region.is_none() {
            let font = full.then_some((self.settings.font.as_str(), self.settings.points));
            char_format(editor, 4, base, background, font);
            char_format(editor, 0, base, background, font);
        } else if reset && let Some(range) = region {
            set_color(range.start, range.end, base, background);
        }
        for (range, color) in runs {
            let run_background = match color {
                Some(Color::DiffAdded) => Some(palette.diff_backgrounds[0]),
                Some(Color::DiffRemoved) => Some(palette.diff_backgrounds[1]),
                _ => background,
            };
            set_color(
                offset + range.start,
                offset + range.end,
                color.map_or(base, |color| palette.highlight(color)),
                run_background,
            );
        }
        drop(range_color);
        select(editor, saved_selection.start, saved_selection.end);
        send(editor, EM_SETSCROLLPOS, 0, &scroll as *const _ as isize);
        send(editor, EM_SETMODIFY, usize::from(modified != 0), 0);
        drop(hold);
        if visible {
            painting(editor, true);
            repaint(editor);
        }
        document.highlighting = next;
        document.colored_view = region;
        document.highlight_pending = false;
    }

    fn repaint_editor(&mut self) {
        unsafe {
            KillTimer(self.window, COLOR_TIMER);
        }
        let indices = [self.active, self.other];
        for index in indices.into_iter().flatten() {
            self.format(index, false);
        }
    }

    fn schedule_viewport_highlight(&self, editor: Handle) {
        if [self.active, self.other]
            .into_iter()
            .flatten()
            .any(|index| {
                let document = &self.documents[index];
                document.editor == editor && document.colored_view.is_some()
            })
        {
            unsafe {
                SetTimer(self.window, VIEWPORT_TIMER, 75, 0);
            }
        }
    }

    fn repaint_viewport(&mut self) {
        unsafe {
            KillTimer(self.window, VIEWPORT_TIMER);
        }
        for index in [self.active, self.other].into_iter().flatten() {
            if self.documents[index].colored_view.is_some() {
                self.format(index, false);
            }
        }
    }

    fn save_document(&mut self, index: usize, save_as: bool) -> io::Result<bool> {
        let document = &self.documents[index];
        if document.is_preview() || document.browser.is_some() {
            self.report(crate::i18n::text("ui.preview.tabs.are.read.only"));
            return Ok(false);
        }
        let path = if let Some(path) = document.path.as_ref().filter(|_| !save_as) {
            path.clone()
        } else {
            let suggestion = document
                .path
                .as_ref()
                .and_then(|path| path.file_name())
                .unwrap_or(OsStr::new("untitled.txt"));
            let Some(path) = file_dialog(self.window, true, suggestion)?
                .into_iter()
                .next()
            else {
                return Ok(false);
            };
            path
        };
        if self.documents.iter().enumerate().any(|(i, other)| {
            i != index
                && other
                    .path
                    .as_ref()
                    .is_some_and(|open| same_path(open, &path))
        }) {
            return Err(io::Error::other(crate::i18n::text(
                "ui.this.file.is.already.open.in.another",
            )));
        }
        let editor = document.editor;
        let dirty = send(editor, EM_GETMODIFY, 0, 0) != 0;
        let changed_path = document
            .path
            .as_ref()
            .is_none_or(|open| !same_path(open, &path));
        if dirty || save_as || changed_path {
            text::save(&path, &editor_text(editor), document.encoding)?;
        }
        let document = &mut self.documents[index];
        document.label = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        if document.language != Language::from_path(&path)
            || !data_view::supports(document.language, Some(&path))
        {
            document.data_view = None;
        }
        document.language = Language::from_path(&path);
        document.path = Some(path);
        document.dirty = false;
        send(editor, EM_SETMODIFY, 0, 0);
        if changed_path {
            self.format(index, true);
            self.layout();
        }
        self.refresh_tab();
        self.update_status();
        self.report(crate::i18n::text("ui.saved"));
        self.request_git_refresh();
        Ok(true)
    }

    fn confirm_document(&mut self, index: usize) -> bool {
        let document = &self.documents[index];
        if send(document.editor, EM_GETMODIFY, 0, 0) == 0 {
            return true;
        }
        match message(
            self.window,
            &crate::i18n::message("ui.save.changes.to", std::slice::from_ref(&document.label)),
            "Maker",
            MB_YESNOCANCEL | MB_ICONWARNING,
        ) {
            6 => match self.save_document(index, false) {
                Ok(saved) => saved,
                Err(error) => {
                    self.show_error(error);
                    false
                }
            },
            7 => true,
            _ => false,
        }
    }

    fn confirm_exit(&mut self) -> bool {
        (0..self.documents.len()).all(|index| self.confirm_document(index))
    }

    fn close_tab(&mut self) {
        if let Some(index) = self.active {
            self.close_document(index);
        }
    }

    fn close_document(&mut self, index: usize) {
        if !self.confirm_document(index) {
            return;
        }
        self.remove_document(index, true);
    }

    fn remove_document(&mut self, index: usize, remember: bool) {
        self.clear_tab_motion();
        let document = self.documents.remove(index);
        self.dock.forget_window_move(document.editor);
        if remember && let Some(path) = document.path {
            self.recently_closed.push(path);
        }
        unsafe {
            DestroyWindow(document.editor);
        }
        let adjust = |old: usize| if old > index { old - 1 } else { old };
        let replacement = self
            .documents
            .iter()
            .rposition(|doc| doc.group == document.group);
        let had_split = self.other.is_some();
        self.active = self.active.and_then(|old| {
            if old == index {
                replacement
            } else {
                Some(adjust(old))
            }
        });
        self.other = self.other.and_then(|old| {
            if old == index {
                replacement
            } else {
                Some(adjust(old))
            }
        });
        if had_split && (self.active.is_none() || self.other.is_none()) {
            self.active = self.active.or(self.other);
            self.collapse_editor_groups();
        }
        if self.documents.is_empty() {
            self.layout();
            self.update_status();
            self.focus_editor();
            self.queue_session_save();
        } else {
            self.activate(
                self.active
                    .unwrap_or_else(|| index.min(self.documents.len() - 1)),
                true,
            );
        }
    }

    fn load_folder(&mut self, path: PathBuf) -> io::Result<()> {
        let path = std::path::absolute(path)?;
        let files = text::files(&path)?;
        if self.root.as_ref() != Some(&path) {
            self.expanded.clear();
            self.explorer_selection = None;
            self.explorer.selected.clear();
            self.explorer.archive.reset();
            self.explorer_top = 0;
        }
        self.root = Some(path);
        self.files = files;
        self.files_stale = false;
        self.sidebar = true;
        self.clear_mode();
        self.request_git_refresh();
        Ok(())
    }

    fn show_files(&mut self, query: &str) {
        self.showing_issues = false;
        if self.git.open {
            self.show_git_items();
            return;
        }
        if self.mode == Some(Mode::Quick) {
            let search = query.to_lowercase();
            let items = self
                .files
                .iter()
                .filter(|path| {
                    relative(path, self.root.as_deref())
                        .to_lowercase()
                        .contains(&search)
                })
                .cloned()
                .map(|path| ListItem::File(path, None))
                .collect();
            title(self.sidebar_title, crate::i18n::text("ui.quick.open"));
            self.set_list(items);
            return;
        }
        let label = self.root.as_deref().map_or_else(
            || crate::i18n::text("ui.explorer").into(),
            |root| {
                root.file_name()
                    .unwrap_or(root.as_os_str())
                    .to_string_lossy()
            },
        );
        title(self.sidebar_title, &label);
        let mut items = Vec::new();
        if let Some(root) = self.root.as_deref() {
            self.tree_rows(root, 0, &mut items);
        }
        let selected = self.explorer_selection.as_deref().and_then(|selection| {
            items
                .iter()
                .position(|item| explorer::row_path(item) == Some(selection))
        });
        let choices: Vec<_> = if self.explorer.selected.is_empty() {
            Vec::new()
        } else {
            let paths: HashSet<&Path> = self
                .explorer
                .selected
                .iter()
                .map(PathBuf::as_path)
                .collect();
            items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    paths.contains(explorer::row_path(item)?).then_some(index)
                })
                .collect()
        };
        self.set_list(items);
        if let Some(row) = selected.or_else(|| choices.first().copied()) {
            list_select(self.list, row);
            for item in choices {
                list_mark(self.list, item);
            }
        }
        if !self.list_items.is_empty() {
            send(
                self.list,
                LB_SETTOPINDEX,
                self.explorer_top.min(self.list_items.len() - 1),
                0,
            );
        }
        self.sync_scrollbars();
    }

    fn tree_rows(&self, folder: &Path, depth: usize, rows: &mut Vec<ListItem>) {
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        let mut entries: Vec<_> = entries
            .flatten()
            .filter_map(|entry| {
                let kind = entry.file_type().ok()?;
                let link = kind.is_symlink();
                let path = entry.path();
                let directory = kind.is_dir() || (link && path.is_dir());
                if !directory && !kind.is_file() && !link {
                    return None;
                }
                Some((
                    path,
                    directory,
                    entry.file_name().to_string_lossy().to_lowercase(),
                ))
            })
            .collect();
        entries.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));
        for (path, directory, _) in entries {
            if directory {
                let open = depth < 128 && self.expanded.contains(&path);
                rows.push(ListItem::Folder {
                    path: path.clone(),
                    depth,
                    open,
                });
                if open {
                    self.tree_rows(&path, depth + 1, rows);
                }
            } else if archive::supported(&path) {
                let open = self.explorer.archive.opened(&path);
                rows.push(ListItem::Archive {
                    path: path.clone(),
                    depth,
                    open,
                });
                if open {
                    self.explorer
                        .archive
                        .rows(&path, Path::new(""), depth + 1, rows);
                }
            } else {
                rows.push(ListItem::File(path, Some(depth)));
            }
        }
    }

    fn set_list(&mut self, items: Vec<ListItem>) {
        self.cancel_explorer_edit(false);
        chrome::reset_file_motion(self.list);
        painting(self.list, false);
        send(self.list, LB_RESETCONTENT, 0, 0);
        for item in &items {
            let (label, depth, icon) = match item {
                ListItem::Draft => unreachable!("draft rows are inserted into the current tree"),
                ListItem::File(path, depth) => {
                    let name = if depth.is_some() {
                        file_name(path)
                    } else {
                        relative(path, self.root.as_deref())
                    };
                    (
                        name.into_owned(),
                        depth.unwrap_or(0),
                        chrome::file_icon(Some(path)),
                    )
                }
                ListItem::Folder { path, depth, open } => (
                    file_name(path).into_owned(),
                    *depth,
                    chrome::folder_icon(path, *open),
                ),
                ListItem::Archive { path, depth, open } => (
                    file_name(path).into_owned(),
                    *depth,
                    50 + usize::from(*open),
                ),
                ListItem::ArchiveMember {
                    path,
                    depth,
                    directory,
                    open,
                    ..
                } => (
                    file_name(path).into_owned(),
                    *depth,
                    if *directory {
                        chrome::folder_icon(path, *open)
                    } else {
                        chrome::file_icon(Some(path))
                    },
                ),
                ListItem::Match {
                    path,
                    line,
                    excerpt,
                    ..
                } => (
                    format!(
                        "{}:{}  {excerpt}",
                        relative(path, self.root.as_deref()),
                        line + 1
                    ),
                    0,
                    chrome::file_icon(Some(path)),
                ),
                ListItem::Issue { line, kind, .. } => {
                    let key = match kind {
                        text::BracketIssue::Unclosed => "ui.unclosed.bracket",
                        text::BracketIssue::UnmatchedClosing => "ui.unmatched.closing.bracket",
                    };
                    (
                        i18n::message(
                            "ui.line",
                            &[(line + 1).to_string(), i18n::text(key).to_owned()],
                        ),
                        0,
                        13,
                    )
                }
                ListItem::Group(label) => (label.clone(), 0, 14),
                ListItem::Git(change) => {
                    let marker = if change.kind == b'?' {
                        'U'
                    } else {
                        char::from(change.kind)
                    };
                    (
                        format!("{marker}  {}", change.path.display()),
                        0,
                        chrome::file_icon(Some(&change.path)),
                    )
                }
            };
            list_entry(self.list, &label, (depth << 8) | icon);
        }
        if !items.is_empty() {
            if self.is_tree() {
                list_set_caret(self.list, 0);
            } else {
                list_select(self.list, 0);
            }
        }
        self.list_items = items;
        painting(self.list, true);
        redraw(self.list);
        self.sync_scrollbars();
    }

    fn is_tree(&self) -> bool {
        !self.git.open
            && !self.showing_issues
            && !matches!(self.mode, Some(Mode::Quick | Mode::Workspace))
    }

    fn remember_tree_focus(&mut self, row: usize, path: PathBuf) {
        self.explorer_selection = Some(path);
        self.explorer_top = (send(self.list, LB_GETTOPINDEX, 0, 0).max(0) as usize).min(row);
        self.queue_session_save();
    }

    fn toggle_folder(&mut self, row: usize, path: PathBuf) -> io::Result<()> {
        self.remember_tree_focus(row, path.clone());
        let open = !self.expanded.remove(&path);
        if open {
            fs::read_dir(&path)?;
            self.expanded.insert(path);
        }
        self.show_files("");
        chrome::animate_folder(self.list, row, open);
        Ok(())
    }

    fn tree_key(&mut self, key: i32) -> bool {
        if !self.is_tree() || !matches!(key, 0x25 | 0x27) {
            return false;
        }
        let row = list_caret(self.list).unwrap_or(usize::MAX);
        match self.list_items.get(row) {
            Some(ListItem::Archive { path, open, .. }) if *open == (key == 0x25) => {
                if let Err(error) = self.archive_toggle(path.clone()) { self.show_error(error); }
            }
            Some(ListItem::Archive { open: true, .. } | ListItem::ArchiveMember { directory: true, open: true, .. })
                if key == 0x27 => {
                if matches!((self.list_items.get(row), self.list_items.get(row + 1)),
                    (Some(ListItem::Archive { depth, .. } | ListItem::ArchiveMember { depth, .. }),
                    Some(ListItem::ArchiveMember { depth: child, .. })) if child > depth)
                {
                    chrome::select_file_row(self.list, row + 1);
                }
            }
            Some(ListItem::ArchiveMember { archive, path, directory: true, open, .. })
                if *open == (key == 0x25) => {
                if let Err(error) = self.archive_member(archive.clone(), path.clone(), true) { self.show_error(error); }
            }
            Some(ListItem::ArchiveMember { archive, path, .. }) if key == 0x25 => {
                if let Some(index) = self.list_items[..row].iter().rposition(|item| match item {
                    ListItem::ArchiveMember { archive: source, path: parent, .. } =>
                        source == archive && Some(parent.as_path()) == path.parent(),
                    ListItem::Archive { path: source, .. } =>
                        source == archive && path.parent() == Some(Path::new("")),
                    _ => false,
                }) {
                    chrome::select_file_row(self.list, index);
                }
            }
            Some(ListItem::Folder { path, open, .. }) if *open == (key == 0x25) => {
                if let Err(error) = self.toggle_folder(row, path.clone()) { self.show_error(error); }
            }
            Some(ListItem::Folder { open: true, depth, .. }) if key == 0x27 => {
                let descendant = self.list_items.get(row + 1);
                if matches!(descendant, Some(ListItem::File(_, Some(next)) | ListItem::Folder { depth: next, .. }) if next > depth) {
                    chrome::select_file_row(self.list, row + 1);
                }
            }
            Some(ListItem::Folder { path, .. } | ListItem::Archive { path, .. } | ListItem::File(path, Some(_))) if key == 0x25 => {
                if let Some(index) = self.list_items[..row].iter().rposition(|item| {
                    matches!(item, ListItem::Folder { path: folder, .. } if Some(folder.as_path()) == path.parent())
                }) {
                    chrome::select_file_row(self.list, index);
                }
            }
            _ => {}
        }
        self.sync_scrollbars();
        self.track_explorer_selection();
        true
    }

    fn open_selected(&mut self) -> io::Result<()> {
        let row = if self.is_tree() {
            let selected = list_selection(self.list);
            let Some(&row) = selected.first() else {
                return Ok(());
            };
            if selected.len() > 1 {
                let files = selected
                    .into_iter()
                    .filter_map(|index| match self.list_items.get(index) {
                        Some(ListItem::File(path, _)) => Some(path.clone()),
                        _ => None,
                    })
                    .collect();
                self.open_paths(files);
                return Ok(());
            }
            row
        } else {
            list_caret(self.list).unwrap_or(usize::MAX)
        };
        let Some(item) = self.list_items.get(row) else {
            return Ok(());
        };
        match item {
            ListItem::Git(change) => self.preview_git(change.clone())?,
            ListItem::Group(_) | ListItem::Draft => {}
            ListItem::Folder { path, .. } => self.toggle_folder(row, path.clone())?,
            ListItem::Archive { path, .. } => self.archive_extract(path.clone())?,
            ListItem::ArchiveMember {
                archive,
                path,
                directory,
                ..
            } => {
                self.archive_member(archive.clone(), path.clone(), *directory)?;
            }
            ListItem::File(path, _) if file_actions::executable(path) => {
                self.report(crate::i18n::text(
                    "ui.double.click.the.executable.to.run.it",
                ));
            }
            ListItem::File(path, depth) => {
                let path = path.clone();
                if depth.is_some() {
                    self.remember_tree_focus(row, path.clone());
                }
                if self.mode == Some(Mode::Quick) {
                    self.clear_mode();
                }
                self.open_path(&path)?;
            }
            ListItem::Match {
                path, line, column, ..
            } => {
                let (path, line, column) = (path.clone(), *line, *column);
                self.open_path(&path)?;
                self.jump_line(line, column);
            }
            ListItem::Issue { index, .. } => {
                let index = *index;
                if let Some(editor) = self.editor() {
                    let text = editor_text(editor);
                    if let Some(prefix) = text.get(..index) {
                        let position = text::wide_offset(prefix, prefix.len());
                        self.reveal(position, position);
                    }
                }
            }
        }
        Ok(())
    }

    fn refresh_file_index(&mut self) {
        if self.files_stale
            && let Some(root) = &self.root
            && let Ok(files) = text::files(root)
        {
            self.files = files;
            self.files_stale = false;
        }
    }

    fn start_search(&mut self) {
        let query = caption(self.query);
        if query.is_empty() {
            return;
        }
        if self.root.is_none() {
            self.report(crate::i18n::text("ui.open.a.workspace.folder.first"));
            return;
        }
        self.refresh_file_index();
        let paths = self.files.clone();
        let dirty = self
            .documents
            .iter()
            .filter(|doc| doc.dirty)
            .filter_map(|doc| doc.path.clone().map(|path| (path, editor_text(doc.editor))))
            .collect::<Vec<_>>();
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        self.search = Some(Search { receiver, cancel });
        self.showing_issues = false;
        title(self.sidebar_title, crate::i18n::text("ui.search.workspace"));
        self.set_list(Vec::new());
        self.report(crate::i18n::text("ui.searching"));
        thread::spawn(move || {
            let mut hits = Vec::new();
            for path in paths {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                let code = if let Some((_, code)) =
                    dirty.iter().find(|(file, _)| same_path(file, &path))
                {
                    std::borrow::Cow::Borrowed(code.as_str())
                } else {
                    if !fs::metadata(&path).is_ok_and(|data| data.len() <= 3 * 1024 * 1024) {
                        continue;
                    }
                    let Ok((code, _)) = text::read(&path) else {
                        continue;
                    };
                    std::borrow::Cow::Owned(code)
                };
                for (line, value) in code.lines().enumerate() {
                    if let Some(column) = value.find(&query) {
                        hits.push(ListItem::Match {
                            path: path.clone(),
                            line,
                            column,
                            excerpt: value.trim().chars().take(96).collect(),
                        });
                        if hits.len() == 160 {
                            break;
                        }
                    }
                }
                if hits.len() == 160 {
                    break;
                }
            }
            let _ = sender.send(hits);
        });
        unsafe {
            SetTimer(self.window, SEARCH_TIMER, 120, 0);
        }
    }

    fn check_search(&mut self) {
        let Some(search) = self.search.as_ref() else {
            unsafe {
                KillTimer(self.window, SEARCH_TIMER);
            }
            return;
        };
        match search.receiver.try_recv() {
            Ok(items) => {
                self.report(&crate::i18n::message(
                    "ui.search.results",
                    &[(items.len()).to_string()],
                ));
                self.set_list(items);
            }
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.report(crate::i18n::text("ui.could.not.complete.the.search"))
            }
        }
        self.search = None;
        unsafe {
            KillTimer(self.window, SEARCH_TIMER);
        }
    }

    fn reveal(&self, start: i32, end: i32) {
        if let Some(editor) = self.editor() {
            select(editor, start, end);
            send(editor, EM_SCROLLCARET, 0, 0);
            self.focus_editor();
            self.update_status();
        }
    }

    fn jump_line(&self, target: usize, column: usize) {
        let Some(editor) = self.editor() else {
            return;
        };
        let text = editor_text(editor);
        let mut index = 0;
        for (line, current) in text.split_inclusive('\n').enumerate() {
            if line == target {
                index += current.get(..column).map_or(0, str::len);
                break;
            }
            index += current.len();
        }
        let position = text::wide_offset(&text, index);
        self.reveal(position, position);
    }

    fn show_mode(&mut self, mode: Mode) {
        let was_floating = self.mode.is_some_and(Mode::floats);
        let reflow = !mode.floats() || self.mode.is_some_and(|old| !old.floats());
        self.search = None;
        self.cancel_explorer_edit(false);
        self.mode = Some(mode);
        if matches!(mode, Mode::Quick | Mode::Workspace) {
            self.git.open = false;
        }
        let (label, button) = match mode {
            Mode::Find => (crate::i18n::text("ui.find"), "↓"),
            Mode::Replace => (crate::i18n::text("ui.find.and.replace"), "↓"),
            Mode::Workspace => (
                crate::i18n::text("ui.search.folder"),
                crate::i18n::text("ui.search"),
            ),
            Mode::Quick => (
                crate::i18n::text("ui.quick.open"),
                crate::i18n::text("ui.open"),
            ),
            Mode::Goto => (
                crate::i18n::text("ui.line.to.go.to"),
                crate::i18n::text("ui.go"),
            ),
            Mode::Rename => (
                crate::i18n::text("ui.symbol.name"),
                crate::i18n::text("ui.rename"),
            ),
        };
        title(self.mode_label, label);
        title(self.action, button);
        title(
            self.second,
            if mode == Mode::Replace {
                crate::i18n::text("ui.replace")
            } else {
                "↑"
            },
        );
        title(self.third, crate::i18n::text("ui.all"));
        title(
            self.dismiss,
            if mode.floats() {
                "×"
            } else {
                crate::i18n::text("ui.close")
            },
        );
        edit_hint(
            self.query,
            match mode {
                Mode::Find | Mode::Replace => crate::i18n::text("ui.find"),
                Mode::Rename => crate::i18n::text("ui.new.name"),
                _ => "",
            },
        );
        edit_hint(self.replacement, crate::i18n::text("ui.replace.with"));
        let initial = match mode {
            Mode::Find | Mode::Replace | Mode::Workspace => self.last_find.clone(),
            _ => String::new(),
        };
        title(self.query, &initial);
        if mode == Mode::Replace {
            title(self.replacement, "");
        }
        if matches!(mode, Mode::Workspace | Mode::Quick) {
            self.sidebar = true;
        }
        if mode == Mode::Quick {
            self.refresh_file_index();
            self.show_files("");
        }
        if reflow {
            self.layout();
        } else {
            self.layout_editor_tools(mode);
        }
        if mode.floats() && !was_floating {
            self.animate_editor_tools(true);
        }
        send(self.query, 0x00b1, 0, -1);
        unsafe {
            SetFocus(self.query);
        }
    }

    fn clear_mode(&mut self) {
        self.cancel_editor_tools_motion();
        self.mode = None;
        self.symbol_rename = None;
        self.cancel_explorer_edit(false);
        self.search = None;
        self.show_files("");
        self.layout();
        self.focus_editor();
    }

    fn dismiss_mode(&mut self) {
        if self.mode.is_some_and(Mode::floats) {
            self.mode = None;
            self.symbol_rename = None;
            self.cancel_explorer_edit(false);
            self.animate_editor_tools(false);
            self.focus_editor();
        } else {
            self.clear_mode();
        }
    }

    fn find(&mut self, backward: bool) {
        let Some(editor) = self.editor() else {
            return;
        };
        let query = if matches!(self.mode, Some(Mode::Find | Mode::Replace)) {
            caption(self.query)
        } else {
            self.last_find.clone()
        };
        if query.is_empty() {
            self.show_mode(Mode::Find);
            return;
        }
        self.last_find = query.clone();
        let code = editor_text(editor);
        let range = selection(editor);
        let begin = text::byte_offset(&code, range.start);
        let end = text::byte_offset(&code, range.end);
        let found = if backward {
            code[..begin].rfind(&query).or_else(|| code.rfind(&query))
        } else {
            code[end..]
                .find(&query)
                .map(|index| index + end)
                .or_else(|| code.find(&query))
        };
        if let Some(index) = found {
            self.reveal(
                text::wide_offset(&code, index),
                text::wide_offset(&code, index + query.len()),
            );
            self.report(crate::i18n::text("ui.selected.a.search.result"));
        } else {
            self.report(crate::i18n::text("ui.no.matching.text.found"));
        }
    }

    fn replace(&mut self, all: bool) {
        if self
            .active
            .is_some_and(|index| self.documents[index].is_preview())
        {
            self.report(crate::i18n::text("ui.preview.tabs.are.read.only"));
            return;
        }
        let Some(editor) = self.editor() else {
            return;
        };
        let query = caption(self.query);
        if query.is_empty() {
            return;
        }
        let code = editor_text(editor);
        let replacement = caption(self.replacement);
        if all {
            let count = code.matches(&query).count();
            if count == 0 {
                self.report(crate::i18n::text("ui.no.matching.text.found"));
                return;
            }
            select(editor, 0, -1);
            self.insert(editor, &code.replace(&query, &replacement));
            self.report(&crate::i18n::message(
                "ui.replaced.matches",
                &[(count).to_string()],
            ));
        } else {
            let range = selection(editor);
            let start = text::byte_offset(&code, range.start);
            let end = text::byte_offset(&code, range.end);
            if code[start..end] == query {
                self.insert(editor, &replacement);
            }
            self.find(false);
        }
    }

    fn show_problems(&mut self) {
        self.git.open = false;
        let Some(handle) = self.editor() else {
            return;
        };
        let code = editor_text(handle);
        let language = self.documents[self.active.expect("active editor")].language;
        if code.len() > HIGHLIGHT_LIMIT {
            self.report(crate::i18n::text(
                "ui.quick.bracket.checking.is.skipped.for.large",
            ));
            return;
        }
        self.mode = None;
        self.search = None;
        self.sidebar = true;
        self.showing_issues = true;
        let items = text::brackets(&code, language)
            .into_iter()
            .map(|(index, kind)| ListItem::Issue {
                index,
                line: code[..index].bytes().filter(|&b| b == b'\n').count(),
                kind,
            })
            .collect::<Vec<_>>();
        self.report(&crate::i18n::message(
            "ui.bracket.issues",
            &[(items.len()).to_string()],
        ));
        title(
            self.sidebar_title,
            crate::i18n::text("ui.problems.bracket.check"),
        );
        self.set_list(items);
        self.layout();
    }

    fn dismiss_completion_request(&mut self) {
        self.completion_pending = None;
        unsafe {
            KillTimer(self.window, COMPLETION_TIMER);
        }
    }

    fn dismiss_completion(&mut self) {
        self.dismiss_completion_request();
        if self.suggestion_editor().is_some() {
            self.close_menu(false);
        }
    }

    fn tick_completion(&mut self) {
        let pending = self.completion_pending;
        self.dismiss_completion_request();
        if pending.is_some()
            && self.editor() == pending
            && unsafe { GetFocus() } == pending.unwrap_or(0)
            && (self.menu_owner().is_none() || self.suggestion_editor().is_some())
        {
            self.complete(false);
        }
    }

    fn complete(&mut self, manual: bool) {
        self.dismiss_completion_request();
        let Some(editor) = self.editor() else {
            return;
        };
        let selected = selection(editor);
        let document = &self.documents[self.active.expect("active editor")];
        if document.is_preview() {
            self.dismiss_completion();
            return;
        }
        let lang = document.language;
        if selected.start != selected.end
            || editor_size(editor) > HIGHLIGHT_LIMIT
            || !manual && lang == Language::Plain
        {
            self.dismiss_completion();
            return;
        }
        let code = editor_text(editor);
        let cursor = text::byte_offset(&code, selected.end);
        let indent = if self.settings.spaces == 0 {
            "\t"
        } else {
            &"        "[..self.settings.spaces]
        };
        let Some(suggestions) = text::completion::suggest(&code, cursor, lang, manual, indent)
        else {
            self.dismiss_completion();
            if manual {
                self.report(crate::i18n::text("ui.no.completion.suggestions"));
            }
            return;
        };
        let range = Selection {
            start: text::wide_offset(&code, suggestions.replace.start),
            end: text::wide_offset(&code, suggestions.replace.end),
        };
        self.show_suggestions(chrome::Suggestions {
            editor,
            caret: selected.end,
            range,
            candidates: suggestions.candidates,
        });
    }

    fn comment(&mut self) {
        let Some(editor) = self.editor() else {
            return;
        };
        let Some(prefix) = self.documents[self.active.unwrap()].language.line_comment() else {
            self.report(crate::i18n::text(
                "ui.line.comments.are.not.configured.for.this",
            ));
            return;
        };
        let code = editor_text(editor);
        let range = selection(editor);
        let start = text::byte_offset(&code, range.start);
        let end = text::byte_offset(&code, range.end);
        let first = code[..start].rfind('\n').map_or(0, |index| index + 1);
        let last = if end > start && code[..end].ends_with('\n') {
            end - 1
        } else {
            code[end..]
                .find('\n')
                .map_or(code.len(), |index| end + index)
        };
        let lines = code[first..last].split('\n');
        let remove = lines
            .clone()
            .filter(|line| !line.trim().is_empty())
            .all(|line| line.trim_start().starts_with(prefix));
        let mut result = Vec::new();
        for line in lines {
            let content = line.trim_start();
            let indent = &line[..line.len() - content.len()];
            if let Some(rest) = remove.then(|| content.strip_prefix(prefix)).flatten() {
                result.push(format!(
                    "{indent}{}",
                    rest.strip_prefix(' ').unwrap_or(rest)
                ));
            } else if !remove && !content.is_empty() {
                result.push(format!("{indent}{prefix} {content}"));
            } else {
                result.push(line.to_owned());
            }
        }
        select(
            editor,
            text::wide_offset(&code, first),
            text::wide_offset(&code, last),
        );
        self.insert(editor, &result.join("\n"));
    }

    fn insert(&mut self, editor: Handle, value: &str) {
        if self
            .documents
            .iter()
            .any(|doc| doc.editor == editor && doc.is_preview())
        {
            return;
        }
        if !value.is_empty() {
            let range = selection(editor);
            if range.start != range.end {
                self.invalidate_editor_styles(editor);
            }
        }
        send(editor, EM_STOPGROUPTYPING, 0, 0);
        replace_selection(editor, value);
        self.changed(editor);
        self.update_caret_status();
    }

    fn invalidate_editor_styles(&mut self, editor: Handle) {
        if let Some(doc) = self.documents.iter_mut().find(|doc| doc.editor == editor) {
            doc.highlighting = None;
        }
    }

    fn changed(&mut self, editor: Handle) {
        let Some(index) = self.documents.iter().position(|doc| doc.editor == editor) else {
            return;
        };
        self.documents[index].revision = self.documents[index].revision.wrapping_add(1);
        self.documents[index].data_view = None;
        let syntax = self.documents[index].language != Language::Plain;
        self.documents[index].highlight_pending = syntax;
        let dirty = send(editor, EM_GETMODIFY, 0, 0) != 0;
        if self.documents[index].dirty != dirty {
            self.documents[index].dirty = dirty;
            self.refresh_tab();
            self.update_status();
        }
        if self.showing_issues {
            self.show_files("");
        }
        repaint(if self.documents[index].group == 0 {
            self.gutter
        } else {
            self.other_gutter
        });
        self.queue_session_save();
        if syntax {
            self.queue_highlight();
        }
    }

    fn queue_highlight(&self) {
        unsafe {
            SetTimer(self.window, COLOR_TIMER, 220, 0);
        }
    }

    fn on_command(&mut self, id: usize, event: u16, sender: Handle) {
        match (id, event) {
            (data_view::TREE, 4) | (data_view::PRETTY_EDITOR, EN_SETFOCUS) => {
                self.data_view_focus(sender)
            }
            (data_view::EXPAND_ALL | data_view::COLLAPSE_ALL, 0) => {
                self.expand_data_view(sender, id == data_view::EXPAND_ALL);
            }
            (browser::WORK_FOLDER | browser::OPEN_FILE | browser::RECENT_FILE, 0) => {
                if let Err(error) = self.browser_action(id, sender) {
                    self.show_error(error);
                }
            }
            (browser::QUERY_ID, 0x0200) => self.browser_focus_outline(sender),
            (browser::QUERY_ID, EN_CHANGE) => self.browse_input(sender),
            (LIST, 4) => self.recent_file_list = sender,
            (browser::QUERY_ID, EN_SETFOCUS) | (browser::RESULTS, 4) => {
                if id == browser::RESULTS {
                    self.recent_file_list = sender;
                }
                self.browser_focus(sender);
                self.browser_focus_outline(sender);
            }
            (EDITOR, EN_CHANGE) => {
                self.changed(sender);
                if self.editor() == Some(sender)
                    && unsafe { GetFocus() } == sender
                    && !chrome::deleting_text(sender)
                    && self.documents[self.active.unwrap()].language != Language::Plain
                {
                    self.completion_pending = Some(sender);
                    unsafe {
                        SetTimer(self.window, COMPLETION_TIMER, 125, 0);
                    }
                }
            }
            (EDITOR, 0x0200)
                if self.suggestion_editor() == Some(sender)
                    || self.completion_pending == Some(sender) =>
            {
                self.dismiss_completion();
            }
            (EDITOR | data_view::PRETTY_EDITOR, EN_VSCROLL) => {
                chrome::queue_scroll_sync(self.window, sender)
            }
            (EDITOR, EN_SETFOCUS) => {
                if let Some(index) = self.documents.iter().position(|doc| doc.editor == sender)
                    && self.active != Some(index)
                {
                    self.activate(index, false);
                }
            }
            (QUERY, EN_CHANGE) if self.mode == Some(Mode::Quick) => {
                self.show_files(&caption(self.query))
            }
            (LIST, 1) => self.track_explorer_selection(),
            (LIST, 2) if !self.is_tree() && !self.git.open => {
                if let Err(error) = self.open_selected() {
                    self.show_error(error);
                }
            }
            (git::GRAPH_LIST, 2) => {
                if let Err(error) = self.open_graph_commit() {
                    self.show_error(error);
                }
            }
            (_, 0) => self.command(id),
            _ => {}
        }
    }

    fn on_notify(&mut self, notice: &Notify) {
        if notice.id == EDITOR
            && notice.code == EN_SELCHANGE
            && self.editor() == Some(notice.window)
        {
            self.update_caret_status();
        }
    }

    fn command(&mut self, id: usize) {
        if let Err(error) = self.perform(id) {
            self.show_error(error);
        } else if self.menu_owner().is_none() {
            self.queue_session_save();
        }
    }

    fn perform(&mut self, id: usize) -> io::Result<()> {
        match id {
            data_view::FIRST_MODE..=data_view::LAST_MODE => self.change_data_mode(id)?,
            data_view::OPEN_WEB_FIRST..=data_view::OPEN_WEB_LAST => {
                self.open_markup_in_browser(id - data_view::OPEN_WEB_FIRST)?
            }
            file_actions::COPY | file_actions::PASTE => {
                self.file_clipboard(id, self.choose_clipboard_list())?;
            }
            browser::NEW_TAB => self.new_browser()?,
            dock::NEW_SECONDARY => {
                let base = self.root.clone().map_or_else(std::env::current_dir, Ok)?;
                if let Some(path) = folder_dialog(
                    self.window,
                    Some(&base),
                    crate::i18n::text("ui.open.a.folder.in.secondary.explorer"),
                )? {
                    self.open_secondary_browser(path)?;
                }
            }
            dock::CLOSE_SECONDARY => {
                self.secondary_browser = None;
                self.layout();
            }
            git::EXPLORER_VIEW
            | git::CHANGES_VIEW
            | git::REFRESH
            | git::COMMIT
            | git::STAGE
            | git::UNSTAGE
            | git::GRAPH_BRANCH
            | git::GRAPH_COMPARE
            | git::FETCH
            | git::PULL
            | git::PUSH
            | git::REF_PICK_START..=git::REF_PICK_END => self.git_action(id)?,
            explorer::NEW_FILE => self.new_explorer_item(false)?,
            explorer::NEW_FOLDER => self.new_explorer_item(true)?,
            terminal::TOGGLE
            | terminal::NEW
            | terminal::NEW_CMD
            | terminal::DOCK_BOTTOM
            | terminal::DOCK_LEFT
            | terminal::DOCK_RIGHT
            | terminal::DOCK_SIDEBAR
            | terminal::CLOSE_TAB => self.terminal_command(id)?,
            NEW => self.new_document()?,
            WELCOME_NEW => self.new_browser()?,
            NEW_WINDOW => {
                self.new_window_sender()?
                    .send(dock::Transfer::Empty)
                    .map_err(|_| {
                        io::Error::other(crate::i18n::text("ui.could.not.open.a.new.window"))
                    })?;
            }
            OPEN | WELCOME_OPEN => {
                let paths = file_dialog(self.window, false, OsStr::new(""))?;
                self.open_paths(paths);
            }
            FOLDER | WELCOME_FOLDER => {
                if let Some(path) = folder_dialog(
                    self.window,
                    self.root.as_deref(),
                    crate::i18n::text("ui.open.workspace.folder"),
                )? {
                    self.load_folder(path)?;
                }
            }
            SAVE | SAVE_AS => {
                if let Some(index) = self.active {
                    self.save_document(index, id == SAVE_AS)?;
                }
            }
            SAVE_ALL => {
                for index in 0..self.documents.len() {
                    if self.documents[index].dirty && !self.save_document(index, false)? {
                        break;
                    }
                }
            }
            CLOSE => self.close_tab(),
            REOPEN => {
                if let Some(path) = self.recently_closed.pop() {
                    self.open_path(&path)?;
                }
            }
            FIND => self.show_mode(Mode::Find),
            REPLACE => self.show_mode(Mode::Replace),
            WORKSPACE => self.show_mode(Mode::Workspace),
            QUICK => self.show_mode(Mode::Quick),
            GOTO => self.show_mode(Mode::Goto),
            EXPLORER => {
                self.cancel_explorer_edit(false);
                self.sidebar = !self.sidebar;
                if self.sidebar {
                    self.clear_mode();
                    self.request_git_refresh();
                }
                self.layout();
            }
            REFRESH => {
                if let Some(path) = self.root.clone() {
                    self.load_folder(path)?;
                }
            }
            CLOSE_FOLDER => {
                self.root = None;
                self.files.clear();
                self.files_stale = false;
                self.expanded.clear();
                self.explorer_selection = None;
                self.explorer.selected.clear();
                self.explorer.archive.reset();
                self.explorer_top = 0;
                self.clear_mode();
                self.request_git_refresh();
            }
            PROBLEMS => self.show_problems(),
            FONT => self.open_font_preferences()?,
            SPLIT => self.toggle_split(),
            THEME | BIGGER | SMALLER | RESET_FONT | WRAP | INDENT_TWO | INDENT_FOUR
            | INDENT_TAB => {
                match id {
                    THEME => self.settings.theme = Theme::from_dark(!self.settings.dark()),
                    BIGGER => self.settings.points = (self.settings.points + 1).min(36),
                    SMALLER => self.settings.points = (self.settings.points - 1).max(8),
                    RESET_FONT => self.settings.points = Settings::default().points,
                    WRAP => self.settings.wrap = !self.settings.wrap,
                    INDENT_TWO => self.settings.spaces = 2,
                    INDENT_FOUR => self.settings.spaces = 4,
                    INDENT_TAB => self.settings.spaces = 0,
                    _ => unreachable!(),
                }
                self.apply_preferences();
                self.settings.save()?;
                self.focus_editor();
            }
            DOCK_SIDEBAR => {
                self.settings.sidebar_right = !self.settings.sidebar_right;
                self.layout();
                self.save_sidebar_settings();
            }
            COLLAPSE_FOLDERS => {
                self.expanded.clear();
                self.explorer_top = 0;
                self.clear_mode();
            }
            PREFERENCES => self.open_preferences()?,
            FULLSCREEN => self.toggle_fullscreen(),
            FILE_MENU | EDIT_MENU | TOOLS | terminal::MENU => self.show_toolbar_menu(id),
            ACTION => match self.mode {
                Some(Mode::Find | Mode::Replace) => self.find(false),
                Some(Mode::Workspace) => self.start_search(),
                Some(Mode::Quick) => self.open_selected()?,
                Some(Mode::Goto) => {
                    if let Ok(line) = caption(self.query).trim().parse::<usize>() {
                        self.clear_mode();
                        self.jump_line(line.saturating_sub(1), 0);
                    }
                }
                Some(Mode::Rename) => self.commit_symbol_rename()?,
                None => {}
            },
            SECOND => {
                if self.mode == Some(Mode::Replace) {
                    self.replace(false);
                } else {
                    self.find(true);
                }
            }
            THIRD => self.replace(true),
            DISMISS => self.dismiss_mode(),
            _ => {}
        }
        Ok(())
    }

    fn toolbar_handle(&self, id: usize) -> Handle {
        if id == git::GRAPH_BRANCH {
            return self.git.graph.branch;
        }
        if id == git::GRAPH_COMPARE {
            return self.git.graph.compare;
        }
        BUTTONS
            .iter()
            .position(|button| button.0 == id)
            .map_or(self.window, |index| self.toolbar[index].0)
    }

    fn show_toolbar_menu(&mut self, id: usize) {
        let items: &[(usize, &str)] = match id {
            FILE_MENU => &[
                (browser::NEW_TAB, crate::i18n::text("ui.new.tab_2")),
                (NEW, crate::i18n::text("ui.new.file_2")),
                (NEW_WINDOW, crate::i18n::text("ui.new.window")),
                (OPEN, crate::i18n::text("ui.open.file_2")),
                (FOLDER, crate::i18n::text("ui.open.folder")),
                (QUICK, crate::i18n::text("ui.quick.open_2")),
                (CLOSE_FOLDER, crate::i18n::text("ui.close.folder")),
                (0, ""),
                (SAVE, crate::i18n::text("ui.save")),
                (SAVE_AS, crate::i18n::text("ui.save.as")),
                (SAVE_ALL, crate::i18n::text("ui.save.all")),
                (0, ""),
                (CLOSE, crate::i18n::text("ui.close.tab")),
                (REOPEN, crate::i18n::text("ui.reopen.closed.tab")),
            ],
            EDIT_MENU => &[
                (
                    file_actions::COPY,
                    crate::i18n::text("ui.copy.files.and.folders"),
                ),
                (
                    file_actions::PASTE,
                    crate::i18n::text("ui.paste.files.and.folders"),
                ),
                (0, ""),
                (FIND, crate::i18n::text("ui.find_2")),
                (REPLACE, crate::i18n::text("ui.find.and.replace_2")),
                (WORKSPACE, crate::i18n::text("ui.search.workspace_2")),
                (GOTO, crate::i18n::text("ui.go.to.line")),
            ],
            TOOLS => &[
                (
                    dock::NEW_SECONDARY,
                    crate::i18n::text("ui.open.secondary.explorer"),
                ),
                (
                    dock::CLOSE_SECONDARY,
                    crate::i18n::text("ui.close.secondary.explorer"),
                ),
                (EXPLORER, crate::i18n::text("ui.toggle.sidebar")),
                (git::EXPLORER_VIEW, crate::i18n::text("ui.explorer_2")),
                (git::CHANGES_VIEW, crate::i18n::text("ui.source.control_3")),
                (
                    DOCK_SIDEBAR,
                    if self.settings.sidebar_right {
                        crate::i18n::text("ui.move.sidebar.left")
                    } else {
                        crate::i18n::text("ui.move.sidebar.right")
                    },
                ),
                (
                    COLLAPSE_FOLDERS,
                    crate::i18n::text("ui.collapse.all.folders"),
                ),
                (REFRESH, crate::i18n::text("ui.refresh.folder")),
                (0, ""),
                (PROBLEMS, crate::i18n::text("ui.problems.bracket.check_2")),
                (FULLSCREEN, crate::i18n::text("ui.toggle.full.screen")),
                (SPLIT, crate::i18n::text("ui.split.editor.with.another.tab")),
                (terminal::TOGGLE, crate::i18n::text("ui.toggle.terminal")),
            ],
            terminal::MENU => &[
                (terminal::NEW, crate::i18n::text("ui.new.powershell.tab")),
                (
                    terminal::NEW_CMD,
                    crate::i18n::text("ui.new.command.prompt.tab"),
                ),
                (
                    terminal::CLOSE_TAB,
                    crate::i18n::text("ui.close.current.terminal.tab"),
                ),
                (
                    terminal::TOGGLE,
                    if self.terminal.visible {
                        crate::i18n::text("ui.hide.terminal")
                    } else {
                        crate::i18n::text("ui.show.terminal")
                    },
                ),
                (0, ""),
                (
                    terminal::DOCK_BOTTOM,
                    crate::i18n::text("ui.dock.at.bottom"),
                ),
                (
                    terminal::DOCK_LEFT,
                    crate::i18n::text("ui.dock.on.the.left"),
                ),
                (
                    terminal::DOCK_RIGHT,
                    crate::i18n::text("ui.dock.on.the.right"),
                ),
                (
                    terminal::DOCK_SIDEBAR,
                    crate::i18n::text("terminal.dock_sidebar"),
                ),
            ],
            _ => &[],
        };
        self.open_menu(id, items);
    }

    fn toggle_fullscreen(&mut self) {
        if let Some(saved) = self.fullscreen.take() {
            fullscreen(self.window, Some(saved));
        } else if let Some(saved) = window_placement(self.window)
            && fullscreen(self.window, None)
        {
            self.fullscreen = Some(saved);
        }
        self.layout();
        self.queue_session_save();
    }

    fn collapse_editor_groups(&mut self) {
        self.other = None;
        self.active_right = false;
        for doc in &mut self.documents {
            doc.group = 0;
        }
        self.clear_tab_motion();
    }

    fn toggle_split(&mut self) {
        if self.other.is_some() {
            self.collapse_editor_groups();
            self.layout();
        } else if let Some(active) = self.active {
            let neighboring = active
                .checked_sub(1)
                .or_else(|| (self.documents.len() > 1).then_some(1));
            self.split_document(active, dock::Destination::Right, neighboring);
        }
    }

    fn keyboard(&mut self, target: Handle, key: i32) -> bool {
        if key == 0x7a && unsafe { GetKeyState(0x11) >= 0 && GetKeyState(0x12) >= 0 } {
            self.command(FULLSCREEN);
            return true;
        }
        if matches!(key, 0x43 | 0x56)
            && self.workspace_list(target)
            && unsafe { GetKeyState(0x11) < 0 && GetKeyState(0x12) >= 0 }
        {
            if let Err(error) = self.file_clipboard(
                if key == 0x43 {
                    file_actions::COPY
                } else {
                    file_actions::PASTE
                },
                target,
            ) {
                self.show_error(error);
            }
            return true;
        }
        if self.keyboard_data_view(target, key) || self.browser_key(target, key) {
            return true;
        }
        if target == self.git.message && key == 13 {
            self.command(git::COMMIT);
            return true;
        }
        if self.git.open && key == 0x74 {
            self.command(git::REFRESH);
            return true;
        }
        if target == self.explorer.input {
            match key {
                13 => {
                    if let Err(error) = self.commit_explorer_edit() {
                        self.report(&error.to_string());
                    }
                    return true;
                }
                9 | 0x1b => {
                    self.cancel_explorer_edit(true);
                    return true;
                }
                _ => return false,
            }
        }
        if target == self.list && key == 0x1b && self.explorer_editing() {
            self.cancel_explorer_edit(true);
            return true;
        }
        if target == self.list && key == 0x71 {
            self.rename_explorer_item();
            return true;
        }
        if target == self.list && key == 0x2e && self.is_tree() {
            if !unsafe { GetKeyState(0x11) < 0 || GetKeyState(0x12) < 0 }
                && let Err(error) = self.delete_explorer_items()
            {
                self.show_error(error);
            }
            return true;
        }
        if self.suggestion_editor() == Some(target) {
            let plain = unsafe { GetKeyState(0x11) >= 0 && GetKeyState(0x12) >= 0 };
            if key == 0x1b {
                self.dismiss_completion();
                return true;
            }
            if plain && matches!(key, 9 | 13 | 0x26 | 0x28) {
                if self.completion_pending == Some(target) {
                    self.complete(false);
                }
                if self.menu_key(key) {
                    return true;
                }
            } else if !plain || matches!(key, 0x20 | 0x21..=0x25 | 0x27 | 0x2d | 0x70..=0x7b) {
                self.dismiss_completion();
            }
        } else if self.menu_owner().is_some() && self.menu_key(key) {
            return true;
        }
        if self.terminal_key(target, key) {
            return true;
        }
        if target == self.terminal.body() {
            return false;
        }
        if target == self.list && self.tree_key(key) {
            return true;
        }
        if let Some(group) = self.chrome.tab_group(target)
            && matches!(key, 0x25 | 0x27)
        {
            let current = self.group_document(group).unwrap_or(0);
            let mut tabs = self
                .documents
                .iter()
                .enumerate()
                .filter(|(_, doc)| doc.group == group);
            let next = if key == 0x25 {
                tabs.rfind(|(i, _)| *i < current)
            } else {
                tabs.find(|(i, _)| *i > current)
            };
            if let Some((index, _)) = next {
                self.activate(index, false);
            }
            return true;
        }
        let down = |vk| unsafe { GetKeyState(vk) < 0 };
        let ctrl = down(0x11);
        let shift = down(0x10);
        let alt = down(0x12);
        if self.pending_chord && !matches!(key, 0x10..=0x12) {
            self.pending_chord = false;
            let command = match key {
                0x4f if ctrl => FOLDER,
                0x53 if ctrl => PREFERENCES,
                0x53 => SAVE_ALL,
                0x54 if ctrl => THEME,
                0x46 => CLOSE_FOLDER,
                _ => 0,
            };
            if command != 0 {
                self.command(command);
                return true;
            }
        }
        if ctrl && !alt {
            let command = match key {
                0x4e if shift => NEW_WINDOW,
                0x4e => NEW,
                0x4f => OPEN,
                0x53 if shift => SAVE_AS,
                0x53 => SAVE,
                0x73 | 0x57 => CLOSE,
                0x54 if shift => REOPEN,
                0x54 => browser::NEW_TAB,
                0x50 if shift => TOOLS,
                0x50 => QUICK,
                0x46 if shift => WORKSPACE,
                0x46 => FIND,
                0x48 => REPLACE,
                0x47 if shift => git::CHANGES_VIEW,
                0x47 => GOTO,
                0x4d if shift => PROBLEMS,
                0x42 => EXPLORER,
                0x45 if shift => git::EXPLORER_VIEW,
                0xbc => PREFERENCES,
                0xdc => SPLIT,
                0xbb | 0x6b => BIGGER,
                0xbd | 0x6d => SMALLER,
                0x60 => RESET_FONT,
                _ => 0,
            };
            if command != 0 {
                self.command(command);
                return true;
            }
            match key {
                0x4b if !shift => {
                    self.pending_chord = true;
                    self.report("Ctrl+K …");
                    return true;
                }
                9 | 0x21 | 0x22 if !self.documents.is_empty() => {
                    let backward = shift || key == 0x21;
                    let length = self.documents.len();
                    let current = self.active.unwrap_or(0);
                    self.activate(
                        if backward {
                            (current + length - 1) % length
                        } else {
                            (current + 1) % length
                        },
                        true,
                    );
                    return true;
                }
                _ => {}
            }
        }
        if alt && !ctrl && key == 0x5a {
            self.command(WRAP);
            return true;
        }
        match key {
            0x70 if !ctrl && !alt => {
                self.command(TOOLS);
                return true;
            }
            0x72 if !ctrl && !alt => {
                self.find(shift);
                return true;
            }
            0x1b if self.mode.is_some() => {
                self.dismiss_mode();
                return true;
            }
            13 if target == self.query || target == self.replacement => {
                self.command(if target == self.replacement {
                    SECOND
                } else {
                    ACTION
                });
                return true;
            }
            13 if target == self.list => {
                if let Err(error) = self.open_selected() {
                    self.show_error(error);
                }
                return true;
            }
            13 if target == self.git.graph.list => {
                if let Err(error) = self.open_graph_commit() {
                    self.show_error(error);
                }
                return true;
            }
            0x28 if target == self.query && self.mode == Some(Mode::Quick) => {
                unsafe {
                    SetFocus(self.list);
                }
                return true;
            }
            9 if target == self.query && self.mode == Some(Mode::Replace) => {
                unsafe {
                    SetFocus(self.replacement);
                }
                return true;
            }
            _ => {}
        }
        let Some(editor) = self.editor().filter(|&editor| editor == target) else {
            if key == 9 && !ctrl && !alt {
                self.focus_next_control(target, shift);
                return true;
            }
            return false;
        };
        if key == 0x71 && !ctrl && !shift && !alt {
            if let Err(error) = self.start_symbol_rename(editor) {
                self.report(&error.to_string());
            }
            return true;
        }
        if ctrl && !alt {
            let edit_message = match key {
                0x5a if shift => EM_REDO,
                0x5a => EM_UNDO,
                0x59 => EM_REDO,
                0x56 => 0x0400 + 64,
                _ => 0,
            };
            if edit_message != 0 {
                if key == 0x56 {
                    self.invalidate_editor_styles(editor);
                }
                send(editor, edit_message, if key == 0x56 { 13 } else { 0 }, 0);
                self.changed(editor);
                self.update_caret_status();
                return true;
            }
            match key {
                0x41 => {
                    select(editor, 0, -1);
                    self.update_status();
                    return true;
                }
                0x20 => {
                    self.complete(true);
                    return true;
                }
                0xbf => {
                    self.comment();
                    return true;
                }
                _ => {}
            }
        }
        if key == 9 && !ctrl && !alt && !shift {
            let spaces = "        ";
            let indent = if self.settings.spaces == 0 {
                "\t"
            } else {
                &spaces[..self.settings.spaces]
            };
            self.insert(editor, indent);
            return true;
        }
        false
    }

    fn focus_next_control(&self, current: Handle, backward: bool) {
        let mut controls = self
            .toolbar
            .iter()
            .map(|&(handle, _)| handle)
            .collect::<Vec<_>>();
        if let Some(mode) = self.mode {
            controls.push(self.query);
            if mode == Mode::Replace {
                controls.push(self.replacement);
            }
            controls.push(self.action);
            if matches!(mode, Mode::Find | Mode::Replace) {
                controls.push(self.second);
            }
            if mode == Mode::Replace {
                controls.push(self.third);
            }
            controls.push(self.dismiss);
        }
        if self.sidebar {
            controls.extend([self.git.explorer, self.git.changes]);
            if self.git.open && self.git.has_repository() {
                controls.extend(self.git.action_controls());
            }
            if self.root.is_some() && self.is_tree() {
                controls.extend([self.explorer.file, self.explorer.folder]);
            }
            controls.push(self.list);
            if self.graph_visible() {
                controls.extend(self.git.graph.remote);
                controls.extend([
                    self.git.graph.branch,
                    self.git.graph.compare,
                    self.git.graph.list,
                ]);
            }
        }
        if self.terminal.visible {
            controls.push(self.terminal.body());
        }
        for group in 0..2 {
            if group == 1 && self.other.is_none() {
                continue;
            }
            controls.push(self.chrome.tab_handle(group));
            controls.extend(self.data_buttons[group]);
            if let Some(doc) = self.group_document(group).map(|i| &self.documents[i]) {
                if let Some(view) = &doc.browser {
                    controls.extend(view.keyboard_controls());
                } else if doc.data_mode() != data_view::ViewMode::Raw {
                    if let Some(view) = doc.data_view.as_ref() {
                        controls.extend(view.keyboard_controls());
                    }
                } else {
                    controls.push(doc.editor);
                }
            }
        }
        if self.documents.is_empty() {
            controls.extend(self.welcome_actions);
        }
        controls.retain(|&handle| unsafe {
            IsWindowVisible(handle) != 0 && IsWindowEnabled(handle) != 0
        });
        if controls.is_empty() {
            return;
        }
        let index = controls
            .iter()
            .position(|&handle| handle == current)
            .unwrap_or(0);
        let length = controls.len();
        let next = if backward {
            (index + length - 1) % length
        } else {
            (index + 1) % length
        };
        unsafe {
            SetFocus(controls[next]);
        }
    }

    fn start_symbol_rename(&mut self, editor: Handle) -> io::Result<()> {
        if self
            .active
            .is_some_and(|index| self.documents[index].is_preview())
        {
            return Err(io::Error::other(crate::i18n::text(
                "ui.preview.tabs.are.read.only",
            )));
        }
        if editor_size(editor) > 1_000_000 {
            return Err(io::Error::other(crate::i18n::text(
                "ui.this.file.is.too.large.for.symbol",
            )));
        }
        let language = self.documents[self.active.expect("active editor")].language;
        let source = editor_text(editor);
        let caret = text::byte_offset(&source, selection(editor).start);
        let rename = text::symbols::prepare(&source, caret, language)?;
        self.show_mode(Mode::Rename);
        title(self.query, &rename.name);
        send(self.query, 0x00b1, 0, -1);
        self.symbol_rename = Some((editor, rename));
        Ok(())
    }

    fn commit_symbol_rename(&mut self) -> io::Result<()> {
        let Some((editor, rename)) = self.symbol_rename.as_ref() else {
            return Ok(());
        };
        let editor = *editor;
        let name = caption(self.query);
        if name == rename.name {
            self.clear_mode();
            return Ok(());
        }
        let language = self
            .documents
            .iter()
            .find(|doc| doc.editor == editor)
            .map(|doc| doc.language)
            .ok_or_else(|| io::Error::other(crate::i18n::text("ui.the.editor.tab.closed")))?;
        let source = editor_text(editor);
        let edit = rename.apply(&source, &name, language)?;
        let first = text::wide_offset(&source, edit.range.start);
        let last = text::wide_offset(&source, edit.range.end);
        let start = first + text::wide_offset(&edit.replacement, edit.selected.start);
        let end = first + text::wide_offset(&edit.replacement, edit.selected.end);
        select(editor, first, last);
        self.insert(editor, &edit.replacement);
        select(editor, start, end);
        send(editor, EM_SCROLLCARET, 0, 0);
        self.clear_mode();
        self.update_status();
        self.report(&crate::i18n::message(
            "ui.renamed.the.symbol.in.places",
            &[(edit.count).to_string()],
        ));
        Ok(())
    }
}

fn report_error(window: Handle, error: &str) {
    message(window, error, "Maker", MB_ICONERROR);
}
fn relative<'a>(path: &'a Path, root: Option<&Path>) -> std::borrow::Cow<'a, str> {
    root.and_then(|base| path.strip_prefix(base).ok())
        .unwrap_or(path)
        .to_string_lossy()
}

fn file_name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
}

fn caret_position(editor: Handle, wrapped: bool) -> (usize, usize) {
    let index = selection(editor).end.max(0);
    if wrapped && let Some((line, start)) = editor_paragraph(editor, index) {
        let units = (index - start) as usize;
        let column = if units <= 8192 {
            editor_range(editor, start, index).chars().count()
        } else {
            units
        };
        return (line, column + 1);
    }
    let line = send(editor, EM_EXLINEFROMCHAR, 0, index as isize).max(0);
    let start = send(editor, EM_LINEINDEX, line as usize, 0).max(0);
    (line as usize, (index as isize - start).max(0) as usize + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_editor_keeps_unicode_selection_and_undo_after_recoloring() -> io::Result<()> {
        let _ole = Ole::init()?;
        setup(WINDOW_CLASS, window_proc)?;
        chrome::register()?;
        terminal::register()?;
        let window = main_window(WINDOW_CLASS)?;
        let mut app = App::new(window)?;
        let original = "fn main() {\n    call([\"한글😀\"]);\n}\n";
        app.add_document(
            Some(PathBuf::from("sample.rs")),
            "sample.rs".into(),
            original,
            Encoding::default(),
        )?;
        let editor = app.editor().expect("opened tab");
        assert_eq!(editor_text(editor), original);
        assert!(
            app.documents[0].highlighting.is_some(),
            "native syntax formatting initialized"
        );
        let bracket_color = |needle: &str| {
            let at = original.find(needle).expect("test bracket");
            let start = text::wide_offset(original, at);
            select(editor, start, start + 1);
            selection_foreground(editor)
        };
        for dark in [true, false] {
            app.settings.theme = Theme::from_dark(dark);
            app.apply_preferences();
            assert_eq!(
                bracket_color("([\""),
                PAINT.get().unwrap().highlight(Color::Bracket(1))
            );
            assert_eq!(
                bracket_color("[\""),
                PAINT.get().unwrap().highlight(Color::Bracket(2))
            );
            assert_eq!(
                bracket_color("]);"),
                PAINT.get().unwrap().highlight(Color::Bracket(2))
            );
            assert_eq!(
                bracket_color("\"한"),
                PAINT.get().unwrap().highlight(Color::String)
            );
        }

        let offset = original.find("한글😀").expect("test word");
        let start = text::wide_offset(original, offset);
        let end = start + "한글😀".encode_utf16().count() as i32;
        select(editor, start, end);
        app.insert(editor, "다른😀");
        let edited = original.replace("한글😀", "다른😀");
        assert_eq!(editor_text(editor), edited);
        let caret = selection(editor);
        app.repaint_editor();
        assert_eq!(selection(editor), caret);
        assert!(app.documents[0].dirty);

        send(editor, EM_UNDO, 0, 0);
        assert_eq!(editor_text(editor), original);
        send(editor, EM_REDO, 0, 0);
        assert_eq!(editor_text(editor), edited);

        app.settings.wrap = true;
        app.apply_preferences();
        let second_line = edited.find('\n').unwrap() + 1;
        let content = edited[second_line..].find('\n').unwrap();
        let end = text::wide_offset(&edited, second_line + content);
        select(editor, end, end);
        assert_eq!(
            caret_position(editor, true),
            (
                1,
                edited[second_line..second_line + content].chars().count() + 1
            )
        );
        let end = text::wide_offset(&edited, second_line);
        select(editor, end, end);
        assert_eq!(caret_position(editor, true), (1, 1));

        app.settings.wrap = false;
        app.apply_preferences();
        select(editor, 0, -1);
        app.insert(editor, &edited);
        app.repaint_editor();
        select(editor, 0, 2);
        assert_eq!(
            selection_foreground(editor),
            PAINT.get().unwrap().highlight(Color::Keyword)
        );

        let symbol = text::wide_offset(&edited, edited.find("fn ").unwrap() + 1);
        select(editor, symbol, symbol + 1);
        app.insert(editor, "");
        app.repaint_editor();
        select(editor, symbol - 1, symbol);
        assert_eq!(
            selection_foreground(editor),
            PAINT.get().unwrap().foreground,
            "deleting part of a keyword restores its ordinary color",
        );

        let large = "fn sample() { let text = \"한글😀\"; }\n".repeat(9_000);
        app.add_document(
            Some(PathBuf::from("large.rs")),
            "large.rs".into(),
            &large,
            Encoding::default(),
        )?;
        let editor = app.editor().expect("large tab");
        let first_line_end = text::wide_offset(&large, large.find('\n').unwrap() + 1);
        assert_eq!(
            editor_range(editor, 0, first_line_end * 2),
            large.lines().take(2).collect::<Vec<_>>().join("\n") + "\n"
        );
        send(editor, EM_LINESCROLL, 0, 7_000);
        let line = send(editor, 0x00ce, 0, 0);
        assert!(line >= 6_000, "large viewport moved to line {line}");
        app.repaint_viewport();
        let start = send(editor, 0x00bb, line as usize, 0) as i32;
        let range = app.documents[app.active.unwrap()]
            .colored_view
            .expect("highlighted viewport");
        assert!(range.start > 0 && range.start <= start && start + 2 <= range.end);
        select(editor, start, start + 2);
        assert_eq!(
            selection_foreground(editor),
            PAINT.get().unwrap().highlight(Color::Keyword)
        );
        send(editor, EM_LINESCROLL, 0, 1);
        app.repaint_viewport();
        assert_eq!(app.documents[app.active.unwrap()].colored_view, Some(range));

        let diff = "diff --git a/sample b/sample\n--- a/sample\n+++ b/sample\n@@ -1,2 +1,2 @@\n--- old\n+++ 새 😀\n context\n";
        app.add_document(None, "Git diff".into(), diff, Encoding::default())?;
        let index = app.active.unwrap();
        let editor = app.documents[index].editor;
        app.documents[index].language = Language::Diff;
        send(editor, EM_SETREADONLY, 1, 0);
        for theme in [Theme::Dark, Theme::Light] {
            app.settings.theme = theme;
            app.apply_preferences();
            let paint = PAINT.get().unwrap();
            for (prefix, foreground, background) in [
                ("--- a/", paint.highlight(Color::Markup), paint.editor_rgb),
                (
                    "--- old",
                    paint.highlight(Color::DiffRemoved),
                    paint.diff_backgrounds[1],
                ),
                (
                    "+++ 새",
                    paint.highlight(Color::DiffAdded),
                    paint.diff_backgrounds[0],
                ),
                (" context", paint.foreground, paint.editor_rgb),
            ] {
                let start = text::wide_offset(diff, diff.find(prefix).unwrap());
                select(editor, start, start + prefix.encode_utf16().count() as i32);
                assert_eq!(
                    selection_colors(editor),
                    (foreground, background),
                    "{prefix}"
                );
            }
        }
        assert_eq!(editor_text(editor), diff);
        web_preview_keeps_tabs_and_terminal(&mut app)?;

        unsafe {
            DestroyWindow(window);
        }
        PAINT.with(|paint| {
            if let Some(old) = paint.take() {
                old.release();
            }
        });
        Ok(())
    }

    fn web_preview_keeps_tabs_and_terminal(app: &mut App) -> io::Result<()> {
        app.terminal.visible = true;
        app.simulate_http_response("<!doctype html><html><body><p>Preview</p></body></html>")?;
        let index = app.active.unwrap();
        assert!(app.documents[index].response);
        let terminal_header = unsafe { GetDlgItem(app.window, terminal::HEADER as i32) };
        let terminal_body = unsafe { GetDlgItem(app.window, terminal::BODY as i32) };
        let tabs = app.chrome.tab_handle(app.documents[index].group);
        let chrome = [tabs, terminal_header, terminal_body];
        let original = chrome.map(|window| relative_rect(window, app.window));
        let assert_chrome = |app: &App| {
            assert!(chrome.into_iter().all(is_shown));
            assert!(
                original
                    .iter()
                    .all(|rect| rect.width() > 0 && rect.height() > 0)
            );
            assert!(chrome.map(|window| relative_rect(window, app.window)) == original);
        };

        app.change_document_data_mode(index, data_view::ViewMode::Pretty)?;
        assert_chrome(app);
        let editor = app.documents[index].editor;
        let mut pretty = None;
        for _ in 0..100 {
            app.complete_data_view(editor);
            pretty = app.documents[index]
                .data_view
                .as_ref()
                .and_then(data_view::View::pretty_editor);
            if pretty.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let pretty = pretty.expect("Pretty Print finished loading the response");
        assert!(is_shown(pretty));
        assert!(editor_text(pretty).contains("<p>Preview</p>"));
        assert_chrome(app);

        for (kind, header) in [
            (dock::TabKind::File, original[0]),
            (dock::TabKind::Terminal, original[1]),
        ] {
            app.receive_tab_preview(
                kind as usize,
                Point {
                    x: header.left,
                    y: header.top,
                },
            );
            let guide = relative_rect(app.dock.guide, app.window);
            assert!(is_shown(app.dock.guide));
            assert!(header.has(Point {
                x: guide.left,
                y: guide.top
            }));
            assert!(header.has(Point {
                x: guide.right - 1,
                y: guide.bottom - 1
            }));
        }
        app.receive_tab_preview(0, Point::default());
        assert!(!is_shown(app.dock.guide));
        assert_chrome(app);
        Ok(())
    }
}
