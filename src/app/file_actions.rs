use super::*;
use std::sync::Mutex;

pub(super) const COPY: usize = 393;
pub(super) const PASTE: usize = 394;

struct MakerClipboard {
    sequence: u32,
    paths: Vec<PathBuf>,
}

static FILE_CLIPBOARD: Mutex<Option<MakerClipboard>> = Mutex::new(None);

fn copy_to_clipboard(window: Handle, paths: Vec<PathBuf>) -> bool {
    let private = paths_to_clipboard(window, &paths).is_err();
    *FILE_CLIPBOARD
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = private.then(|| MakerClipboard {
        sequence: clipboard_sequence(),
        paths,
    });
    private
}

fn files_from_clipboard(window: Handle) -> io::Result<Vec<PathBuf>> {
    let sequence = clipboard_sequence();
    let fallback = FILE_CLIPBOARD
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(stored) = fallback
        .as_ref()
        .filter(|stored| sequence == 0 || stored.sequence == sequence)
    {
        return Ok(stored.paths.clone());
    }
    drop(fallback);
    clipboard_paths(window)
}

pub(super) fn executable(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        && path.is_file()
}

impl App {
    pub(super) fn workspace_list(&self, control: Handle) -> bool {
        control == self.list && self.is_tree() || self.browser_list_base(control).is_some()
    }

    pub(super) fn file_row_path(&self, control: Handle, row: usize) -> Option<PathBuf> {
        if control == self.list {
            match self.list_items.get(row)? {
                ListItem::File(path, _)
                | ListItem::Folder { path, .. }
                | ListItem::Archive { path, .. } => Some(path.clone()),
                _ => None,
            }
        } else {
            self.browser_row_path(control, row)
        }
    }

    pub(super) fn file_clipboard(&mut self, id: usize, control: Handle) -> io::Result<()> {
        if id == COPY {
            let paths: Vec<_> = list_selection(control)
                .into_iter()
                .filter_map(|row| self.file_row_path(control, row))
                .collect();
            if paths.is_empty() {
                return Ok(());
            }
            let count = paths.len();
            if copy_to_clipboard(self.window, paths) {
                self.report(&crate::i18n::message(
                    "ui.copied.items.within.maker.the.windows.clipboard",
                    &[(count).to_string()],
                ));
            } else {
                self.report(&crate::i18n::message(
                    "ui.copied.items",
                    &[(count).to_string()],
                ));
            }
            return Ok(());
        }
        let directory = list_selection(control)
            .first()
            .and_then(|&row| self.file_row_path(control, row))
            .and_then(|path| {
                if path.is_dir() {
                    Some(path)
                } else {
                    path.parent().map(Path::to_owned)
                }
            })
            .or_else(|| self.browser_list_base(control).map(Path::to_owned))
            .or_else(|| self.root.clone())
            .ok_or_else(|| {
                io::Error::other(crate::i18n::text("ui.open.the.destination.folder.first"))
            })?;
        let paths = files_from_clipboard(self.window)?;
        if paths.is_empty() {
            self.report(crate::i18n::text(
                "ui.the.clipboard.contains.no.files.or.folders",
            ));
            return Ok(());
        }
        let target = directory.canonicalize()?;
        for source in &paths {
            if !source.is_absolute() {
                return Err(io::Error::other(crate::i18n::text(
                    "ui.a.file.path.on.the.clipboard.is",
                )));
            }
            if source.is_dir() && target.starts_with(source.canonicalize()?) {
                return Err(io::Error::other(crate::i18n::text(
                    "ui.a.folder.cannot.be.copied.into.itself",
                )));
            }
        }
        let complete = copy_paths(self.window, &paths, &directory);
        if let Some(root) = &self.root {
            self.files = text::files(root).unwrap_or_default();
        }
        self.show_files("");
        self.reload_open_folders();
        self.request_git_refresh();
        if complete? {
            self.report(&crate::i18n::message(
                "ui.pasted.items",
                &[(paths.len()).to_string()],
            ));
        }
        Ok(())
    }

    pub(super) fn choose_clipboard_list(&self) -> Handle {
        if self.workspace_list(self.recent_file_list)
            && unsafe { IsWindowVisible(self.recent_file_list) } != 0
        {
            return self.recent_file_list;
        }
        self.active
            .and_then(|index| self.documents[index].browser.as_ref())
            .map_or(self.list, |view| view.list)
    }

    pub(super) fn run_file_row(&self, control: Handle, row: usize) -> bool {
        let Some(path) = self
            .file_row_path(control, row)
            .filter(|path| executable(path))
        else {
            return false;
        };
        if let Err(error) = run_application(self.window, &path) {
            self.show_error(error);
        }
        true
    }
}
