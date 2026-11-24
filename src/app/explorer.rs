use super::*;

pub(super) const NEW_FILE: usize = 350;
pub(super) const NEW_FOLDER: usize = 351;
pub(super) const NAME: usize = 352;

pub(super) enum Edit {
    Create { parent: PathBuf, folder: bool },
    Rename { path: PathBuf },
}

pub(super) struct Explorer {
    pub file: Handle,
    pub folder: Handle,
    pub input: Handle,
    pub edit: Option<Edit>,
    pub selected: Vec<PathBuf>,
    pub archive: archive::Panel,
}

impl Explorer {
    pub fn new(window: Handle) -> io::Result<Self> {
        let button = |id, label| child(window, id, "BUTTON", label, BS_OWNERDRAW | WS_TABSTOP);
        let file = button(NEW_FILE, "새 파일")?;
        let folder = button(NEW_FOLDER, "새 폴더")?;
        chrome::watch_control(file, true);
        chrome::watch_control(folder, true);
        Ok(Self {
            file,
            folder,
            input: child(
                window,
                NAME,
                "EDIT",
                "",
                WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
            )?,
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
        if !self.sidebar
            || !self.is_tree()
            || !matches!(self.explorer.edit, Some(Edit::Create { .. }))
        {
            show(self.explorer.input, false);
            return;
        }
        let list = relative_rect(self.list, self.window);
        show_overlay(
            self.explorer.input,
            Rect::from_size(
                list.left + self.px(13),
                list.top - self.px(28),
                (list.width() - self.px(77)).max(0),
                self.px(24),
            ),
        );
    }

    fn start_explorer_edit(&mut self, edit: Edit, name: &str, select_length: i32) {
        self.explorer.edit = Some(edit);
        title(self.explorer.input, name);
        self.layout_explorer_edit();
        send(self.explorer.input, 0x00b1, 0, select_length as isize);
        unsafe {
            SetFocus(self.explorer.input);
        }
    }

    pub(super) fn cancel_explorer_edit(&mut self, focus: bool) {
        if self.explorer.edit.take().is_some() {
            show(self.explorer.input, false);
            if focus && self.sidebar {
                unsafe {
                    SetFocus(self.list);
                }
            }
        }
    }

    pub(super) fn new_explorer_item(&mut self, folder: bool) -> io::Result<()> {
        let Some(root) = self.root.as_ref() else {
            return Err(io::Error::other("먼저 작업 폴더를 여세요."));
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
        self.report(if folder {
            "새 폴더 이름을 입력하고 Enter를 누르세요."
        } else {
            "새 파일 이름을 입력하고 Enter를 누르세요."
        });
        if self.mode == Some(Mode::Rename) {
            self.clear_mode();
        }
        self.start_explorer_edit(Edit::Create { parent, folder }, "", -1);
        Ok(())
    }

    pub(super) fn rename_explorer_item(&mut self) {
        if !self.is_tree() {
            return;
        }
        let selected = list_selection(self.list);
        if selected.len() != 1 {
            self.report("이름을 바꿀 파일이나 폴더를 하나만 선택하세요.");
            return;
        }
        let row = selected[0];
        let Some((path, _)) = self.list_items.get(row).and_then(row_path) else {
            return;
        };
        let path = path.to_owned();
        let label = file_name(&path).into_owned();
        let basename = if path.is_file() {
            path.file_stem().and_then(OsStr::to_str).unwrap_or(&label)
        } else {
            &label
        };
        self.show_mode(Mode::Rename);
        self.explorer.edit = Some(Edit::Rename { path: path.clone() });
        title(
            self.mode_label,
            if path.is_file() {
                "파일 이름"
            } else {
                "폴더 이름"
            },
        );
        title(self.query, &label);
        send(
            self.query,
            0x00b1,
            0,
            basename.encode_utf16().count() as isize,
        );
    }

    pub(super) fn commit_explorer_edit(&mut self) -> io::Result<()> {
        let rename = matches!(self.explorer.edit, Some(Edit::Rename { .. }));
        let name = caption(if rename {
            self.query
        } else {
            self.explorer.input
        });
        validate_component(&name)?;
        let Some(edit) = self.explorer.edit.as_ref() else {
            return Ok(());
        };
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
        if rename {
            self.mode = None;
        }
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
            if let Ok(files) = text::files(root) {
                self.files = files;
            }
        }
        self.explorer_selection = Some(target.clone());
        self.explorer.selected.clear();
        self.show_files("");
        if rename {
            self.layout();
        }
        if let Some(row) = self
            .list_items
            .iter()
            .position(|item| row_path(item).is_some_and(|(path, _)| path == target))
        {
            list_select(self.list, row);
        }
        if new_file {
            self.open_path(&target)?;
        } else {
            unsafe {
                SetFocus(self.list);
            }
        }
        self.update_status();
        self.report("탐색기에 반영했습니다.");
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
                    .map(|(path, _)| path.to_owned())
            })
            .collect();
        let row = list_caret(self.list)
            .filter(|row| rows.contains(row))
            .or_else(|| rows.last().copied())
            .unwrap_or(usize::MAX);
        if let Some((path, _)) = self.list_items.get(row).and_then(row_path) {
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
        for (path, _) in list_selection(self.list)
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
                    return Err(io::Error::other(
                        "연결된 폴더 안에서는 휴지통으로 이동할 수 없습니다.",
                    ));
                }
            }
            paths.push(path.to_owned());
        }
        if paths.is_empty() {
            self.report("휴지통으로 보낼 파일이나 폴더를 선택하세요.");
            return Ok(());
        }
        let count = paths.len();
        let prompt = if count == 1 {
            format!("「{}」 항목을 휴지통으로 이동할까요?", file_name(&paths[0]))
        } else {
            format!("선택한 {count}개 항목을 휴지통으로 이동할까요?")
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
                        doc.label = format!("{} · 미저장", doc.label);
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
            "휴지통으로 이동했습니다."
        } else if deleted > 0 {
            "일부 항목을 휴지통으로 이동했습니다."
        } else {
            "휴지통 이동을 취소했거나 선택한 항목을 이동할 수 없습니다."
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
        self.refresh_tab();
    }
}

fn row_path(item: &ListItem) -> Option<(&Path, usize)> {
    match item {
        ListItem::Folder { path, depth, .. }
        | ListItem::Archive { path, depth, .. }
        | ListItem::File(path, Some(depth)) => Some((path, *depth)),
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
        Err(io::Error::other(
            "Windows에서 사용할 수 있는 파일 또는 폴더 이름을 입력하세요.",
        ))
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
