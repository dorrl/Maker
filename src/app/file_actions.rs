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
                self.report(&format!(
                    "Maker에 {count}개 항목을 복사했습니다. Windows 클립보드에 접근할 수 없습니다."
                ));
            } else {
                self.report(&format!("{count}개 항목을 복사했습니다."));
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
            .ok_or_else(|| io::Error::other("붙여넣을 폴더를 먼저 여세요."))?;
        let paths = files_from_clipboard(self.window)?;
        if paths.is_empty() {
            self.report("클립보드에 파일이나 폴더가 없습니다.");
            return Ok(());
        }
        let target = directory.canonicalize()?;
        for source in &paths {
            if !source.is_absolute() {
                return Err(io::Error::other(
                    "클립보드의 파일 경로가 올바르지 않습니다.",
                ));
            }
            if source.is_dir() && target.starts_with(source.canonicalize()?) {
                return Err(io::Error::other(
                    "폴더를 자기 자신이나 하위 폴더에 복사할 수 없습니다.",
                ));
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
            self.report(&format!("{}개 항목을 붙여넣었습니다.", paths.len()));
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
