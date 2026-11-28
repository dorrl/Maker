use super::*;

pub(super) const NEW_FILE: usize = 350;
pub(super) const NEW_FOLDER: usize = 351;
pub(super) const NAME: usize = 352;
pub(super) const NAME_FRAME: usize = 353;
pub(super) const CANCEL_EDIT: u32 = 0x8051;

enum Edit {
    Create { parent: PathBuf, folder: bool },
    Rename { path: PathBuf },
}

pub(super) struct Explorer {
    pub file: Handle,
    pub folder: Handle,
    pub input: Handle,
    pub frame: Handle,
    edit: Option<(usize, Edit)>,
    pub selected: Vec<PathBuf>,
    pub archive: archive::Panel,
}

impl Explorer {
    pub fn new(window: Handle) -> io::Result<Self> {
        let button = |id, label| child(window, id, "BUTTON", label, BS_OWNERDRAW | WS_TABSTOP);
        let file = button(NEW_FILE, crate::i18n::text("ui.new.file"))?;
        let folder = button(NEW_FOLDER, crate::i18n::text("ui.new.folder"))?;
        chrome::watch_control(file, true);
        chrome::watch_control(folder, true);
        Ok(Self {
            file,
            folder,
            frame: child(window, NAME_FRAME, chrome::SURFACE, "", 0)?,
            input: child(window, NAME, "EDIT", "", WS_TABSTOP | ES_AUTOHSCROLL)?,
            edit: None,
            selected: Vec::new(),
            archive: archive::Panel::default(),
        })
    }
}

pub(super) fn paint_button(item: &DrawItem, palette: Paint) {
    fill_color(
        item.dc,
        item.rect,
        chrome::button_tint(item, palette, palette.panel_rgb, false),
    );
    let px = |n| n * palette.dpi / 96;
    let left = (item.rect.left + item.rect.right - px(16)) / 2;
    let top = (item.rect.top + item.rect.bottom - px(16)) / 2;
    let folder = item.id as usize == NEW_FOLDER;
    let top_edge = top + px(if folder { 5 } else { 1 });
    let right = left + px(if folder { 14 } else { 11 });
    let bottom = top + px(if folder { 13 } else { 15 });
    let line = |x, y, w, h| fill_color(item.dc, Rect::from_size(x, y, w, h), palette.foreground);
    let stroke = px(1).max(1);
    line(left, top_edge, stroke, bottom - top_edge);
    line(left, top_edge, right - left, stroke);
    line(right - stroke, top_edge, stroke, px(5));
    line(left, bottom, right - left - px(4), stroke);
    if folder {
        line(left + stroke, top_edge - px(2), px(5), px(2));
    }
    let plus_x = right - px(1);
    let plus_y = bottom - px(3);
    fill(
        item.dc,
        Rect::from_size(plus_x - px(3), plus_y, px(7), stroke),
        palette.accent_brush,
    );
    fill(
        item.dc,
        Rect::from_size(plus_x, plus_y - px(3), stroke, px(7)),
        palette.accent_brush,
    );
}

impl App {
    pub(super) fn explorer_editing(&self) -> bool {
        self.explorer.edit.is_some()
    }

    pub(super) fn layout_explorer_buttons(&self, title_area: Rect) {
        let visible = self.sidebar
            && self.root.is_some()
            && self.is_tree()
            && title_area.width() >= self.px(100);
        let size = self.px(25);
        place(
            self.explorer.file,
            title_area.right - self.px(6) - 2 * size,
            title_area.top + self.px(4),
            size,
            size,
        );
        place(
            self.explorer.folder,
            title_area.right - self.px(6) - size,
            title_area.top + self.px(4),
            size,
            size,
        );
        for button in [self.explorer.file, self.explorer.folder] {
            show(button, visible);
        }
    }

    pub(super) fn layout_explorer_edit(&self) {
        let Some(frame) = self.explorer_edit_rect() else {
            show(self.explorer.input, false);
            show(self.explorer.frame, false);
            return;
        };
        let input_height = self.px(17).min(frame.height() - 2);
        let input = Rect::from_size(
            frame.left + self.px(4),
            frame.top + (frame.height() - input_height) / 2,
            frame.width() - self.px(8),
            input_height,
        );
        for (window, area) in [(self.explorer.frame, frame), (self.explorer.input, input)] {
            if unsafe { IsWindowVisible(window) } == 0 || relative_rect(window, self.window) != area
            {
                show_overlay(window, area);
            }
        }
    }

    fn explorer_edit_rect(&self) -> Option<Rect> {
        let (row, _) = self.explorer.edit.as_ref()?;
        if !self.sidebar || !self.is_tree() {
            return None;
        }
        let item = list_item_rect(self.list, *row)?;
        let viewport = bounds(self.list);
        if item.top < viewport.top || item.bottom > viewport.bottom {
            return None;
        }
        let data = send(self.list, LB_GETITEMDATA, *row, 0) as usize;
        let left =
            item.left + chrome::file_row_label_left(data >> 8, item.width(), self.dpi) - self.px(4);
        let right = viewport.right - self.px(4);
        if right - left < self.px(40) {
            return None;
        }
        let origin = map_point(self.list, self.window, Point::default());
        Some(Rect::from_size(
            origin.x + left,
            origin.y + item.top + 1,
            right - left,
            item.height() - 2,
        ))
    }

    fn start_explorer_edit(&mut self, row: usize, edit: Edit, name: &str, select_length: isize) {
        self.dismiss_completion();
        let hint = match &edit {
            Edit::Create { folder: true, .. } => crate::i18n::text("ui.folder.name"),
            Edit::Create { folder: false, .. } => crate::i18n::text("ui.file.name"),
            Edit::Rename { .. } => "",
        };
        self.explorer.edit = Some((row, edit));
        let cue: Vec<u16> = hint.encode_utf16().chain([0]).collect();
        send(self.explorer.input, 0x1501, 1, cue.as_ptr() as isize);
        title(self.explorer.input, name);
        self.reveal_explorer_row(row);
        self.sync_scrollbars();
        self.layout_explorer_edit();
        if self.explorer_edit_rect().is_none() {
            self.cancel_explorer_edit(true);
            self.report(crate::i18n::text("ui.explorer.name_space"));
            return;
        }
        unsafe {
            SetFocus(self.explorer.input);
        }
        send(self.explorer.input, 0x00b1, 0, select_length);
    }

    fn reveal_explorer_row(&self, row: usize) {
        let height = bounds(self.list).height();
        if let Some(item) = list_item_rect(self.list, row)
            && (item.top < 0 || item.bottom > height)
        {
            let visible = (height / item.height().max(1)).max(1) as usize;
            let top = if item.top < 0 {
                row
            } else {
                (row + 1).saturating_sub(visible)
            };
            send(self.list, LB_SETTOPINDEX, top, 0);
        }
    }

    pub(super) fn cancel_explorer_edit(&mut self, focus: bool) {
        let Some((row, edit)) = self.explorer.edit.take() else {
            return;
        };
        show(self.explorer.input, false);
        show(self.explorer.frame, false);
        if matches!(edit, Edit::Create { .. }) {
            chrome::reset_file_motion(self.list);
            self.list_items.remove(row);
            send(self.list, LB_DELETESTRING, row, 0);
            self.redraw_scroll_position(self.list);
        }
        if focus && self.sidebar {
            self.report("");
            unsafe {
                SetFocus(self.list);
            }
        }
    }

    pub(super) fn new_explorer_item(&mut self, folder: bool) -> io::Result<()> {
        self.cancel_explorer_edit(false);
        if !self.sidebar || !self.is_tree() {
            return Ok(());
        }
        let Some(root) = self.root.as_ref() else {
            return Err(io::Error::other(crate::i18n::text(
                "ui.open.a.workspace.folder.first",
            )));
        };
        let parent = self
            .explorer_selection
            .as_deref()
            .filter(|path| path.starts_with(root))
            .and_then(|path| {
                if path.is_dir() {
                    Some(path)
                } else {
                    path.parent()
                }
            })
            .unwrap_or(root)
            .to_owned();
        let depth = parent
            .strip_prefix(root)
            .map_or(0, |path| path.components().count());
        if depth > 0 && !self.expanded.contains(&parent) {
            self.expanded.insert(parent.clone());
            self.show_files("");
        }
        let mut row = if depth == 0 {
            0
        } else {
            let Some(row) = self.list_items.iter().position(
                |item| matches!(item, ListItem::Folder { path, open: true, .. } if path == &parent),
            ) else {
                return Ok(());
            };
            row + 1
        };
        if !folder {
            while self.list_items.get(row).is_some_and(|item| match item {
                ListItem::Folder { depth: level, .. } => *level >= depth,
                ListItem::Archive { depth: level, .. }
                | ListItem::ArchiveMember { depth: level, .. }
                | ListItem::File(_, Some(level)) => *level > depth,
                _ => false,
            }) {
                row += 1;
            }
        }
        let icon = if folder {
            chrome::folder_icon(Path::new(""), false)
        } else {
            chrome::file_icon(None)
        };
        chrome::reset_file_motion(self.list);
        insert_list_entry(self.list, row, (depth << 8) | icon)?;
        self.list_items.insert(row, ListItem::Draft);
        self.report(if folder {
            crate::i18n::text("ui.enter.a.new.folder.name.and.press")
        } else {
            crate::i18n::text("ui.enter.a.new.file.name.and.press")
        });
        self.start_explorer_edit(row, Edit::Create { parent, folder }, "", -1);
        Ok(())
    }

    pub(super) fn rename_explorer_item(&mut self) {
        self.cancel_explorer_edit(false);
        if !self.sidebar || !self.is_tree() {
            return;
        }
        let selected = list_selection(self.list);
        if selected.len() != 1 {
            self.report(crate::i18n::text("ui.select.one.file.or.folder.to.rename"));
            return;
        }
        let row = selected[0];
        let Some(path) = self.list_items.get(row).and_then(row_path) else {
            return;
        };
        let path = path.to_owned();
        let label = file_name(&path).into_owned();
        let basename = if !matches!(self.list_items[row], ListItem::Folder { .. }) {
            path.file_stem().and_then(OsStr::to_str).unwrap_or(&label)
        } else {
            &label
        };
        let select_length = basename.encode_utf16().count() as isize;
        self.start_explorer_edit(row, Edit::Rename { path }, &label, select_length);
    }

    pub(super) fn commit_explorer_edit(&mut self) -> io::Result<()> {
        let Some((_, edit)) = self.explorer.edit.as_ref() else {
            return Ok(());
        };
        let name = caption(self.explorer.input);
        validate_component(&name)?;
        let (target, new_file) = match edit {
            Edit::Create { parent, folder } => {
                let target = parent.join(&name);
                if *folder {
                    fs::create_dir(&target)?;
                } else {
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)?;
                }
                (target, !folder)
            }
            Edit::Rename { path } => {
                let before = path.clone();
                let target = path.with_file_name(&name);
                if before.as_os_str() != target.as_os_str() {
                    move_path(&before, &target)?;
                    self.rebase_explorer_paths(&before, &target);
                }
                (target, false)
            }
        };
        self.cancel_explorer_edit(false);
        self.search = None;
        self.showing_issues = false;
        if let Some(root) = self.root.as_ref() {
            for parent in target
                .ancestors()
                .skip(1)
                .take_while(|parent| *parent != root)
            {
                self.expanded.insert(parent.to_owned());
            }
        }
        self.files_stale = true;
        self.explorer_selection = Some(target.clone());
        self.explorer.selected.clear();
        self.explorer_top = send(self.list, LB_GETTOPINDEX, 0, 0).max(0) as usize;
        self.show_files("");
        if let Some(row) = self
            .list_items
            .iter()
            .position(|item| row_path(item) == Some(target.as_path()))
        {
            self.reveal_explorer_row(row);
            self.redraw_scroll_position(self.list);
            self.remember_tree_focus(row, target.clone());
        }
        if new_file {
            self.open_path(&target)?;
        } else {
            unsafe {
                SetFocus(self.list);
            }
        }
        self.update_status();
        self.report(crate::i18n::text("ui.updated.in.explorer"));
        self.request_git_refresh();
        self.queue_session_save();
        Ok(())
    }

    pub(super) fn track_explorer_selection(&mut self) {
        if !self.is_tree() {
            return;
        }
        let rows = list_selection(self.list);
        self.explorer.selected = rows
            .iter()
            .filter_map(|row| {
                self.list_items
                    .get(*row)
                    .and_then(row_path)
                    .map(Path::to_owned)
            })
            .collect();
        let row = list_caret(self.list)
            .filter(|row| rows.contains(row))
            .or_else(|| rows.last().copied())
            .unwrap_or(usize::MAX);
        if let Some(path) = self.list_items.get(row).and_then(row_path) {
            self.remember_tree_focus(row, path.to_owned());
        } else {
            self.explorer_selection = None;
            self.queue_session_save();
        }
    }

    pub(super) fn delete_explorer_items(&mut self) -> io::Result<()> {
        use std::os::windows::fs::MetadataExt;
        let Some(root) = &self.root else {
            return Ok(());
        };
        let mut paths: Vec<PathBuf> = Vec::new();
        for path in list_selection(self.list)
            .into_iter()
            .filter_map(|row| row_path(self.list_items.get(row)?))
        {
            if path == root
                || !path.starts_with(root)
                || paths.iter().any(|parent| path.starts_with(parent))
            {
                continue;
            }
            for parent in path
                .ancestors()
                .skip(1)
                .take_while(|parent| *parent != root)
            {
                if fs::symlink_metadata(parent)?.file_attributes() & 0x400 != 0 {
                    return Err(io::Error::other(crate::i18n::text(
                        "ui.items.in.linked.folders.cannot.be.moved",
                    )));
                }
            }
            paths.push(path.to_owned());
        }
        if paths.is_empty() {
            self.report(crate::i18n::text("ui.select.files.or.folders.to.move.to"));
            return Ok(());
        }
        let count = paths.len();
        let prompt = if count == 1 {
            crate::i18n::message(
                "ui.move.to.the.recycle.bin",
                &[(file_name(&paths[0])).to_string()],
            )
        } else {
            crate::i18n::message(
                "ui.move.the.selected.items.to.the.recycle",
                &[(count).to_string()],
            )
        };
        if message(
            self.window,
            &prompt,
            "Maker",
            MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2,
        ) != 1
        {
            return Ok(());
        }
        let result = recycle_paths(self.window, &paths);
        paths.retain(|path| {
            fs::symlink_metadata(path).is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        });
        let removed = |path: &Path| paths.iter().any(|parent| path.starts_with(parent));
        if !paths.is_empty() {
            for index in (0..self.documents.len()).rev() {
                let doc = &mut self.documents[index];
                if doc.path.as_deref().is_some_and(removed) {
                    if send(doc.editor, EM_GETMODIFY, 0, 0) != 0 {
                        doc.path = None;
                        doc.label =
                            crate::i18n::message("ui.unsaved", std::slice::from_ref(&doc.label));
                    } else {
                        self.remove_document(index, false);
                    }
                } else if doc
                    .archive_member
                    .as_ref()
                    .is_some_and(|(source, _)| removed(source))
                {
                    self.remove_document(index, false);
                }
            }
            self.expanded.retain(|path| !removed(path));
            self.recently_closed.retain(|path| !removed(path));
            self.explorer_selection = self.explorer_selection.take().filter(|path| !removed(path));
            self.explorer.selected.retain(|path| !removed(path));
            self.explorer.archive.reset();
            if let Some(root) = &self.root
                && let Ok(files) = text::files(root)
            {
                self.files = files;
            }
            self.show_files("");
            self.refresh_tab();
            self.update_status();
            self.request_git_refresh();
            self.queue_session_save();
            unsafe {
                SetFocus(self.list);
            }
        }
        let completed = result?;
        let deleted = paths.len();
        self.report(if completed && deleted == count {
            crate::i18n::text("ui.moved.to.the.recycle.bin")
        } else if deleted > 0 {
            crate::i18n::text("ui.some.items.were.moved.to.the.recycle")
        } else {
            crate::i18n::text("ui.the.recycle.bin.operation.was.canceled.or")
        });
        Ok(())
    }

    fn rebase_explorer_paths(&mut self, before: &Path, after: &Path) {
        self.explorer.archive.reset();
        let mut changed_language = Vec::new();
        for (index, doc) in self.documents.iter_mut().enumerate() {
            let Some(path) = doc
                .path
                .as_deref()
                .and_then(|path| rebase(path, before, after))
            else {
                continue;
            };
            let language = Language::from_path(&path);
            if doc.language != language {
                changed_language.push(index);
                doc.data_view = None;
            } else if !data_view::supports(language, Some(&path)) {
                doc.data_view = None;
            }
            doc.label = file_name(&path).into_owned();
            doc.language = language;
            doc.path = Some(path);
        }
        for index in changed_language {
            self.format(index, true);
        }
        self.expanded = self
            .expanded
            .drain()
            .map(|path| rebase(&path, before, after).unwrap_or(path))
            .collect();
        for path in &mut self.recently_closed {
            if let Some(updated) = rebase(path, before, after) {
                *path = updated;
            }
        }
        self.rebase_terminal_paths(|path| rebase(path, before, after));
        self.layout();
    }
}

pub(super) fn row_path(item: &ListItem) -> Option<&Path> {
    match item {
        ListItem::Folder { path, .. }
        | ListItem::Archive { path, .. }
        | ListItem::File(path, Some(_)) => Some(path),
        _ => None,
    }
}

fn rebase(path: &Path, before: &Path, after: &Path) -> Option<PathBuf> {
    for candidate in path.ancestors() {
        if same_path(candidate, before) {
            let rest = path.strip_prefix(candidate).ok()?;
            return Some(if rest.as_os_str().is_empty() {
                after.to_owned()
            } else {
                after.join(rest)
            });
        }
    }
    None
}

pub(super) fn validate_component(name: &str) -> io::Result<()> {
    let first = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(
        first.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || first
        .strip_prefix("COM")
        .or_else(|| first.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        });
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name.encode_utf16().count() > 255
        || reserved
        || name.chars().any(|ch| {
            ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
    {
        Err(io::Error::other(crate::i18n::text(
            "ui.explorer.invalid_name",
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rename_cannot_overwrite_and_accepts_case_only_changes() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("maker-move-test-{}", std::process::id()));
        fs::create_dir(&root)?;
        let source = root.join("before.txt");
        let other = root.join("other.txt");
        let renamed = root.join("Before.txt");
        let result = (|| {
            fs::write(&source, "one")?;
            fs::write(&other, "two")?;
            assert!(move_path(&source, &other).is_err());
            assert_eq!(fs::read_to_string(&other)?, "two");
            move_path(&source, &renamed)?;
            assert!(
                fs::read_dir(&root)?
                    .flatten()
                    .any(|entry| entry.file_name() == "Before.txt")
            );
            Ok(())
        })();
        fs::remove_file(renamed).ok();
        fs::remove_file(source).ok();
        fs::remove_file(other).ok();
        fs::remove_dir(root).ok();
        result
    }
}
