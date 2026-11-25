use super::*;
use std::{
    io::Read,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

pub(super) const NEW_TAB: usize = 376;
pub(super) const QUERY_ID: usize = 377;
pub(super) const RESULTS: usize = 378;
pub(super) const SELECT: u32 = 0x8020;
pub(super) const HTTP_READY: u32 = 0x8021;
pub(super) const SEARCH_READY: u32 = 0x8022;
pub(super) const TIMER: usize = 11;
type Response = (String, u16);

pub(super) const FIELD_FRAME: usize = 388;
pub(super) const HERO: usize = 389;
pub(super) const WORK_FOLDER: usize = 390;
pub(super) const OPEN_FILE: usize = 391;
pub(super) const RECENT_FILE: usize = 392;

const MAX_ENTRIES: usize = 3000;
const MAX_RESPONSE: u64 = 4 * 1024 * 1024;

struct Row {
    path: PathBuf,
    depth: usize,
    directory: bool,
    open: bool,
}

pub(super) struct Browser {
    pub query: Handle,
    frame: Handle,
    hero: Handle,
    actions: [Handle; 3],
    input_height: Cell<i32>,
    pub list: Handle,
    base: PathBuf,
    folder: bool,
    expanded: HashSet<PathBuf>,
    rows: Vec<Row>,
    request: Option<(String, mpsc::Receiver<io::Result<Response>>)>,
    search_due: bool,
    search: Option<mpsc::Receiver<Vec<Row>>>,
    open_on_match: Option<bool>,
}

impl Browser {
    pub fn new(parent: Handle, base: PathBuf, folder: bool) -> io::Result<Self> {
        let mut controls = Vec::new();
        let mut make = |id, class: &str, label: &str, style| {
            let handle = child(parent, id, class, label, style)?;
            controls.push(handle);
            Ok::<Handle, io::Error>(handle)
        };
        let result = (|| {
            let frame = make(FIELD_FRAME, "STATIC", "", 0x000d)?;
            let hero = if folder {
                0
            } else {
                make(HERO, "STATIC", "", 0x000d)?
            };
            let mut actions = [0; 3];
            if !folder {
                for (index, (id, label)) in [
                    (WORK_FOLDER, "작업 폴더"),
                    (OPEN_FILE, "파일 열기"),
                    (RECENT_FILE, "최근 닫은 파일"),
                ]
                .into_iter()
                .enumerate()
                {
                    actions[index] = make(id, "BUTTON", label, 0x000b | WS_TABSTOP)?;
                    chrome::watch_control(actions[index], true);
                }
            }
            let query = make(QUERY_ID, "EDIT", "", WS_TABSTOP | ES_AUTOHSCROLL)?;
            let list = make(
                RESULTS,
                "LISTBOX",
                "",
                LBS_NOTIFY
                    | LBS_EXTENDEDSEL
                    | LBS_OWNERDRAWFIXED
                    | LBS_HASSTRINGS
                    | LBS_NOINTEGRALHEIGHT
                    | WS_TABSTOP
                    | WS_VSCROLL,
            )?;
            chrome::watch_control(list, false);
            send(query, 0x00c5, 32767, 0);
            Ok::<Self, io::Error>(Self {
                query,
                frame,
                hero,
                actions,
                input_height: Cell::new(20),
                list,
                base,
                folder,
                expanded: HashSet::new(),
                rows: Vec::new(),
                request: None,
                search_due: false,
                search: None,
                open_on_match: None,
            })
        })();
        let mut result = match result {
            Ok(result) => result,
            Err(error) => {
                for control in controls {
                    unsafe {
                        DestroyWindow(control);
                    }
                }
                return Err(error);
            }
        };
        result.apply_font();
        result.reload_tree();
        Ok(result)
    }

    pub fn apply_font(&self) {
        if let Some(palette) = PAINT.get() {
            set_font(self.query, palette.font);
            self.input_height
                .set(cell_size(self.query, palette.font).y + 2);
            for button in self.actions.into_iter().filter(|&button| button != 0) {
                set_font(button, palette.font);
            }
            set_font(self.list, palette.font);
            send(
                self.list,
                LB_SETITEMHEIGHT,
                0,
                (24 * palette.dpi / 96) as isize,
            );
        }
        let hint = if self.folder {
            format!("{}에서 찾기 · 경로 또는 URL", file_name(&self.base))
        } else {
            "파일·폴더 이름, 경로 또는 URL 검색".to_owned()
        };
        edit_hint(self.query, &hint);
    }

    pub fn keyboard_controls(&self) -> impl Iterator<Item = Handle> {
        [self.query, self.list]
            .into_iter()
            .chain(self.actions)
            .filter(|handle| *handle != 0)
    }

    pub fn folder(&self) -> Option<&Path> {
        self.folder.then_some(self.base.as_path())
    }

    pub fn loading(&self) -> bool {
        self.request.is_some()
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn layout(&self, area: Option<Rect>, dpi: i32) {
        let px = |size: i32| size * dpi / 96;
        let Some(area) = area.filter(|area| area.width() > px(70) && area.height() >= px(42))
        else {
            for control in [self.query, self.list, self.frame, self.hero]
                .into_iter()
                .chain(self.actions)
            {
                if control != 0 {
                    show(control, false);
                }
            }
            return;
        };
        let width = (area.width() - px(24)).min(px(if self.folder { 1600 } else { 592 }));
        let x = area.left + (area.width() - width) / 2;
        let hero = !self.folder && area.height() >= px(195);
        let y = area.top
            + if self.folder {
                px(8)
            } else if hero {
                (area.height() / 3).clamp(px(83), px(185))
            } else {
                px(12)
            };
        let height = px(if self.folder { 32 } else { 44 });
        let input_height = self.input_height.get().min(height - px(6));
        place(self.frame, x, y, width, height);
        show(self.frame, true);
        let query = Rect::from_size(
            x + px(38),
            y + (height - input_height) / 2,
            width - px(50),
            input_height,
        );
        let visible = unsafe { IsWindowVisible(self.query) } != 0;
        if !visible || relative_rect(self.query, unsafe { GetParent(self.query) }) != query {
            show_overlay(self.query, query);
        }
        if self.hero != 0 {
            show(self.hero, hero);
            if hero {
                place(self.hero, x, y - px(77), width, px(66));
            }
        }
        let actions = hero && width >= px(302) && caption(self.query).is_empty();
        let button_width = width.min(px(390)) / 3;
        let left = x + (width - button_width * 3) / 2;
        for (i, control) in self
            .actions
            .into_iter()
            .filter(|&control| control != 0)
            .enumerate()
        {
            if actions {
                place(
                    control,
                    left + i as i32 * button_width + px(4),
                    y + height + px(13),
                    button_width - px(8),
                    px(30),
                );
            }
            show(control, actions);
        }
        let bottom = (area.bottom - y - height - px(10)).max(0);
        let results_height = if self.folder {
            bottom
        } else {
            bottom.min(self.rows.len().min(9) as i32 * px(24) + 4)
        };
        place(self.list, x, y + height + px(5), width, results_height);
        show(
            self.list,
            results_height > 0 && (self.folder || !self.rows.is_empty()),
        );
    }

    fn reload_tree(&mut self) {
        let mut rows = Vec::new();
        if self.folder {
            self.tree(&self.base, 0, &mut rows);
        }
        self.set_rows(rows, false);
    }

    fn set_rows(&mut self, rows: Vec<Row>, search: bool) {
        chrome::reset_file_motion(self.list);
        let visible = unsafe { IsWindowVisible(self.list) } != 0;
        if visible {
            painting(self.list, false);
        }
        send(self.list, LB_RESETCONTENT, 0, 0);
        for row in &rows {
            let icon = if row.directory {
                chrome::folder_icon(&row.path, row.open)
            } else {
                chrome::file_icon(Some(&row.path))
            };
            let label = if search {
                row.path
                    .strip_prefix(&self.base)
                    .ok()
                    .filter(|name| !name.as_os_str().is_empty())
                    .map_or_else(
                        || file_name(&row.path),
                        |name| name.as_os_str().to_string_lossy(),
                    )
            } else {
                file_name(&row.path)
            };
            list_entry(self.list, &label, (row.depth << 8) | icon);
        }
        if search && !rows.is_empty() {
            list_select(self.list, 0);
        }
        self.rows = rows;
        if visible {
            painting(self.list, true);
        }
        repaint(self.list);
    }

    fn tree(&self, folder: &Path, depth: usize, rows: &mut Vec<Row>) {
        for (path, directory) in children(folder) {
            if rows.len() >= MAX_ENTRIES {
                break;
            }
            let open = directory && depth < 24 && self.expanded.contains(&path);
            rows.push(Row {
                path: path.clone(),
                depth,
                directory,
                open,
            });
            if open {
                self.tree(&path, depth + 1, rows);
            }
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        for control in [self.query, self.list, self.frame, self.hero]
            .into_iter()
            .chain(self.actions)
        {
            if control != 0 {
                unsafe {
                    DestroyWindow(control);
                }
            }
        }
    }
}

fn children(folder: &Path) -> Vec<(PathBuf, bool)> {
    let mut entries: Vec<_> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .take(MAX_ENTRIES)
        .filter_map(|entry| {
            let kind = entry.file_type().ok()?;
            let path = entry.path();
            let directory = kind.is_dir() || kind.is_symlink() && path.is_dir();
            (directory || kind.is_file() || kind.is_symlink() && path.is_file())
                .then_some((path, directory))
        })
        .collect();
    entries.sort_by_cached_key(|(path, dir)| (!dir, file_name(path).to_lowercase()));
    entries
}

fn is_url(text: &str) -> bool {
    text.get(..7)
        .is_some_and(|start| start.eq_ignore_ascii_case("http://"))
        || text
            .get(..8)
            .is_some_and(|start| start.eq_ignore_ascii_case("https://"))
}

fn target_path(base: &Path, text: &str) -> io::Result<PathBuf> {
    let path = Path::new(text);
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    if path.has_root()
        || matches!(
            path.components().next(),
            Some(std::path::Component::Prefix(_))
        )
    {
        return Err(io::Error::other(
            "드라이브 문자를 포함한 전체 경로를 입력하세요.",
        ));
    }
    Ok(base.join(path))
}

fn search_paths(base: &Path, query: &str) -> Vec<Row> {
    let Ok(path) = target_path(base, query) else {
        return Vec::new();
    };
    if let Ok(info) = fs::metadata(&path)
        && (info.is_dir() || info.is_file())
    {
        return vec![Row {
            path,
            depth: 0,
            directory: info.is_dir(),
            open: false,
        }];
    }
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let Some(name) = path.file_name() else {
        return Vec::new();
    };
    let name = name.to_string_lossy().to_lowercase();
    let recursive = parent == base && !Path::new(query).has_root();
    let mut pending = std::collections::VecDeque::from([parent.to_owned()]);
    let mut matches = Vec::new();
    let mut visited = 0;
    while let Some(directory) = pending.pop_front() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_ENTRIES || matches.len() == 256 {
                return rank_matches(matches, &name);
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            let directory = kind.is_dir() || kind.is_symlink() && path.is_dir();
            if directory && recursive && !kind.is_symlink() && entry.file_name() != ".git" {
                pending.push_back(path.clone());
            }
            if (directory || kind.is_file() || kind.is_symlink() && path.is_file())
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&name)
            {
                matches.push(Row {
                    path,
                    depth: 0,
                    directory,
                    open: false,
                });
            }
        }
    }
    rank_matches(matches, &name)
}

fn rank_matches(mut matches: Vec<Row>, name: &str) -> Vec<Row> {
    matches.sort_by_cached_key(|row| {
        let filename = file_name(&row.path).to_lowercase();
        (
            filename != name,
            !filename.starts_with(name),
            row.path.components().count(),
            filename,
            row.path.clone(),
        )
    });
    matches
}

impl App {
    pub(super) fn new_browser(&mut self) -> io::Result<()> {
        let base = self
            .active
            .and_then(|index| {
                let document = &self.documents[index];
                document
                    .browser
                    .as_ref()
                    .map(|view| view.base().to_owned())
                    .or_else(|| {
                        document
                            .path
                            .as_deref()
                            .and_then(Path::parent)
                            .map(Path::to_owned)
                    })
            })
            .or_else(|| self.root.clone())
            .map_or_else(std::env::current_dir, Ok)?;
        self.add_browser(base, false)
    }

    pub(super) fn add_browser(&mut self, base: PathBuf, folder: bool) -> io::Result<()> {
        let browser = Browser::new(self.window, base, folder)?;
        let label = if folder {
            file_name(browser.base()).into_owned()
        } else {
            "새 탭".to_owned()
        };
        self.add_document(None, label, "", Encoding::default())?;
        let index = self.active.expect("new browser document");
        self.documents[index].browser = Some(browser);
        self.layout();
        self.focus_editor();
        self.update_status();
        Ok(())
    }

    pub(super) fn browse_path(&mut self, path: &Path) -> io::Result<()> {
        if path.is_dir() {
            self.add_browser(path.to_owned(), true)
        } else {
            self.open_path(path)
        }
    }

    fn browser_for(&self, window: Handle) -> Option<&Browser> {
        self.documents
            .iter()
            .filter_map(|doc| doc.browser.as_ref())
            .chain(self.secondary_browser.iter())
            .find(|view| view.query == window || view.list == window)
    }

    fn browser_for_mut(&mut self, window: Handle) -> Option<&mut Browser> {
        self.documents
            .iter_mut()
            .filter_map(|doc| doc.browser.as_mut())
            .chain(self.secondary_browser.iter_mut())
            .find(|view| view.query == window || view.list == window)
    }

    pub(super) fn paint_browser(&self, item: &DrawItem, palette: Paint) {
        let Some(view) = self
            .documents
            .iter()
            .filter_map(|doc| doc.browser.as_ref())
            .chain(self.secondary_browser.iter())
            .find(|view| view.frame == item.window || view.hero == item.window)
        else {
            return;
        };
        let sidebar = self
            .secondary_browser
            .as_ref()
            .is_some_and(|view| view.frame == item.window);
        let rect = item.rect;
        fill(
            item.dc,
            rect,
            if sidebar {
                palette.panel
            } else {
                palette.background
            },
        );
        if view.hero == item.window {
            draw_label(
                item.dc,
                Rect {
                    bottom: rect.top + self.px(37),
                    ..rect
                },
                "파일, 폴더 및 URL 열기",
                palette.large_font,
                palette.foreground,
                true,
            );
            let base = format!("검색 위치   {}", view.base.display());
            draw_label(
                item.dc,
                Rect {
                    top: rect.top + self.px(39),
                    ..rect
                },
                &base,
                palette.font,
                palette.muted,
                true,
            );
        } else {
            let focused = unsafe { GetFocus() } == view.query;
            fill_rounded(
                item.dc,
                rect,
                self.px(7),
                if focused {
                    palette.accent
                } else {
                    palette.border_rgb
                },
            );
            fill_rounded(
                item.dc,
                Rect {
                    left: rect.left + 1,
                    top: rect.top + 1,
                    right: rect.right - 1,
                    bottom: rect.bottom - 1,
                },
                self.px(6),
                palette.control_rgb,
            );
            let cy = rect.height() / 2;
            let icon = Rect::from_size(self.px(12), cy - self.px(7), self.px(12), self.px(12));
            fill_rounded(item.dc, icon, self.px(6), palette.muted);
            fill_rounded(
                item.dc,
                Rect {
                    left: icon.left + 1,
                    top: icon.top + 1,
                    right: icon.right - 1,
                    bottom: icon.bottom - 1,
                },
                self.px(5),
                palette.control_rgb,
            );
            stroke_line(
                item.dc,
                Point {
                    x: icon.right - self.px(2),
                    y: icon.bottom - self.px(2),
                },
                Point {
                    x: icon.right + self.px(4),
                    y: icon.bottom + self.px(4),
                },
                palette.muted,
            );
        }
    }

    pub(super) fn browser_focus_outline(&self, window: Handle) {
        if let Some(view) = self.browser_for(window) {
            repaint(view.frame);
        }
    }

    pub(super) fn browser_action(&mut self, id: usize, sender: Handle) -> io::Result<()> {
        let Some(index) = self.documents.iter().position(|doc| {
            doc.browser
                .as_ref()
                .is_some_and(|view| view.actions.contains(&sender))
        }) else {
            return Ok(());
        };
        self.activate(index, false);
        let view = self.documents[index].browser.as_ref().unwrap();
        match id {
            WORK_FOLDER => {
                let query = view.query;
                let path = self
                    .root
                    .as_deref()
                    .unwrap_or_else(|| view.base())
                    .to_owned();
                self.follow_browser_path(query, path, false)
            }
            OPEN_FILE => self.perform(OPEN),
            _ => {
                if self.recently_closed.is_empty() {
                    self.report("최근에 닫은 파일이 없습니다.");
                } else {
                    self.perform(REOPEN)?;
                }
                Ok(())
            }
        }
    }

    pub(super) fn browser_focus(&mut self, window: Handle) {
        if let Some(index) = self.documents.iter().position(|doc| {
            doc.browser
                .as_ref()
                .is_some_and(|view| view.query == window || view.list == window)
        }) && self.active != Some(index)
        {
            self.activate(index, false);
        }
    }

    pub(super) fn browser_list_base(&self, control: Handle) -> Option<&Path> {
        self.browser_for(control)
            .filter(|view| view.list == control && view.folder)
            .map(Browser::base)
    }

    pub(super) fn reload_open_folders(&mut self) {
        for view in self
            .documents
            .iter_mut()
            .filter_map(|doc| doc.browser.as_mut())
            .chain(self.secondary_browser.iter_mut())
        {
            if view.folder && caption(view.query).is_empty() {
                view.reload_tree();
            }
        }
        self.layout_browsers();
    }

    pub(super) fn browser_row_path(&self, window: Handle, row: usize) -> Option<PathBuf> {
        self.browser_for(window)?
            .rows
            .get(row)
            .map(|item| item.path.clone())
    }

    pub(super) fn post_browser_click(
        &self,
        window: Handle,
        row: usize,
        point: Point,
        new_tab: bool,
    ) {
        let Some(item) = self.browser_for(window).and_then(|view| view.rows.get(row)) else {
            return;
        };
        let arrow = point.x < self.px(20) + (item.depth as i32) * self.px(14);
        let flags = (row as isize) << 2 | isize::from(arrow) | (isize::from(new_tab) << 1);
        unsafe {
            PostMessageW(self.window, SELECT, window as usize, flags);
        }
    }

    pub(super) fn browse_input(&mut self, window: Handle) {
        let Some(view) = self.browser_for_mut(window) else {
            return;
        };
        let input = caption(view.query);
        let query = input.trim().trim_matches('"');
        view.open_on_match = None;
        view.search = None;
        if view.request.as_ref().is_some_and(|(url, _)| url != query) {
            view.request = None;
        }
        view.search_due = !query.is_empty() && !is_url(query);
        if query.is_empty() {
            view.reload_tree();
        } else if !view.rows.is_empty() {
            view.set_rows(Vec::new(), false);
        }
        if view.search_due {
            unsafe {
                SetTimer(self.window, TIMER, 125, 0);
            }
        }
        self.layout_browsers();
    }

    pub(super) fn start_browser_searches(&mut self) {
        let window = self.window;
        unsafe {
            KillTimer(window, TIMER);
        }
        let views = self
            .documents
            .iter_mut()
            .filter_map(|doc| doc.browser.as_mut())
            .chain(self.secondary_browser.iter_mut());
        for view in views {
            if !std::mem::take(&mut view.search_due) {
                continue;
            }
            let query = view.query;
            let input = caption(query);
            let base = view.base.clone();
            let (ready, results) = mpsc::channel();
            match thread::Builder::new()
                .name("maker-path-search".into())
                .spawn(move || {
                    let matches = search_paths(&base, input.trim().trim_matches('"'));
                    if ready.send(matches).is_ok() {
                        unsafe {
                            PostMessageW(window, SEARCH_READY, query as usize, 0);
                        }
                    }
                }) {
                Ok(_) => view.search = Some(results),
                Err(_) => view.open_on_match = None,
            }
        }
    }

    pub(super) fn complete_browser_search(&mut self, window: Handle) {
        let Some(view) = self.browser_for_mut(window) else {
            return;
        };
        let Some(rows) = view
            .search
            .as_ref()
            .and_then(|result| result.try_recv().ok())
        else {
            return;
        };
        view.search = None;
        let open = view.open_on_match.take();
        view.set_rows(rows, true);
        self.layout_browsers();
        if let Some(new_tab) = open
            && let Err(error) = self.submit_browser(window, new_tab)
        {
            self.report(&error.to_string());
        }
    }

    pub(super) fn layout_browsers(&self) {
        for (index, doc) in self.documents.iter().enumerate() {
            if let Some(view) = &doc.browser {
                let visible = self.group_document(doc.group) == Some(index);
                view.layout(visible.then(|| self.pane_content(doc.group)), self.dpi);
            }
        }
        if let Some(view) = &self.secondary_browser {
            let area = self.dock.sidebar;
            view.layout((area.width() > 0).then_some(area), self.dpi);
        }
    }

    pub(super) fn browser_key(&mut self, window: Handle, key: i32) -> bool {
        let Some(view) = self.browser_for(window) else {
            return false;
        };
        match key {
            13 => {
                let new_tab = unsafe { GetKeyState(0x11) < 0 };
                let result = if window == view.query {
                    self.submit_browser(window, new_tab)
                } else if let Some(row) = list_caret(window) {
                    self.browse_row(window, row, new_tab, false)
                } else {
                    Ok(())
                };
                if let Err(error) = result {
                    self.show_error(error);
                }
            }
            0x28 if window == view.query && !view.rows.is_empty() => {
                let list = view.list;
                list_select(list, 0);
                unsafe {
                    SetFocus(list);
                }
            }
            0x25 | 0x27 if window == view.list => {
                if let Some(row) = list_caret(window).filter(|&row| {
                    view.rows
                        .get(row)
                        .is_some_and(|row| row.directory && row.open == (key == 0x25))
                }) && let Err(error) = self.browse_row(window, row, false, true)
                {
                    self.show_error(error);
                }
            }
            _ => return false,
        }
        true
    }

    fn submit_browser(&mut self, window: Handle, new_tab: bool) -> io::Result<()> {
        let query = caption(window);
        let query = query.trim().trim_matches('"');
        if query.is_empty() {
            return Ok(());
        }
        if is_url(query) {
            let owner = if new_tab
                || self
                    .secondary_browser
                    .as_ref()
                    .is_some_and(|view| view.query == window)
            {
                self.new_browser()?;
                let view = self.documents[self.active.unwrap()]
                    .browser
                    .as_ref()
                    .unwrap();
                title(view.query, query);
                view.query
            } else {
                window
            };
            return self.start_http(owner, query.to_owned());
        }
        let view = self.browser_for_mut(window).expect("active browser");
        if view.search_due || view.search.is_some() {
            view.open_on_match = Some(new_tab);
            return Ok(());
        }
        if let Some(row) = view.rows.first() {
            let path = row.path.clone();
            self.follow_browser_path(window, path, new_tab)
        } else {
            Err(io::Error::other("해당 파일이나 폴더를 찾을 수 없습니다."))
        }
    }

    pub(super) fn browse_row(
        &mut self,
        window: Handle,
        row: usize,
        new_tab: bool,
        arrow: bool,
    ) -> io::Result<()> {
        let Some(view) = self.browser_for_mut(window) else {
            return Ok(());
        };
        let Some(item) = view.rows.get(row) else {
            return Ok(());
        };
        let path = item.path.clone();
        if arrow && item.directory && view.folder && caption(view.query).is_empty() && !new_tab {
            if !view.expanded.remove(&path) {
                view.expanded.insert(path);
            }
            let top = send(window, LB_GETTOPINDEX, 0, 0).max(0) as usize;
            view.reload_tree();
            list_select(window, row);
            send(window, LB_SETTOPINDEX, top, 0);
            return Ok(());
        }
        self.follow_browser_path(window, path, new_tab)
    }

    fn follow_browser_path(
        &mut self,
        window: Handle,
        path: PathBuf,
        new_tab: bool,
    ) -> io::Result<()> {
        if file_actions::executable(&path) {
            self.report("실행 파일을 두 번 클릭하면 실행됩니다.");
            return Ok(());
        }
        if !new_tab {
            if self
                .secondary_browser
                .as_ref()
                .is_some_and(|view| view.query == window || view.list == window)
                && path.is_dir()
            {
                return self.open_secondary_browser(path);
            }
            if let Some(index) = self.documents.iter().position(|doc| {
                doc.browser
                    .as_ref()
                    .is_some_and(|view| view.query == window || view.list == window)
            }) {
                let previous_count = self.documents.len();
                if path.is_dir() {
                    self.documents[index].browser =
                        Some(Browser::new(self.window, path.clone(), true)?);
                    self.documents[index].label = file_name(&path).into_owned();
                    self.activate(index, true);
                    return Ok(());
                }
                self.open_path(&path)?;
                if self.active != Some(index) {
                    let more = self.documents.len() > previous_count;
                    self.remove_document(index, false);
                    if more {
                        self.move_document(self.documents.len() - 1, index);
                    }
                }
                return Ok(());
            }
        }
        self.browse_path(&path)
    }

    pub(super) fn open_secondary_browser(&mut self, folder: PathBuf) -> io::Result<()> {
        self.secondary_browser = Some(Browser::new(self.window, folder, true)?);
        self.sidebar = true;
        self.layout();
        self.queue_session_save();
        Ok(())
    }

    fn start_http(&mut self, query: Handle, url: String) -> io::Result<()> {
        let window = self.window;
        let result = self.browser_for_mut(query).expect("HTTP browser");
        if result.loading() {
            return Ok(());
        }
        let (sender, receiver) = mpsc::channel();
        let request_url = url.clone();
        thread::Builder::new()
            .name("maker-http".into())
            .spawn(move || {
                let _ = sender.send(get(&request_url));
                unsafe {
                    PostMessageW(window, HTTP_READY, query as usize, 0);
                }
            })?;
        result.request = Some((url, receiver));
        self.report("GET 요청 중…");
        Ok(())
    }

    pub(super) fn complete_http(&mut self, query: Handle) {
        let Some(index) = self
            .documents
            .iter()
            .position(|doc| doc.browser.as_ref().is_some_and(|view| view.query == query))
        else {
            return;
        };
        let view = self.documents[index].browser.as_mut().unwrap();
        let Some((_, receiver)) = &view.request else {
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        let (url, _) = view.request.take().unwrap();
        match result {
            Ok((body, status)) => {
                let doc = &mut self.documents[index];
                doc.label = url.chars().take(100).collect();
                doc.browser = None;
                doc.response = true;
                doc.language = if body.trim_start().starts_with(['{', '[']) {
                    Language::Json
                } else {
                    Language::Plain
                };
                set_editor_text(doc.editor, &body);
                send(doc.editor, EM_SETREADONLY, 1, 0);
                send(doc.editor, EM_SETMODIFY, 0, 0);
                self.format(index, true);
                self.layout();
                if self.active == Some(index) {
                    self.focus_editor();
                    self.update_status();
                }
                self.report(&format!("HTTP {status}"));
            }
            Err(error) => self.report(&error.to_string()),
        }
    }
}

fn get(url: &str) -> io::Result<Response> {
    let curl = system_program("curl.exe")?;
    let mut process = Command::new(curl)
        .args([
            "--disable",
            "--silent",
            "--globoff",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--location",
            "--max-redirs",
            "5",
            "--connect-timeout",
            "6",
            "--max-time",
            "15",
            "--max-filesize",
            "4194304",
            "--write-out",
            "\n%{http_code}",
            "--url",
            url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()?;
    let mut bytes = Vec::new();
    let result = process
        .stdout
        .take()
        .expect("curl stdout")
        .take(MAX_RESPONSE + 5)
        .read_to_end(&mut bytes);
    if result.is_err() || bytes.len() as u64 > MAX_RESPONSE + 4 {
        let _ = process.kill();
    }
    let status = process.wait()?;
    result?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "GET 요청을 완료하지 못했습니다. (curl {})",
            status.code().unwrap_or(-1)
        )));
    }
    let end = bytes
        .iter()
        .rposition(|&byte| byte == b'\n')
        .ok_or_else(|| io::Error::other("HTTP 상태 줄이 없습니다."))?;
    let code = std::str::from_utf8(&bytes[end + 1..])
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::other("HTTP 상태를 읽을 수 없습니다."))?;
    let (text, _) = text::decode(&bytes[..end])
        .map_err(|_| io::Error::other("응답을 UTF-8 텍스트로 열 수 없습니다."))?;
    Ok((text, code))
}
