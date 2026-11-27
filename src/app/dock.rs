use super::*;
use std::{
    collections::HashMap,
    sync::{Condvar, LazyLock, Mutex},
};

pub(super) const GUIDE: usize = 379;
pub(super) const SPLITTER: usize = 380;
pub(super) const NEW_SECONDARY: usize = 381;
pub(super) const CLOSE_SECONDARY: usize = 382;
pub(super) const SIDEBAR_SPLITTER: usize = 383;
pub(super) const WINDOW_DROP: u32 = 0x8024;
pub(super) const WINDOW_RECEIPT: u32 = 0x8025;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Destination {
    Left,
    Right,
    Top,
    Bottom,
    Sidebar,
    TerminalSidebar,
    Window,
}

pub(super) enum Transfer {
    Empty,
    Path(PathBuf),
    Browser(PathBuf, bool, String),
    Editor(Box<TransferredEditor>),
    Terminal(Box<terminal::Tab>),
}

pub(super) struct TransferredEditor {
    path: Option<PathBuf>,
    label: String,
    text: String,
    encoding: Encoding,
    language: Language,
    dirty: bool,
    response: bool,
    response_url: Option<String>,
    git_diff: Option<(PathBuf, bool)>,
    archive_member: Option<(PathBuf, PathBuf)>,
    selection: Selection,
    data_mode: data_view::ViewMode,
}

pub(super) struct DropTarget {
    window: Handle,
    point: Point,
}

struct Incoming {
    transfer: Transfer,
    point: Point,
    source: Option<(Handle, Handle)>,
}

static WINDOWS: LazyLock<Mutex<HashMap<Handle, mpsc::Sender<Incoming>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) struct WindowRegistration(Handle);

impl Drop for WindowRegistration {
    fn drop(&mut self) {
        WINDOWS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.0);
    }
}

impl DropTarget {
    pub fn send(
        &self,
        transfer: Transfer,
        source: Option<(Handle, Handle)>,
    ) -> Result<(), Transfer> {
        let windows = WINDOWS.lock().unwrap_or_else(|error| error.into_inner());
        let Some(sender) = windows.get(&self.window) else {
            return Err(transfer);
        };
        if unsafe { PostMessageW(self.window, WINDOW_DROP, 0, 0) } == 0 {
            return Err(transfer);
        }
        sender
            .send(Incoming {
                transfer,
                point: self.point,
                source,
            })
            .map_err(|error| error.0.transfer)
    }
}

pub(super) struct Dock {
    pub guide: Handle,
    pub splitter: Handle,
    pub sidebar_splitter: Handle,
    pub sidebar_ratio: i32,
    pub sidebar_area: Rect,
    pub terminal_sidebar: Rect,
    pub area: Rect,
    pub sidebar: Rect,
    pub stacked: bool,
    pub ratio: i32,
    preview: Option<Destination>,
    resizing: bool,
    list_drag: Option<ListDrag>,
    incoming: Option<mpsc::Receiver<Incoming>>,
    pending: HashMap<Handle, u64>,
}

struct ListDrag {
    control: Handle,
    origin: Point,
    path: PathBuf,
    folder: bool,
    row: usize,
}

static DETACHED: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

struct WindowThread;

impl WindowThread {
    fn new() -> Self {
        *DETACHED.0.lock().unwrap_or_else(|error| error.into_inner()) += 1;
        Self
    }
}

impl Drop for WindowThread {
    fn drop(&mut self) {
        *DETACHED.0.lock().unwrap_or_else(|error| error.into_inner()) -= 1;
        DETACHED.1.notify_all();
    }
}

pub(super) fn wait_for_windows() {
    let count = DETACHED.0.lock().unwrap_or_else(|error| error.into_inner());
    drop(DETACHED.1.wait_while(count, |count| *count != 0));
}

impl Dock {
    pub fn new(window: Handle) -> io::Result<Self> {
        Ok(Self {
            guide: child(window, GUIDE, chrome::SURFACE, "", 0)?,
            splitter: child(window, SPLITTER, chrome::SURFACE, "", 0)?,
            sidebar_splitter: child(window, SIDEBAR_SPLITTER, chrome::SURFACE, "", 0)?,
            sidebar_ratio: 50,
            sidebar_area: Rect::default(),
            terminal_sidebar: Rect::default(),
            area: Rect::default(),
            sidebar: Rect::default(),
            stacked: false,
            ratio: 50,
            preview: None,
            resizing: false,
            list_drag: None,
            incoming: None,
            pending: HashMap::new(),
        })
    }

    pub fn forget_window_move(&mut self, editor: Handle) {
        self.pending.remove(&editor);
    }

    pub fn register(&mut self, window: Handle) -> WindowRegistration {
        let (sender, receiver) = mpsc::channel();
        WINDOWS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(window, sender);
        self.incoming = Some(receiver);
        WindowRegistration(window)
    }

    pub fn panes(&self, axis_gap: i32) -> (Rect, Rect, Rect) {
        let area = self.area;
        if self.stacked {
            let middle = area.top + (area.height() - axis_gap).max(0) * self.ratio / 100;
            let first = Rect {
                bottom: middle,
                ..area
            };
            let second = Rect {
                top: middle + axis_gap,
                ..area
            };
            let bar = Rect {
                top: middle,
                bottom: second.top,
                ..area
            };
            (first, second, bar)
        } else {
            let middle = area.left + (area.width() - axis_gap).max(0) * self.ratio / 100;
            let first = Rect {
                right: middle,
                ..area
            };
            let second = Rect {
                left: middle + axis_gap,
                ..area
            };
            let bar = Rect {
                left: middle,
                right: second.left,
                ..area
            };
            (first, second, bar)
        }
    }
}

fn destination(
    area: Rect,
    sidebar: Rect,
    pt: Point,
    folder: bool,
    window_only: bool,
) -> Destination {
    if window_only {
        return Destination::Window;
    }
    if folder && sidebar.has(pt) {
        return Destination::Sidebar;
    }
    if !area.has(pt) {
        return Destination::Window;
    }
    let x = (pt.x - area.left) * 100 / area.width().max(1);
    let y = (pt.y - area.top) * 100 / area.height().max(1);
    if x < 25 {
        Destination::Left
    } else if x >= 75 {
        Destination::Right
    } else if y < 26 {
        Destination::Top
    } else if y >= 74 {
        Destination::Bottom
    } else {
        Destination::Window
    }
}

impl App {
    pub(super) fn transfer_target(&self, from: Handle, point: Point) -> Option<DropTarget> {
        let (window, point) = window_at(from, point);
        (window != self.window
            && WINDOWS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .contains_key(&window))
        .then_some(DropTarget { window, point })
    }

    pub(super) fn receive_window_drops(&mut self) {
        // Synchronize with posting the message and queuing the payload.
        let incoming = {
            let _windows = WINDOWS.lock().unwrap_or_else(|error| error.into_inner());
            self.dock
                .incoming
                .as_ref()
                .into_iter()
                .flat_map(|inbox| inbox.try_iter())
                .collect::<Vec<_>>()
        };
        for item in incoming {
            if !matches!(item.transfer, Transfer::Terminal(_)) && self.other.is_some() {
                let (_, right, _) = self.dock.panes(self.px(6));
                if let Some(index) = self.group_document(usize::from(right.has(item.point))) {
                    self.activate(index, false);
                }
            }
            let result = self.receive_transfer(item.transfer);
            if let Some((window, handle)) = item.source {
                unsafe {
                    PostMessageW(
                        window,
                        WINDOW_RECEIPT,
                        handle as usize,
                        isize::from(result.is_ok()),
                    );
                }
            }
            if let Err(error) = result {
                self.show_error(error);
            } else {
                unsafe {
                    SetForegroundWindow(self.window);
                }
            }
            self.queue_session_save();
        }
    }

    pub(super) fn finish_window_move(&mut self, editor: Handle, accepted: bool) {
        let Some(revision) = self.dock.pending.remove(&editor) else {
            return;
        };
        if !accepted {
            return;
        }
        if let Some(index) = self.documents.iter().position(|doc| doc.editor == editor) {
            if self.documents[index].revision == revision {
                self.remove_document(index, false);
                self.close_empty_after_move();
            } else {
                self.report(crate::i18n::text("ui.edits.made.during.the.move.were.kept"));
            }
        }
    }

    pub(super) fn close_empty_after_move(&self) {
        if self.documents.is_empty() && self.terminal.is_empty() {
            unsafe {
                PostMessageW(self.window, WM_CLOSE, 0, 0);
            }
        }
    }

    pub(super) fn move_document_to_window(
        &mut self,
        index: usize,
        target: DropTarget,
    ) -> io::Result<()> {
        let editor = self.documents[index].editor;
        if self.dock.pending.contains_key(&editor) {
            return Ok(());
        }
        let transfer = self.document_transfer(index)?;
        target
            .send(transfer, Some((self.window, editor)))
            .map_err(|_| io::Error::other(crate::i18n::text("ui.could.not.move.the.tab.to.the")))?;
        self.dock
            .pending
            .insert(editor, self.documents[index].revision);
        Ok(())
    }

    pub(super) fn new_window_sender(&self) -> io::Result<mpsc::SyncSender<Transfer>> {
        let (payload, receiver) = mpsc::sync_channel(1);
        let (ready, started) = mpsc::sync_channel(1);
        let guard = WindowThread::new();
        let worker = thread::Builder::new()
            .name("maker-window".into())
            .spawn(move || {
                let _guard = guard;
                let result = (|| {
                    let _ole = Ole::init()?;
                    let window = main_window(WINDOW_CLASS)?;
                    let app = App::new(window);
                    let mut app = match app {
                        Ok(app) => app,
                        Err(error) => {
                            unsafe {
                                DestroyWindow(window);
                            }
                            return Err(error);
                        }
                    };
                    app.detached = true;
                    app.sidebar = false;
                    if ready.send(Ok(())).is_err() {
                        unsafe {
                            DestroyWindow(window);
                        }
                        return Ok(());
                    }
                    if let Ok(transfer) = receiver.recv() {
                        if let Err(error) = app.receive_transfer(transfer) {
                            app.show_error(error);
                        }
                        run_window(app, false)?;
                    } else {
                        unsafe {
                            DestroyWindow(window);
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    let _ = ready.try_send(Err(error));
                }
            })?;
        drop(worker);
        started
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| {
                io::Error::other(crate::i18n::text("ui.could.not.initialize.a.new.window"))
            })??;
        Ok(payload)
    }

    fn receive_transfer(&mut self, transfer: Transfer) -> io::Result<()> {
        match transfer {
            Transfer::Empty => {
                self.sidebar = true;
                Ok(())
            }
            Transfer::Path(path) => self.browse_path(&path),
            Transfer::Browser(base, folder, query) => {
                self.add_browser(base, folder)?;
                let view = self.documents[self.active.unwrap()]
                    .browser
                    .as_mut()
                    .unwrap();
                title(view.query, &query);
                let query_handle = view.query;
                self.browse_input(query_handle);
                self.layout();
                Ok(())
            }
            Transfer::Editor(editor) => {
                let TransferredEditor {
                    path,
                    label,
                    text,
                    encoding,
                    language,
                    dirty,
                    response,
                    response_url,
                    git_diff,
                    archive_member,
                    selection,
                    data_mode,
                } = *editor;
                let conflict = path.as_ref().and_then(|path| {
                    self.documents
                        .iter()
                        .position(|doc| doc.path.as_ref().is_some_and(|open| same_path(open, path)))
                });
                if !dirty && let Some(index) = conflict {
                    self.activate(index, true);
                    return Ok(());
                }
                let label = if conflict.is_some() {
                    crate::i18n::message("ui.moved.draft", std::slice::from_ref(&label))
                } else {
                    label
                };
                self.add_document(
                    if conflict.is_some() { None } else { path },
                    label,
                    &text,
                    encoding,
                )?;
                let index = self.active.unwrap();
                let doc = &mut self.documents[index];
                doc.language = language;
                doc.dirty = dirty;
                doc.response = response;
                doc.response_url = response_url;
                doc.git_diff = git_diff;
                doc.archive_member = archive_member;
                send(doc.editor, EM_SETREADONLY, usize::from(doc.is_preview()), 0);
                send(doc.editor, EM_SETMODIFY, usize::from(dirty), 0);
                select(doc.editor, selection.start, selection.end);
                send(doc.editor, EM_SCROLLCARET, 0, 0);
                self.format(index, true);
                self.update_status();
                if data_mode != data_view::ViewMode::Raw {
                    // The editor has already arrived even if creating the preview fails.
                    if let Err(error) = self.change_document_data_mode(index, data_mode) {
                        self.report(&error.to_string());
                    }
                }
                Ok(())
            }
            Transfer::Terminal(tab) => {
                self.accept_terminal(*tab);
                Ok(())
            }
        }
    }

    fn document_transfer(&self, index: usize) -> io::Result<Transfer> {
        let document = &self.documents[index];
        if document
            .browser
            .as_ref()
            .is_some_and(browser::Browser::loading)
        {
            return Err(io::Error::other(crate::i18n::text(
                "ui.wait.for.the.get.request.to.finish",
            )));
        }
        let payload = if let Some(view) = &document.browser {
            Transfer::Browser(
                view.base().to_owned(),
                view.folder().is_some(),
                caption(view.query),
            )
        } else {
            Transfer::Editor(Box::new(TransferredEditor {
                path: document.path.clone(),
                label: document.label.clone(),
                text: editor_text(document.editor),
                encoding: document.encoding,
                language: document.language,
                dirty: send(document.editor, EM_GETMODIFY, 0, 0) != 0,
                response: document.response,
                response_url: document.response_url.clone(),
                git_diff: document.git_diff.clone(),
                archive_member: document.archive_member.clone(),
                selection: selection(document.editor),
                data_mode: document.data_mode(),
            }))
        };
        Ok(payload)
    }

    pub(super) fn detach_document(&mut self, index: usize) -> io::Result<()> {
        if self
            .dock
            .pending
            .contains_key(&self.documents[index].editor)
        {
            return Ok(());
        }
        let payload = self.document_transfer(index)?;
        self.new_window_sender()?
            .send(payload)
            .map_err(|_| io::Error::other(crate::i18n::text("ui.the.new.window.closed")))?;
        self.remove_document(index, false);
        self.close_empty_after_move();
        Ok(())
    }

    pub(super) fn move_document(&mut self, from: usize, to: usize) {
        if from == to || from >= self.documents.len() || to >= self.documents.len() {
            return;
        }
        let adjust = |index| {
            if index == from {
                to
            } else if from < index && index <= to {
                index - 1
            } else if to <= index && index < from {
                index + 1
            } else {
                index
            }
        };
        let document = self.documents.remove(from);
        self.documents.insert(to, document);
        self.active = self.active.map(adjust);
        self.other = self.other.map(adjust);
        self.clear_tab_motion();
        self.tab_reveal(to);
        self.queue_session_save();
    }

    pub(super) fn split_document(
        &mut self,
        index: usize,
        target: Destination,
        previous: Option<usize>,
    ) {
        let neighbor = previous
            .filter(|&old| old != index)
            .or(self.other.filter(|&old| old != index))
            .or_else(|| (0..self.documents.len()).find(|&old| old != index));
        if neighbor.is_none() {
            self.activate(index, true);
            return;
        }
        self.dock.stacked = matches!(target, Destination::Top | Destination::Bottom);
        self.active = Some(index);
        self.other = neighbor;
        self.active_right = matches!(target, Destination::Right | Destination::Bottom);
        self.documents[index].group = usize::from(self.active_right);
        self.documents[neighbor.expect("split companion")].group = usize::from(!self.active_right);
        self.clear_tab_motion();
        self.activate(index, true);
    }

    pub(super) fn docking_at(
        &self,
        from: Handle,
        point: Point,
        folder: bool,
        window_only: bool,
    ) -> Destination {
        destination(
            self.dock.area,
            self.dock.sidebar,
            map_point(from, self.window, point),
            folder,
            window_only,
        )
    }

    pub(super) fn show_dock(&mut self, target: Destination) {
        if self.dock.preview == Some(target) {
            return;
        }
        self.dock.preview = Some(target);
        let area = if target == Destination::TerminalSidebar {
            self.dock.terminal_sidebar
        } else if target == Destination::Sidebar {
            self.dock.sidebar
        } else {
            self.dock.area
        };
        let [gap, width, height] = [self.px(10), area.width(), area.height()];
        if width < gap * 3 || height < gap * 3 {
            show(self.dock.guide, false);
            return;
        }
        let x = area.left + gap;
        let y = area.top + gap;
        let zone = match target {
            Destination::Left => Rect::from_size(x, y, width / 4, height - gap * 2),
            Destination::Right => {
                Rect::from_size(area.right - width / 4 - gap, y, width / 4, height - gap * 2)
            }
            Destination::Top => Rect::from_size(x, y, width - gap * 2, height / 4),
            Destination::Bottom | Destination::Sidebar | Destination::TerminalSidebar => {
                Rect::from_size(
                    x,
                    area.bottom - height / 3 - gap,
                    width - gap * 2,
                    height / 3,
                )
            }
            Destination::Window => {
                let button = width.min(self.px(200));
                Rect::from_size(
                    area.left + (width - button) / 2,
                    area.top + height / 2 - self.px(23),
                    button,
                    self.px(46),
                )
            }
        };
        if target == Destination::TerminalSidebar {
            self.report(i18n::text("terminal.sidebar_hint"));
            motion::animate(
                self.window,
                motion::Key::control(self.dock.guide, motion::Effect::Reveal),
                motion::Area::Control,
                false,
                true,
            );
        }
        show_overlay(self.dock.guide, zone);
        repaint(self.dock.guide);
    }

    pub(super) fn clear_dock(&mut self) {
        if self.dock.preview.take().is_some() {
            show(self.dock.guide, false);
        }
    }

    pub(super) fn paint_dock(&self, id: usize, dc: Handle, area: Rect, palette: Paint) {
        if matches!(id, SPLITTER | SIDEBAR_SPLITTER) {
            fill(dc, area, palette.chrome);
            let edge = if self.dock.stacked || id == SIDEBAR_SPLITTER {
                Rect {
                    top: area.bottom / 2,
                    bottom: area.bottom / 2 + 1,
                    ..area
                }
            } else {
                Rect {
                    left: area.right / 2,
                    right: area.right / 2 + 1,
                    ..area
                }
            };
            fill(dc, edge, palette.border);
        } else if let Some(target) = self.dock.preview {
            let border = if target == Destination::TerminalSidebar {
                let reveal = motion::value(
                    motion::Key::control(self.dock.guide, motion::Effect::Reveal),
                    true,
                );
                motion::mix(palette.border_rgb, palette.accent, reveal)
            } else {
                palette.accent
            };
            fill_color(dc, area, border);
            fill(
                dc,
                Rect {
                    left: 2,
                    top: 2,
                    right: area.right - 2,
                    bottom: area.bottom - 2,
                },
                palette.control,
            );
            let label = match target {
                Destination::Window => crate::i18n::text("ui.open.in.new.window"),
                Destination::Sidebar => crate::i18n::text("ui.open.in.secondary.explorer"),
                Destination::TerminalSidebar => crate::i18n::text("ui.open.terminal.in.sidebar"),
                Destination::Left => crate::i18n::text("ui.open.on.the.left"),
                Destination::Right => crate::i18n::text("ui.open.on.the.right"),
                Destination::Top => crate::i18n::text("ui.open.above"),
                Destination::Bottom => crate::i18n::text("ui.open.below"),
            };
            draw_label(dc, area, label, palette.font, palette.foreground, true);
        }
    }

    pub(super) fn splitter_mouse(&mut self, window: Handle, event: u32, point: Point) {
        let sidebar = window == self.dock.sidebar_splitter;
        match event {
            WM_LBUTTONDOWN => {
                self.dock.resizing = true;
                unsafe {
                    SetCapture(window);
                }
            }
            WM_MOUSEMOVE if self.dock.resizing => {
                let point = map_point(window, self.window, point);
                let area = if sidebar {
                    self.dock.sidebar_area
                } else {
                    self.dock.area
                };
                let ratio = if sidebar || self.dock.stacked {
                    (point.y - area.top) * 100 / area.height().max(1)
                } else {
                    (point.x - area.left) * 100 / area.width().max(1)
                };
                let ratio = if sidebar {
                    ratio.clamp(22, 78)
                } else {
                    ratio.clamp(18, 82)
                };
                let current = if sidebar {
                    &mut self.dock.sidebar_ratio
                } else {
                    &mut self.dock.ratio
                };
                if ratio != *current {
                    *current = ratio;
                    self.layout();
                }
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                self.dock.resizing = false;
                if event == WM_LBUTTONUP {
                    unsafe {
                        ReleaseCapture();
                    }
                }
                self.queue_session_save();
            }
            WM_LBUTTONDBLCLK => {
                if sidebar {
                    self.dock.sidebar_ratio = 50;
                } else {
                    self.dock.ratio = 50;
                }
                self.layout();
            }
            _ => {}
        }
    }

    pub(super) fn list_drag(
        &mut self,
        control: Handle,
        event: u32,
        wp: usize,
        packed: isize,
    ) -> bool {
        let point = Point::packed(packed);
        if event == WM_LBUTTONDOWN && wp & 0x0c == 0 {
            let Some(row) = list_at(control, packed) else {
                return false;
            };
            let path = if control == self.list && self.is_tree() {
                match self.list_items.get(row) {
                    Some(
                        ListItem::File(path, _)
                        | ListItem::Folder { path, .. }
                        | ListItem::Archive { path, .. },
                    ) => Some(path.clone()),
                    _ => None,
                }
            } else {
                self.browser_row_path(control, row)
            };
            let Some(path) = path else {
                return false;
            };
            self.dock.list_drag = Some(ListDrag {
                control,
                origin: point,
                folder: path.is_dir(),
                path,
                row,
            });
            chrome::select_file_row(control, row);
            if control == self.list {
                self.track_explorer_selection();
            } else {
                self.browser_focus(control);
            }
            unsafe {
                SetFocus(control);
                SetCapture(control);
            }
            return true;
        }
        let Some(drag) = self
            .dock
            .list_drag
            .as_ref()
            .filter(|drag| drag.control == control)
        else {
            return false;
        };
        let moved = (point.x - drag.origin.x)
            .abs()
            .max((point.y - drag.origin.y).abs())
            >= self.px(7);
        let target = self.docking_at(control, point, drag.folder, false);
        let outside = !bounds(control).has(point)
            || control == self.list
                && target == Destination::Sidebar
                && (point.y - drag.origin.y).abs() >= self.px(24);
        match event {
            WM_MOUSEMOVE => {
                if moved && outside && self.transfer_target(control, point).is_none() {
                    self.show_dock(target);
                } else {
                    self.clear_dock();
                }
            }
            WM_LBUTTONUP => {
                let drag = self.dock.list_drag.take().unwrap();
                self.clear_dock();
                unsafe {
                    ReleaseCapture();
                }
                if moved && outside {
                    let result = if let Some(window) = self.transfer_target(control, point) {
                        window.send(Transfer::Path(drag.path), None).map_err(|_| {
                            io::Error::other(crate::i18n::text(
                                "ui.could.not.open.in.the.target.window",
                            ))
                        })
                    } else {
                        self.drop_path(drag.path, target)
                    };
                    if let Err(error) = result {
                        self.show_error(error);
                    }
                } else if !moved && control == self.list {
                    chrome::post_explorer_click(self.window, control, drag.row, point);
                } else if !moved {
                    self.post_browser_click(control, drag.row, point, false);
                }
            }
            WM_CAPTURECHANGED => {
                self.dock.list_drag = None;
                self.clear_dock();
            }
            _ => return false,
        }
        true
    }

    fn drop_path(&mut self, path: PathBuf, target: Destination) -> io::Result<()> {
        match target {
            Destination::Sidebar => self.open_secondary_browser(path),
            Destination::Window => self
                .new_window_sender()?
                .send(Transfer::Path(path))
                .map_err(|_| io::Error::other(crate::i18n::text("ui.the.new.window.closed"))),
            side => {
                let previous = self.active;
                self.browse_path(&path)?;
                if let Some(active) = self.active {
                    self.split_document(active, side, previous);
                }
                Ok(())
            }
        }
    }
}
