use super::*;
use std::{
    ffi::OsString,
    io::{BufReader, BufWriter, Read, Write},
    os::windows::ffi::{OsStrExt, OsStringExt},
};

const MAGIC: &[u8] = b"MAKER-SESSION\x01";
const MAX_ITEMS: usize = 16_384;
const MAX_BUFFER: usize = 64 * 1024 * 1024;
const MAX_SESSION: u64 = 256 * 1024 * 1024;
pub(super) const TIMER: usize = 4;

struct Entry {
    path: Option<PathBuf>,
    label: String,
    draft: Option<String>,
    encoding: Encoding,
    position: Selection,
    scroll: Point,
}

#[derive(Default)]
struct Session {
    root: Option<PathBuf>,
    open: Vec<Entry>,
    active: Option<usize>,
    other: Option<usize>,
    active_right: bool,
    sidebar: bool,
    expanded: Vec<PathBuf>,
    selected: Option<PathBuf>,
    top: usize,
    untitled: usize,
    recent: Vec<PathBuf>,
    terminal: terminal::SavedPanel,
    git_view: bool,
    window: Option<SavedWindow>,
    editor_layout: Option<(bool, i32, i32)>,
    secondary: Option<PathBuf>,
    fullscreen: bool,
    groups: Vec<usize>,
}

impl App {
    pub(super) fn queue_session_save(&self) {
        if self.detached {
            return;
        }
        unsafe {
            SetTimer(self.window, TIMER, 850, 0);
        }
    }

    pub(super) fn save_session(&self) -> io::Result<()> {
        unsafe {
            KillTimer(self.window, TIMER);
        }
        if self.detached {
            return Ok(());
        }
        if self.fresh_window
            && self.root.is_none()
            && !self.documents.iter().any(|doc| {
                !doc.is_preview()
                    && (session_path(doc).is_some() || send(doc.editor, EM_GETMODIFY, 0, 0) != 0)
            })
        {
            return Ok(());
        }
        let save = |doc: &Document| {
            !doc.is_preview() && (doc.browser.is_none() || session_path(doc).is_some())
        };
        let saved_index = |index: Option<usize>| {
            index
                .filter(|&i| save(&self.documents[i]))
                .map(|i| self.documents[..i].iter().filter(|doc| save(doc)).count())
        };
        let active = saved_index(self.active);
        let other = saved_index(self.other);
        let mut state = Session {
            root: self.root.clone(),
            active: active.or(other),
            other: other.filter(|_| active.is_some()),
            active_right: self.active_right && active.is_some() && other.is_some(),
            sidebar: self.sidebar,
            expanded: self.expanded.iter().cloned().collect(),
            selected: self.explorer_selection.clone(),
            top: if self.is_tree() {
                send(self.list, LB_GETTOPINDEX, 0, 0).max(0) as usize
            } else {
                self.explorer_top
            },
            untitled: self.untitled,
            recent: self
                .recently_closed
                .iter()
                .rev()
                .take(128)
                .cloned()
                .collect(),
            terminal: self.terminal.saved(),
            git_view: self.git.open,
            window: self.fullscreen.or_else(|| window_placement(self.window)),
            fullscreen: self.fullscreen.is_some(),
            groups: Vec::new(),
            editor_layout: Some((self.dock.stacked, self.dock.ratio, self.dock.sidebar_ratio)),
            secondary: self
                .secondary_browser
                .as_ref()
                .and_then(browser::Browser::folder)
                .map(Path::to_owned),
            open: Vec::new(),
        };
        state.recent.reverse();
        for doc in self.documents.iter().filter(|doc| save(doc)) {
            state.groups.push(doc.group);
            let mut scroll = Point::default();
            send(
                doc.editor,
                EM_GETSCROLLPOS,
                0,
                &mut scroll as *mut _ as isize,
            );
            state.open.push(Entry {
                path: session_path(doc).map(Path::to_owned),
                label: doc.label.clone(),
                draft: (send(doc.editor, EM_GETMODIFY, 0, 0) != 0).then(|| editor_text(doc.editor)),
                encoding: doc.encoding,
                position: selection(doc.editor),
                scroll,
            });
        }
        let path = session_file()?;
        fs::create_dir_all(path.parent().expect("session file has a parent"))?;
        text::write_atomically(&path, |file| {
            let mut writer = BufWriter::new(file);
            state.write(&mut writer)?;
            writer.flush()
        })
    }

    pub(super) fn restore_session(&mut self) -> bool {
        let state = session_file().and_then(|path| {
            let file = fs::File::open(path)?;
            if file.metadata()?.len() > MAX_SESSION {
                return Err(invalid());
            }
            Session::read(&mut BufReader::new(file))
        });
        let state = match state {
            Ok(state) => state,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return false,
            Err(_) => {
                self.report("이전 작업 상태를 읽을 수 없습니다.");
                return false;
            }
        };
        if let Some(saved) = state.window {
            position_main_window(self.window, saved);
            if state.fullscreen {
                self.fullscreen = Some(saved);
            }
        }
        if let Some(root) = state.root.filter(|path| path.is_dir()) {
            self.expanded = state
                .expanded
                .into_iter()
                .filter(|path| path.starts_with(&root))
                .collect();
            self.explorer_selection = state.selected.filter(|path| path.starts_with(&root));
            self.files = text::files(&root).unwrap_or_default();
            self.explorer_top = state.top;
            self.root = Some(root);
        }
        self.sidebar = state.sidebar;
        self.git.open = state.git_view;
        self.untitled = state.untitled;
        self.recently_closed = state.recent;
        let mut index_map = Vec::new();
        let mut positions = Vec::new();
        let mut skipped = 0;
        for entry in state.open {
            if entry.path.is_none() && entry.draft.is_none() {
                index_map.push(None);
                continue;
            }
            if entry.draft.is_none()
                && let Some(folder) = entry.path.as_ref().filter(|path| path.is_dir())
            {
                let before = self.documents.len();
                let opened = self.add_browser(folder.clone(), true).is_ok();
                index_map.push(opened.then_some(before));
                skipped += usize::from(!opened);
                continue;
            }
            let dirty = entry.draft.is_some();
            let payload = match (entry.draft, &entry.path) {
                (Some(code), _) => Ok((code, entry.encoding)),
                (None, Some(path)) if path.is_file() => text::read(path),
                (None, None) => Ok((String::new(), entry.encoding)),
                _ => Err(invalid()),
            };
            let before = self.documents.len();
            let opened = payload
                .and_then(|(code, encoding)| {
                    if code.len() > MAX_BUFFER {
                        return Err(invalid());
                    }
                    self.add_document(entry.path, entry.label, &code, encoding)
                })
                .is_ok();
            index_map.push(opened.then_some(before));
            if opened {
                let doc = &mut self.documents[before];
                doc.dirty = dirty;
                send(doc.editor, EM_SETMODIFY, usize::from(dirty), 0);
                positions.push((doc.editor, entry.position, entry.scroll));
            } else {
                skipped += 1;
            }
        }
        let resolve = |index: Option<usize>| index.and_then(|i| *index_map.get(i)?);
        self.active = resolve(state.active).or(self.active);
        self.other = resolve(state.other).filter(|index| self.active != Some(*index));
        self.active_right = state.active_right && self.other.is_some();
        if let (Some(active), Some(other)) = (self.active, self.other) {
            for (saved, loaded) in index_map.iter().enumerate() {
                if let Some(index) = loaded {
                    self.documents[*index].group = state.groups.get(saved).copied().unwrap_or(0);
                }
            }
            self.documents[active].group = usize::from(self.active_right);
            self.documents[other].group = usize::from(!self.active_right);
        }
        if let Some((stacked, ratio, sidebar_ratio)) = state.editor_layout {
            self.dock.stacked = stacked;
            self.dock.ratio = ratio.clamp(18, 82);
            self.dock.sidebar_ratio = sidebar_ratio.clamp(22, 78);
        }
        self.secondary_browser = state
            .secondary
            .filter(|path| path.is_dir())
            .and_then(|path| browser::Browser::new(self.window, path, true).ok());
        self.show_files("");
        self.restore_terminal(state.terminal);
        self.layout();
        self.request_git_refresh();
        for (editor, position, scroll) in positions {
            let length = editor_size(editor).min(i32::MAX as usize) as i32;
            select(
                editor,
                position.start.clamp(0, length),
                position.end.clamp(0, length),
            );
            send(editor, EM_SETSCROLLPOS, 0, &scroll as *const _ as isize);
        }
        if let Some(index) = self.active {
            self.tab_reveal(index);
        }
        self.update_status();
        if skipped > 0 {
            self.report(&format!("열 수 없는 파일 {skipped}개를 건너뛰었습니다."));
        }
        state.window.is_some_and(|window| window.maximized)
    }

    pub(super) fn prepare_session_exit(&mut self) -> bool {
        if self.explorer.archive.is_extracting() {
            self.report("압축 풀기가 끝난 뒤 창을 닫을 수 있습니다.");
            return false;
        }
        if self.detached {
            return self.confirm_exit();
        }
        match self.save_session() {
            Ok(()) => true,
            Err(error) => {
                self.show_error(io::Error::other(format!(
                    "이전 작업 상태를 저장할 수 없습니다. {error}"
                )));
                self.confirm_exit()
            }
        }
    }
}

fn session_path(doc: &Document) -> Option<&Path> {
    doc.browser
        .as_ref()
        .and_then(browser::Browser::folder)
        .or(doc.path.as_deref())
}

fn session_file() -> io::Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("Maker").join("session.bin"))
        .ok_or_else(|| io::Error::other("작업 상태를 저장할 폴더가 없습니다."))
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid Maker session")
}

impl Session {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        let draft_bytes: usize = self
            .open
            .iter()
            .filter_map(|entry| entry.draft.as_ref().map(String::len))
            .sum();
        if draft_bytes > MAX_SESSION as usize - MAX_BUFFER || self.open.len() > MAX_ITEMS {
            return Err(invalid());
        }
        writer.write_all(MAGIC)?;
        write_path(writer, self.root.as_deref())?;
        write_index(writer, self.active)?;
        write_index(writer, self.other)?;
        writer.write_all(&[u8::from(self.active_right), u8::from(self.sidebar)])?;
        write_paths(writer, &self.expanded)?;
        write_path(writer, self.selected.as_deref())?;
        write_u32(writer, self.top)?;
        write_u32(writer, self.untitled)?;
        write_paths(writer, &self.recent)?;
        write_u32(writer, self.open.len())?;
        for entry in &self.open {
            write_path(writer, entry.path.as_deref())?;
            write_string(writer, &entry.label)?;
            writer.write_all(&[u8::from(entry.encoding.bom), u8::from(entry.encoding.crlf)])?;
            for value in [
                entry.position.start,
                entry.position.end,
                entry.scroll.x,
                entry.scroll.y,
            ] {
                writer.write_all(&value.to_le_bytes())?;
            }
            writer.write_all(&[u8::from(entry.draft.is_some())])?;
            if let Some(draft) = &entry.draft {
                write_string(writer, draft)?;
            }
        }
        writer.write_all(&[u8::from(self.terminal.visible)])?;
        write_u32(writer, self.terminal.active)?;
        write_u32(writer, self.terminal.tabs.len())?;
        for (shell, path) in &self.terminal.tabs {
            writer.write_all(&[*shell])?;
            write_path(writer, Some(path))?;
        }
        writer.write_all(&[u8::from(self.git_view), u8::from(self.window.is_some())])?;
        if let Some(window) = self.window {
            writer.write_all(&[u8::from(window.maximized)])?;
            for coordinate in [
                window.normal.left,
                window.normal.top,
                window.normal.right,
                window.normal.bottom,
            ] {
                writer.write_all(&coordinate.to_le_bytes())?;
            }
        }
        if let Some((stacked, ratio, sidebar_ratio)) = self.editor_layout {
            writer.write_all(&[u8::from(stacked)])?;
            writer.write_all(&ratio.to_le_bytes())?;
            writer.write_all(&sidebar_ratio.to_le_bytes())?;
            write_path(writer, self.secondary.as_deref())?;
            writer.write_all(&[u8::from(self.fullscreen)])?;
            write_u32(writer, self.groups.len())?;
            for group in &self.groups {
                writer.write_all(&[u8::from(*group == 1)])?;
            }
        }
        Ok(())
    }

    fn read(reader: &mut impl Read) -> io::Result<Self> {
        let mut magic = [0u8; MAGIC.len()];
        reader.read_exact(&mut magic)?;
        if magic != MAGIC {
            return Err(invalid());
        }
        let mut state = Self {
            root: read_path(reader)?,
            active: read_index(reader)?,
            other: read_index(reader)?,
            active_right: read_bool(reader)?,
            sidebar: read_bool(reader)?,
            expanded: read_paths(reader)?,
            selected: read_path(reader)?,
            top: read_u32(reader)?,
            untitled: read_u32(reader)?,
            recent: read_paths(reader)?,
            ..Self::default()
        };
        let count = read_count(reader, MAX_ITEMS)?;
        for _ in 0..count {
            let path = read_path(reader)?;
            let label = read_string(reader, 4096)?;
            let encoding = Encoding {
                bom: read_bool(reader)?,
                crlf: read_bool(reader)?,
            };
            let position = Selection {
                start: read_i32(reader)?,
                end: read_i32(reader)?,
            };
            let scroll = Point {
                x: read_i32(reader)?,
                y: read_i32(reader)?,
            };
            let draft = if read_bool(reader)? {
                Some(read_string(reader, MAX_BUFFER)?)
            } else {
                None
            };
            state.open.push(Entry {
                path,
                label,
                draft,
                encoding,
                position,
                scroll,
            });
        }
        state.terminal.visible = read_bool(reader)?;
        state.terminal.active = read_u32(reader)?;
        let count = read_count(reader, 500)?;
        for _ in 0..count {
            let shell = read_byte(reader)?;
            let path = read_path(reader)?.ok_or_else(invalid)?;
            state.terminal.tabs.push((shell, path));
        }
        state.git_view = match read_bool(reader) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => false,
            Err(error) => return Err(error),
        };
        match read_bool(reader) {
            Ok(true) => {
                state.window = Some(SavedWindow {
                    maximized: read_bool(reader)?,
                    normal: Rect {
                        left: read_i32(reader)?,
                        top: read_i32(reader)?,
                        right: read_i32(reader)?,
                        bottom: read_i32(reader)?,
                    },
                });
            }
            Ok(false) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(error) => return Err(error),
        }
        match read_bool(reader) {
            Ok(stacked) => {
                state.editor_layout = Some((stacked, read_i32(reader)?, read_i32(reader)?));
                state.secondary = read_path(reader)?;
                match read_bool(reader) {
                    Ok(fullscreen) => {
                        state.fullscreen = fullscreen;
                        let count = read_count(reader, MAX_ITEMS)?;
                        for _ in 0..count {
                            state.groups.push(usize::from(read_bool(reader)?));
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
                    Err(error) => return Err(error),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(error) => return Err(error),
        }
        Ok(state)
    }
}

fn write_u32(writer: &mut impl Write, value: usize) -> io::Result<()> {
    writer.write_all(&u32::try_from(value).map_err(|_| invalid())?.to_le_bytes())
}

fn read_i32(reader: &mut impl Read) -> io::Result<i32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_u32(reader: &mut impl Read) -> io::Result<usize> {
    Ok(read_i32(reader)? as u32 as usize)
}

fn read_count(reader: &mut impl Read, max: usize) -> io::Result<usize> {
    let n = read_u32(reader)?;
    if n > max { Err(invalid()) } else { Ok(n) }
}

fn write_index(writer: &mut impl Write, index: Option<usize>) -> io::Result<()> {
    write_u32(writer, index.map_or(0, |value| value + 1))
}

fn read_index(reader: &mut impl Read) -> io::Result<Option<usize>> {
    Ok(read_u32(reader)?.checked_sub(1))
}

fn read_byte(reader: &mut impl Read) -> io::Result<u8> {
    let mut byte = [0];
    reader.read_exact(&mut byte)?;
    Ok(byte[0])
}

fn read_bool(reader: &mut impl Read) -> io::Result<bool> {
    match read_byte(reader)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid()),
    }
}

fn write_string(writer: &mut impl Write, value: &str) -> io::Result<()> {
    write_u32(writer, value.len())?;
    writer.write_all(value.as_bytes())
}

fn read_string(reader: &mut impl Read, max: usize) -> io::Result<String> {
    let len = read_count(reader, max)?;
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid())
}

fn write_path(writer: &mut impl Write, path: Option<&Path>) -> io::Result<()> {
    let Some(path) = path else {
        return write_u32(writer, 0);
    };
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    write_u32(writer, units.len() + 1)?;
    for unit in units {
        writer.write_all(&unit.to_le_bytes())?;
    }
    Ok(())
}

fn read_path(reader: &mut impl Read) -> io::Result<Option<PathBuf>> {
    let count = read_count(reader, 32_768)?;
    if count == 0 {
        return Ok(None);
    }
    let mut units = Vec::with_capacity(count - 1);
    for _ in 1..count {
        let mut bytes = [0; 2];
        reader.read_exact(&mut bytes)?;
        units.push(u16::from_le_bytes(bytes));
    }
    let name = PathBuf::from(OsString::from_wide(&units));
    if name.is_absolute() && !units.contains(&0) {
        Ok(Some(name))
    } else {
        Err(invalid())
    }
}

fn write_paths(writer: &mut impl Write, paths: &[PathBuf]) -> io::Result<()> {
    write_u32(writer, paths.len().min(MAX_ITEMS))?;
    for path in paths.iter().take(MAX_ITEMS) {
        write_path(writer, Some(path))?;
    }
    Ok(())
}

fn read_paths(reader: &mut impl Read) -> io::Result<Vec<PathBuf>> {
    let count = read_count(reader, MAX_ITEMS)?;
    (0..count)
        .map(|_| read_path(reader)?.ok_or_else(invalid))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_editor_and_explorer_split_sizes() {
        let session = Session {
            editor_layout: Some((true, 63, 37)),
            secondary: Some(PathBuf::from(r"C:\dev\second")),
            groups: vec![0, 1, 0],
            fullscreen: true,
            ..Session::default()
        };
        let mut bytes = Vec::new();
        session.write(&mut bytes).unwrap();
        let restored = Session::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(restored.editor_layout, session.editor_layout);
        assert_eq!(restored.secondary, session.secondary);
        assert_eq!(restored.groups, session.groups);
        assert!(restored.fullscreen);
    }

    #[test]
    fn preserves_unicode_drafts_and_tab_selection() {
        let session = Session {
            active: Some(0),
            window: Some(SavedWindow {
                normal: Rect::from_size(-700, 20, 860, 580),
                maximized: true,
            }),
            open: vec![Entry {
                path: Some(PathBuf::from(r"C:\dev\한글.rs")),
                label: "한글.rs".into(),
                draft: Some("fn 이름() {} 🦀\n".into()),
                encoding: Encoding {
                    bom: true,
                    crlf: true,
                },
                position: Selection { start: 4, end: 6 },
                scroll: Point { x: 7, y: 30 },
            }],
            ..Session::default()
        };
        let mut bytes = Vec::new();
        session.write(&mut bytes).unwrap();
        let read = Session::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(read.active, Some(0));
        assert_eq!(read.open[0].draft, session.open[0].draft);
        assert_eq!(read.open[0].path, session.open[0].path);
        assert_eq!(read.open[0].position, session.open[0].position);
        assert!(read.open[0].encoding.crlf);
        let window = read.window.expect("saved window");
        assert_eq!(window.normal.left, -700);
        assert_eq!(window.normal.width(), 860);
        assert!(window.maximized);

        let previous = &bytes[..bytes.len() - 18];
        assert!(Session::read(&mut &previous[..]).unwrap().window.is_none());
    }
}
