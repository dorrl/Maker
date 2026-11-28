mod markup;
mod model;

use super::*;
use model::Model;

pub(super) const FIRST_MODE: usize = 396;
pub(super) const LAST_MODE: usize = FIRST_MODE + 5;
pub(super) const TREE: usize = 402;
pub(super) const PRETTY_EDITOR: usize = 403;
pub(super) const EXPAND_ALL: usize = 404;
pub(super) const COLLAPSE_ALL: usize = 405;
const NOTICE: usize = 406;
pub(super) const OPEN_WEB_FIRST: usize = 407;
pub(super) const OPEN_WEB_LAST: usize = 408;
pub(super) const READY: u32 = 0x8027;
pub(super) const TOGGLE: u32 = 0x8028;

const LB_SETCOUNT: u32 = 0x01a7;
const LB_GETCURSEL: u32 = 0x0188;
const LB_SETCURSEL: u32 = 0x0186;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ViewMode {
    #[default]
    Raw,
    Pretty,
    Tree,
}

const MODES: [ViewMode; 3] = [ViewMode::Raw, ViewMode::Pretty, ViewMode::Tree];

pub(super) fn supports(language: Language, path: Option<&Path>) -> bool {
    matches!(language, Language::Yaml | Language::Html | Language::Xml)
        || language == Language::Json
            && !path
                .and_then(Path::extension)
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonc"))
}

pub(super) fn pane_buttons(parent: Handle) -> io::Result<[[Handle; 4]; 2]> {
    let mut groups = [[0; 4]; 2];
    for (group, buttons) in groups.iter_mut().enumerate() {
        for (index, text) in [
            crate::i18n::text("ui.source"),
            i18n::text("data.pretty"),
            crate::i18n::text("ui.tree"),
        ]
        .into_iter()
        .enumerate()
        {
            let handle = child(
                parent,
                FIRST_MODE + group * 3 + index,
                "BUTTON",
                text,
                BS_OWNERDRAW | WS_TABSTOP,
            )?;
            chrome::watch_control(handle, true);
            buttons[index] = handle;
        }
    }
    for (group, buttons) in groups.iter_mut().enumerate() {
        buttons[3] = child(
            parent,
            OPEN_WEB_FIRST + group,
            "BUTTON",
            crate::i18n::text("ui.open.in.browser"),
            BS_OWNERDRAW | WS_TABSTOP,
        )?;
        chrome::watch_control(buttons[3], true);
    }
    Ok(groups)
}

pub(super) struct View {
    mode: ViewMode,
    list: Handle,
    pretty: Handle,
    notice: Handle,
    all: [Handle; 2],
    pending: Option<mpsc::Receiver<io::Result<Model>>>,
    data: Option<Model>,
    preview_ready: bool,
    open: Vec<bool>,
    rows: Vec<usize>,
}

impl View {
    fn new(parent: Handle, settings: &Settings) -> io::Result<Self> {
        let mut controls = Vec::new();
        let mut create = |id, class: &str, text: &str, style| {
            let control = child(parent, id, class, text, style)?;
            controls.push(control);
            Ok::<_, io::Error>(control)
        };
        let result = (|| {
            let list = create(
                TREE,
                "LISTBOX",
                "",
                LBS_NOTIFY
                    | LBS_OWNERDRAWFIXED
                    | LBS_NOINTEGRALHEIGHT
                    | 0x2000
                    | WS_TABSTOP
                    | WS_VSCROLL,
            )?;
            chrome::watch_control(list, false);
            let pretty = create(
                PRETTY_EDITOR,
                "RICHEDIT50W",
                "",
                WS_TABSTOP
                    | WS_HSCROLL
                    | WS_VSCROLL
                    | ES_MULTILINE
                    | ES_NOHIDESEL
                    | ES_AUTOHSCROLL
                    | ES_AUTOVSCROLL
                    | ES_DISABLENOSCROLL,
            )?;
            chrome::watch_control(pretty, false);
            send(pretty, EM_EXLIMITTEXT, 0, 32 * 1024 * 1024);
            send(pretty, EM_SETREADONLY, 1, 0);
            send(pretty, EM_SETEVENTMASK, 0, ENM_SCROLL);
            let notice = create(NOTICE, "STATIC", "", SS_NOPREFIX | SS_CENTERIMAGE)?;
            let expand = create(
                EXPAND_ALL,
                "BUTTON",
                crate::i18n::text("ui.expand.all"),
                BS_OWNERDRAW | WS_TABSTOP,
            )?;
            let collapse = create(
                COLLAPSE_ALL,
                "BUTTON",
                crate::i18n::text("ui.collapse.all"),
                BS_OWNERDRAW | WS_TABSTOP,
            )?;
            for button in [expand, collapse] {
                chrome::watch_control(button, true);
            }
            Ok::<Self, io::Error>(Self {
                mode: ViewMode::Raw,
                list,
                pretty,
                notice,
                all: [expand, collapse],
                pending: None,
                data: None,
                preview_ready: false,
                open: Vec::new(),
                rows: Vec::new(),
            })
        })();
        match result {
            Ok(view) => {
                view.apply_style(settings);
                Ok(view)
            }
            Err(error) => {
                for control in controls {
                    unsafe {
                        DestroyWindow(control);
                    }
                }
                Err(error)
            }
        }
    }

    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    pub fn apply_style(&self, settings: &Settings) {
        let Some(palette) = PAINT.get() else {
            return;
        };
        send(
            self.list,
            LB_SETITEMHEIGHT,
            0,
            (24 * palette.dpi / 96) as isize,
        );
        for handle in [self.list, self.notice].into_iter().chain(self.all) {
            set_font(handle, palette.font);
            repaint(handle);
        }
        for (button, key) in self
            .all
            .into_iter()
            .zip(["ui.expand.all", "ui.collapse.all"])
        {
            title(button, i18n::text(key));
        }
        send(
            self.pretty,
            EM_SETBKGNDCOLOR,
            0,
            palette.editor_rgb as isize,
        );
        send(self.pretty, EM_SETTARGETDEVICE, 0, 1);
        let font = Some((settings.font.as_str(), settings.points));
        char_format(self.pretty, 4, palette.foreground, None, font);
        char_format(self.pretty, 0, palette.foreground, None, font);
        repaint(self.pretty);
    }

    pub fn layout(&self, area: Option<Rect>, dpi: i32) {
        let active = area.is_some() && self.mode != ViewMode::Raw;
        let tree = active && self.mode == ViewMode::Tree && self.data.is_some();
        let pretty = active && self.pretty_editor().is_some();
        let message = active && !tree && !pretty;
        if let Some(area) = area.filter(|_| active) {
            let px = |n: i32| n * dpi / 96;
            if tree {
                let toolbar = px(34).min(area.height());
                let width = px(92).min((area.width() - px(18)).max(0) / 2);
                for (index, button) in self.all.into_iter().enumerate() {
                    place(
                        button,
                        area.left + px(6) + index as i32 * (width + px(6)),
                        area.top + px(4),
                        width,
                        toolbar - px(8),
                    );
                }
                place(
                    self.list,
                    area.left,
                    area.top + toolbar,
                    area.width(),
                    area.height() - toolbar,
                );
            } else if pretty {
                place(
                    self.pretty,
                    area.left + px(8),
                    area.top + px(5),
                    area.width() - px(8),
                    area.height() - px(5),
                );
            } else {
                place(
                    self.notice,
                    area.left + px(12),
                    area.top + px(3),
                    area.width() - px(18),
                    px(34).min(area.height()),
                );
            }
        }
        show(self.list, tree);
        show(self.pretty, pretty);
        show(self.notice, message);
        for button in self.all {
            show(button, tree);
        }
    }

    pub fn focus(&self) -> Option<Handle> {
        match self.mode {
            ViewMode::Raw => None,
            ViewMode::Pretty => self.pretty_editor(),
            ViewMode::Tree if self.data.is_some() => Some(self.list),
            _ => None,
        }
    }

    pub fn pretty_editor(&self) -> Option<Handle> {
        (self.mode == ViewMode::Pretty && self.preview_ready).then_some(self.pretty)
    }

    fn owns(&self, handle: Handle) -> bool {
        self.list == handle || self.pretty == handle || self.all.contains(&handle)
    }

    pub fn keyboard_controls(&self) -> impl Iterator<Item = Handle> {
        self.all.into_iter().chain(self.focus())
    }

    fn start(&mut self, window: Handle, editor: Handle, language: Language) -> io::Result<()> {
        if self.pending.is_some() || self.data.is_some() {
            return Ok(());
        }
        if editor_size(editor) > model::MAX_INPUT {
            title(
                self.notice,
                crate::i18n::text("ui.structured.views.are.available.for.documents.up"),
            );
            return Ok(());
        }
        let text = editor_text(editor);
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("maker-structured-view".into())
            .spawn(move || {
                let result = model::parse(&text, language);
                if tx.send(result).is_ok() {
                    unsafe {
                        PostMessageW(window, READY, editor as usize, 0);
                    }
                }
            })?;
        self.pending = Some(rx);
        title(
            self.notice,
            crate::i18n::text("ui.reading.document.structure"),
        );
        Ok(())
    }

    fn receive(&mut self, settings: &Settings) -> Option<io::Result<()>> {
        let parsed = self.pending.as_ref()?.try_recv().ok()?;
        self.pending = None;
        Some(parsed.map(|model| {
            self.open = vec![false; model.entries.len()];
            for &root in &model.roots {
                self.open[root] = true;
            }
            self.data = Some(model);
            self.rebuild(None, 0);
            self.prepare(settings);
        }))
    }

    fn prepare(&mut self, settings: &Settings) {
        if self.mode != ViewMode::Pretty || self.preview_ready {
            return;
        }
        if let Some(model) = &self.data {
            if let Some(text) = &model.pretty {
                set_editor_text(self.pretty, text);
                select(self.pretty, 0, 0);
                self.preview_ready = true;
                self.apply_style(settings);
            } else {
                title(
                    self.notice,
                    crate::i18n::text("ui.the.formatted.text.is.too.large.use"),
                );
            }
        }
    }

    fn rebuild(&mut self, selected_node: Option<usize>, top: usize) {
        let Some(model) = &self.data else {
            return;
        };
        self.rows.clear();
        let mut index = 0;
        while index < model.entries.len() {
            self.rows.push(index);
            index = if self.open[index] {
                index + 1
            } else {
                model.entries[index].end
            };
        }
        chrome::reset_file_motion(self.list);
        send(self.list, LB_SETCOUNT, self.rows.len(), 0);
        let index = selected_node
            .and_then(|node| self.rows.binary_search(&node).ok())
            .unwrap_or(0);
        send(self.list, LB_SETCURSEL, index, 0);
        send(
            self.list,
            LB_SETTOPINDEX,
            top.min(self.rows.len().saturating_sub(1)),
            0,
        );
        repaint(self.list);
    }

    fn selected(&self) -> Option<(usize, usize)> {
        let row = usize::try_from(send(self.list, LB_GETCURSEL, 0, 0)).ok()?;
        Some((row, *self.rows.get(row)?))
    }

    fn toggle(&mut self, row: usize, key: i32) {
        let Some(model) = &self.data else {
            return;
        };
        let Some(&id) = self.rows.get(row) else {
            return;
        };
        let node = &model.entries[id];
        let current = self.open[id];
        if node.expandable(id)
            && (key == 13 || (key == 0x25 && current) || (key == 0x27 && !current))
        {
            self.open[id] = !current;
            let top = send(self.list, LB_GETTOPINDEX, 0, 0).max(0) as usize;
            self.rebuild(Some(id), top);
        } else if key == 0x27 && current && node.expandable(id) {
            send(self.list, LB_SETCURSEL, row + 1, 0);
        } else if key == 0x25
            && let Some(parent) = node.parent
            && let Ok(row) = self.rows.binary_search(&parent)
        {
            send(self.list, LB_SETCURSEL, row, 0);
        }
    }

    fn paint_row(&self, item: &DrawItem, palette: Paint, px: impl Fn(i32) -> i32) {
        let active = item.state & 1 != 0;
        let color = if active {
            palette.selected_rgb
        } else {
            palette.editor_rgb
        };
        let hover = motion::value(
            motion::Key::item(item.window, item.item as usize, motion::Effect::Hover),
            chrome::row_is_hovered(item.window, item.item as usize),
        );
        let color = motion::mix(color, palette.control_rgb, hover);
        fill_color(item.dc, item.rect, color);
        let Some((&index, model)) = self.rows.get(item.item as usize).zip(self.data.as_ref())
        else {
            return;
        };
        let node = &model.entries[index];
        let indent = (px(14) * node.depth as i32).min(item.rect.width() / 2);
        let start = item.rect.left + px(10) + indent;
        if node.expandable(index) {
            chrome::chevron(
                item.dc,
                Point {
                    x: start + px(7),
                    y: (item.rect.top + item.rect.bottom) / 2,
                },
                px(4),
                f32::from(self.open[index]),
                palette.accent,
            );
        }
        let key_left = start + px(21);
        let value_left = if node.key.is_empty() {
            key_left
        } else {
            let available = (item.rect.right - key_left).max(0);
            let want = px(12)
                + node
                    .key
                    .chars()
                    .map(|c| px(if c.is_ascii() { 7 } else { 13 }))
                    .sum::<i32>();
            let value_left = key_left + want.min(available * 2 / 3);
            draw_label(
                item.dc,
                Rect {
                    left: key_left,
                    right: value_left - px(6),
                    ..item.rect
                },
                &node.key,
                palette.font,
                palette.foreground,
                false,
            );
            value_left
        };
        draw_label(
            item.dc,
            Rect {
                left: value_left,
                right: item.rect.right - px(6),
                ..item.rect
            },
            &node.value,
            palette.font,
            palette.highlight(node.color),
            false,
        );
    }
}

impl Drop for View {
    fn drop(&mut self) {
        for control in [self.list, self.pretty, self.notice]
            .into_iter()
            .chain(self.all)
        {
            unsafe {
                DestroyWindow(control);
            }
        }
    }
}

impl App {
    pub(super) fn layout_data_buttons(&self, group: usize, area: Rect, visible: bool) -> i32 {
        let active = self.group_document(group).is_some_and(|index| {
            let doc = &self.documents[index];
            doc.browser.is_none() && supports(doc.language, doc.path.as_deref())
        });
        let web = active
            && self.group_document(group).is_some_and(|index| {
                let doc = &self.documents[index];
                doc.language == Language::Html && (doc.path.is_some() || doc.response_url.is_some())
            });
        let count = if web { 4 } else { 3 };
        let margin = self.px(5).min(area.width());
        let max_width = (area.width() - margin) / count;
        let mut x = area.right - margin;
        for (index, width) in [48, 88, 45, 130].into_iter().enumerate().rev() {
            let button = self.data_buttons[group][index];
            let show_button = visible && active && max_width > 0 && (index != 3 || web);
            if show_button {
                let width = self.px(width).min(max_width);
                x -= width;
                place(button, x, area.top + 1, width, area.height() - 2);
            }
            show(button, show_button);
        }
        if active { x - area.left } else { area.width() }
    }

    pub(super) fn open_markup_in_browser(&self, group: usize) -> io::Result<()> {
        let Some(doc) = self
            .group_document(group)
            .map(|index| &self.documents[index])
        else {
            return Ok(());
        };
        if doc.language == Language::Html {
            if let Some(url) = doc
                .response_url
                .as_deref()
                .filter(|url| browser::is_url(url))
            {
                shell_open(self.window, OsStr::new(url))?;
            } else if let Some(path) = doc.path.as_deref().filter(|path| path.is_file()) {
                open_html_file(self.window, path)?;
            }
        }
        Ok(())
    }

    pub(super) fn data_mode_selected(&self, id: usize) -> bool {
        let slot = id - FIRST_MODE;
        self.group_document(slot / 3)
            .is_some_and(|index| self.documents[index].data_mode() == MODES[slot % 3])
    }

    pub(super) fn change_data_mode(&mut self, id: usize) -> io::Result<()> {
        let slot = id - FIRST_MODE;
        let Some(index) = self.group_document(slot / 3) else {
            return Ok(());
        };
        self.change_document_data_mode(index, MODES[slot % 3])
    }

    pub(super) fn change_document_data_mode(
        &mut self,
        index: usize,
        mode: ViewMode,
    ) -> io::Result<()> {
        let doc = &self.documents[index];
        if doc.browser.is_some() || !supports(doc.language, doc.path.as_deref()) {
            return Ok(());
        }
        self.dismiss_completion();
        let doc = &mut self.documents[index];
        if mode != ViewMode::Raw && doc.data_view.is_none() {
            doc.data_view = Some(View::new(self.window, &self.settings)?);
        }
        if let Some(view) = doc.data_view.as_mut() {
            view.mode = mode;
            if mode != ViewMode::Raw {
                view.start(self.window, doc.editor, doc.language)?;
                view.prepare(&self.settings);
            }
        }
        self.activate(index, true);
        self.redraw_terminal();
        for button in self.data_buttons.into_iter().flatten() {
            repaint(button);
        }
        Ok(())
    }

    pub(super) fn complete_data_view(&mut self, editor: Handle) {
        let Some(index) = self.documents.iter().position(|doc| doc.editor == editor) else {
            return;
        };
        let Some(view) = &mut self.documents[index].data_view else {
            return;
        };
        let Some(result) = view.receive(&self.settings) else {
            return;
        };
        if let Err(error) = result {
            let message = crate::i18n::message(
                "ui.could.not.read.the.structure",
                &[(error.to_string().replace('\n', " ")).to_string()],
            );
            title(view.notice, &message);
            self.report(&message);
        }
        let group = self.documents[index].group;
        if self.group_document(group) != Some(index) {
            return;
        }
        self.layout();
        if self.active == Some(index)
            && self.documents[index].data_mode() != ViewMode::Raw
            && self
                .data_buttons
                .into_iter()
                .flatten()
                .any(|button| button == unsafe { GetFocus() })
        {
            self.focus_editor();
        }
        self.redraw_terminal();
    }

    pub(super) fn layout_data_panels(&self) {
        for (index, doc) in self.documents.iter().enumerate() {
            if let Some(view) = &doc.data_view {
                let visible = self.group_document(doc.group) == Some(index);
                view.layout(visible.then(|| self.pane_content(doc.group)), self.dpi);
            }
        }
    }

    pub(super) fn data_view_focus(&mut self, handle: Handle) {
        if let Some(index) = self
            .documents
            .iter()
            .position(|doc| doc.data_view.as_ref().is_some_and(|view| view.owns(handle)))
            && self.active != Some(index)
        {
            self.activate(index, false);
        }
    }

    pub(super) fn toggle_data_row(&mut self, handle: Handle, row: usize) {
        if let Some(view) = self
            .documents
            .iter_mut()
            .filter_map(|doc| doc.data_view.as_mut())
            .find(|view| view.list == handle)
        {
            view.toggle(row, 13);
        }
    }

    pub(super) fn keyboard_data_view(&mut self, handle: Handle, key: i32) -> bool {
        if !matches!(key, 13 | 0x25 | 0x27) {
            return false;
        }
        let Some(view) = self
            .documents
            .iter_mut()
            .filter_map(|doc| doc.data_view.as_mut())
            .find(|view| view.list == handle)
        else {
            return false;
        };
        if let Some((row, _)) = view.selected() {
            view.toggle(row, key);
        }
        true
    }

    pub(super) fn expand_data_view(&mut self, sender: Handle, open: bool) {
        self.data_view_focus(sender);
        if let Some(view) = self
            .active
            .and_then(|index| self.documents[index].data_view.as_mut())
        {
            view.open.fill(open);
            view.rebuild(None, 0);
            unsafe {
                SetFocus(view.list);
            }
        }
    }

    pub(super) fn paint_data_row(&self, item: &DrawItem, palette: Paint) {
        if let Some(view) = self
            .documents
            .iter()
            .filter_map(|doc| doc.data_view.as_ref())
            .find(|view| view.list == item.window)
        {
            view.paint_row(item, palette, |n| self.px(n));
        } else {
            fill(item.dc, item.rect, palette.background);
        }
    }
}
