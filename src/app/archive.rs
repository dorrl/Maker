use super::*;
use std::{
    collections::{BTreeMap, HashMap},
    ffi::{CStr, c_void},
    io::Write,
    os::windows::ffi::OsStrExt,
    ptr::null_mut,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const TIMER: usize = 8;
const MAX_ENTRIES: usize = 20_000;
const MAX_PREVIEW: usize = 3 * 1024 * 1024;
const MAX_EXTRACT: u64 = 8 * 1024 * 1024 * 1024;
const SUFFIXES: &[&str] = &[
    ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".zip", ".7z", ".rar", ".tar", ".tgz", ".tbz2",
    ".txz",
];

pub(super) fn supported(path: &Path) -> bool {
    suffix(&file_name(path)).is_some()
}

fn suffix(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    SUFFIXES
        .iter()
        .copied()
        .find(|suffix| lower.ends_with(suffix))
}

type Raw = *mut c_void;

struct Api {
    new: unsafe extern "C" fn() -> Raw,
    filters: [ReaderStatus; 4],
    formats: unsafe extern "C" fn(Raw) -> i32,
    open: unsafe extern "C" fn(Raw, *const u16, usize) -> i32,
    next: unsafe extern "C" fn(Raw, *mut Raw) -> i32,
    name: unsafe extern "C" fn(Raw) -> *const std::ffi::c_char,
    kind: unsafe extern "C" fn(Raw) -> u32,
    hardlink: unsafe extern "C" fn(Raw) -> *const u16,
    data: unsafe extern "C" fn(Raw, *mut u8, usize) -> isize,
    error: unsafe extern "C" fn(Raw) -> *const std::ffi::c_char,
    free: unsafe extern "C" fn(Raw) -> i32,
}

impl Api {
    fn load() -> Option<Self> {
        // Load only the archive component installed with Windows.
        let module = system_library("archiveint.dll");
        if module == 0 {
            return None;
        }
        macro_rules! symbol {
            ($name:literal, $signature:ty) => {{
                let address = unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) };
                if address.is_null() {
                    return None;
                }
                unsafe { std::mem::transmute::<*mut c_void, $signature>(address) }
            }};
        }
        Some(Self {
            new: symbol!("archive_read_new", unsafe extern "C" fn() -> Raw),
            filters: [
                symbol!("archive_read_support_filter_gzip", ReaderStatus),
                symbol!("archive_read_support_filter_bzip2", ReaderStatus),
                symbol!("archive_read_support_filter_xz", ReaderStatus),
                symbol!("archive_read_support_filter_zstd", ReaderStatus),
            ],
            formats: symbol!("archive_read_support_format_all", ReaderStatus),
            open: symbol!(
                "archive_read_open_filename_w",
                unsafe extern "C" fn(Raw, *const u16, usize) -> i32
            ),
            next: symbol!(
                "archive_read_next_header",
                unsafe extern "C" fn(Raw, *mut Raw) -> i32
            ),
            name: symbol!("archive_entry_pathname_utf8", ReaderString),
            kind: symbol!("archive_entry_filetype", unsafe extern "C" fn(Raw) -> u32),
            hardlink: symbol!(
                "archive_entry_hardlink_w",
                unsafe extern "C" fn(Raw) -> *const u16
            ),
            data: symbol!(
                "archive_read_data",
                unsafe extern "C" fn(Raw, *mut u8, usize) -> isize
            ),
            error: symbol!("archive_error_string", ReaderString),
            free: symbol!("archive_read_free", ReaderStatus),
        })
    }
}

type ReaderStatus = unsafe extern "C" fn(Raw) -> i32;
type ReaderString = unsafe extern "C" fn(Raw) -> *const std::ffi::c_char;

struct Reader {
    api: &'static Api,
    raw: Raw,
}

impl Drop for Reader {
    fn drop(&mut self) {
        unsafe {
            (self.api.free)(self.raw);
        }
    }
}

impl Reader {
    fn open(path: &Path) -> io::Result<Self> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        let api = API.get_or_init(Api::load).as_ref().ok_or_else(|| {
            io::Error::other("이 Windows에는 압축 파일을 읽는 시스템 구성 요소가 없습니다.")
        })?;
        let raw = unsafe { (api.new)() };
        if raw.is_null() {
            return Err(io::Error::other("압축 파일을 열 수 없습니다."));
        }
        let reader = Self { api, raw };
        let name: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        let filter = match suffix(&file_name(path)) {
            Some(".tar.gz" | ".tgz") => Some(0),
            Some(".tar.bz2" | ".tbz2") => Some(1),
            Some(".tar.xz" | ".txz") => Some(2),
            Some(".tar.zst") => Some(3),
            _ => None,
        };
        unsafe {
            // A warning can indicate libarchive would use an external utility.
            if let Some(index) = filter
                && (api.filters[index])(raw) != 0
            {
                return Err(reader.error());
            }
            (api.formats)(raw);
            if (api.open)(raw, name.as_ptr(), 64 * 1024) != 0 {
                return Err(reader.error());
            }
        }
        Ok(reader)
    }

    fn next(&mut self) -> io::Result<Option<Entry>> {
        let mut raw = null_mut();
        match unsafe { (self.api.next)(self.raw, &mut raw) } {
            0 if !raw.is_null() => {
                let name = unsafe { (self.api.name)(raw).as_ref() }
                    .and_then(|ptr| unsafe { CStr::from_ptr(ptr) }.to_str().ok())
                    .ok_or_else(|| {
                        io::Error::other("압축 파일에서 항목 이름을 읽을 수 없습니다.")
                    })?;
                let kind = unsafe { (self.api.kind)(raw) };
                let regular = kind == 0o100000 && unsafe { (self.api.hardlink)(raw).is_null() };
                Ok(Some(Entry {
                    path: member_path(name)?,
                    directory: kind == 0o040000,
                    regular,
                }))
            }
            1 => Ok(None),
            _ => Err(self.error()),
        }
    }

    fn copy(&self, output: &mut impl Write, budget: &mut u64) -> io::Result<()> {
        let mut buffer = [0; 32 * 1024];
        loop {
            let count = unsafe { (self.api.data)(self.raw, buffer.as_mut_ptr(), buffer.len()) };
            if count == 0 {
                return Ok(());
            }
            if count < 0 {
                return Err(self.error());
            }
            *budget = budget
                .checked_sub(count as u64)
                .ok_or_else(|| io::Error::other("압축을 푼 데이터가 크기 제한을 넘었습니다."))?;
            output.write_all(&buffer[..count as usize])?;
        }
    }

    fn error(&self) -> io::Error {
        let error = unsafe { (self.api.error)(self.raw).as_ref() };
        let detail = error.map(|ptr| unsafe { CStr::from_ptr(ptr) }.to_string_lossy());
        io::Error::other(match detail {
            Some(detail) => format!("압축 파일을 처리할 수 없습니다. {detail}"),
            None => "압축 파일을 처리할 수 없습니다.".to_owned(),
        })
    }
}

struct Entry {
    path: PathBuf,
    directory: bool,
    regular: bool,
}

fn member_path(name: &str) -> io::Result<PathBuf> {
    if name.starts_with(['/', '\\']) {
        return Err(io::Error::other("압축 파일에 절대 경로가 들어 있습니다."));
    }
    let mut path = PathBuf::new();
    for component in name
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
    {
        explorer::validate_component(component)?;
        if path.components().count() >= 64 {
            return Err(io::Error::other("압축 파일의 폴더 경로가 너무 깊습니다."));
        }
        path.push(component);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_paths_remain_inside_destination() {
        assert_eq!(
            member_path("./src/한글.txt").unwrap(),
            Path::new(r"src\한글.txt")
        );
        assert_eq!(member_path("empty/").unwrap(), Path::new("empty"));
        for invalid in [
            "../file",
            "dir/../file",
            "/file",
            r"\\host\file",
            r"C:\file",
            "file.txt:stream",
            "dir/CON.txt",
            "dir./file",
        ] {
            assert!(member_path(invalid).is_err(), "{invalid}");
        }
    }
}

fn scan(path: &Path, mut visit: impl FnMut(&Reader, Entry) -> io::Result<bool>) -> io::Result<()> {
    let mut reader = Reader::open(path)?;
    for count in 0..=MAX_ENTRIES {
        let Some(entry) = reader.next()? else {
            return Ok(());
        };
        if count == MAX_ENTRIES {
            return Err(io::Error::other("압축 파일에 항목이 너무 많습니다."));
        }
        if !visit(&reader, entry)? {
            return Ok(());
        }
    }
    unreachable!()
}

type Catalog = HashMap<PathBuf, Vec<(PathBuf, bool)>>;

fn catalog(path: &Path) -> io::Result<Catalog> {
    let mut nodes = BTreeMap::new();
    scan(path, |_, entry| {
        if !entry.path.as_os_str().is_empty() && (entry.directory || entry.regular) {
            for folder in entry
                .path
                .ancestors()
                .skip(1)
                .take_while(|p| !p.as_os_str().is_empty())
            {
                nodes.insert(folder.to_owned(), true);
            }
            nodes.entry(entry.path).or_insert(entry.directory);
        }
        Ok(true)
    })?;
    let mut children: Catalog = HashMap::new();
    for (path, directory) in nodes {
        let parent = path.parent().unwrap_or(Path::new("")).to_owned();
        children.entry(parent).or_default().push((path, directory));
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|(path, directory)| (!*directory, file_name(path).to_lowercase()));
    }
    Ok(children)
}

fn preview(archive: &Path, member: &Path) -> io::Result<(String, Encoding)> {
    let mut content = None;
    scan(archive, |reader, entry| {
        if entry.path == member && entry.regular {
            let mut bytes = Vec::new();
            reader.copy(&mut bytes, &mut (MAX_PREVIEW as u64))?;
            content = Some(text::decode(&bytes)?);
            return Ok(false);
        }
        Ok(true)
    })?;
    content
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "압축 파일에 해당 항목이 없습니다."))
}

fn extract(archive: &Path, destination: &Path) -> io::Result<PathBuf> {
    let parent = destination.canonicalize()?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let scratch = parent.join(format!(".maker-unpack-{}-{timestamp}", std::process::id()));
    fs::create_dir(&scratch)?;
    let result = (|| {
        let mut budget = MAX_EXTRACT;
        scan(archive, |reader, entry| {
            let target = scratch.join(&entry.path);
            if entry.directory {
                fs::create_dir_all(&target)?;
            } else if entry.regular && !entry.path.as_os_str().is_empty() {
                fs::create_dir_all(target.parent().expect("entry has a parent"))?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)?;
                reader.copy(&mut file, &mut budget)?;
            } else {
                return Err(io::Error::other(
                    "링크 또는 특수 항목이 들어 있는 압축 파일입니다.",
                ));
            }
            Ok(true)
        })?;
        let name = file_name(archive);
        let stem = &name[..name.len() - suffix(&name).unwrap_or("").len()];
        let stem = if explorer::validate_component(stem).is_ok() {
            stem
        } else {
            "압축 풀기"
        };
        for index in 1..=999 {
            let label = if index == 1 {
                stem.to_owned()
            } else {
                format!("{stem} ({index})")
            };
            let target = parent.join(&label);
            match move_path(&scratch, &target) {
                Ok(()) => return Ok(destination.join(label)),
                Err(_) if target.exists() => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other(
            "압축을 풀 새 폴더 이름을 만들 수 없습니다.",
        ))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(scratch);
    }
    result
}

enum Reply {
    Catalog(PathBuf, Catalog),
    Preview(PathBuf, PathBuf, String, Encoding),
    Extracted(PathBuf),
}

#[derive(Default)]
pub(super) struct Panel {
    catalogs: HashMap<PathBuf, Catalog>,
    opened: HashSet<PathBuf>,
    folders: HashSet<(PathBuf, PathBuf)>,
    job: Option<mpsc::Receiver<io::Result<Reply>>>,
    extracting: bool,
}

impl Panel {
    pub fn is_extracting(&self) -> bool {
        self.extracting
    }
    pub fn opened(&self, archive: &Path) -> bool {
        self.opened.contains(archive)
    }

    pub fn reset(&mut self) {
        self.catalogs.clear();
        self.opened.clear();
        self.folders.clear();
    }

    pub fn rows(&self, archive: &Path, parent: &Path, depth: usize, rows: &mut Vec<ListItem>) {
        let Some(children) = self.catalogs.get(archive).and_then(|tree| tree.get(parent)) else {
            return;
        };
        for (path, directory) in children {
            let open = *directory && self.folders.contains(&(archive.to_owned(), path.clone()));
            rows.push(ListItem::ArchiveMember {
                archive: archive.to_owned(),
                path: path.clone(),
                depth,
                directory: *directory,
                open,
            });
            if open {
                self.rows(archive, path, depth + 1, rows);
            }
        }
    }

    fn pending(&self) -> bool {
        self.job.is_some()
    }

    fn start(
        &mut self,
        window: Handle,
        operation: impl FnOnce() -> io::Result<Reply> + Send + 'static,
    ) -> io::Result<()> {
        let (send, receive) = mpsc::channel();
        thread::Builder::new()
            .name("maker-archive".into())
            .spawn(move || {
                let _ = send.send(operation());
            })?;
        self.job = Some(receive);
        unsafe {
            SetTimer(window, TIMER, 75, 0);
        }
        Ok(())
    }
}

impl App {
    pub(super) fn archive_toggle(&mut self, path: PathBuf) -> io::Result<()> {
        if self.explorer.archive.opened.remove(&path) {
            self.show_files("");
        } else if self.explorer.archive.pending() {
            self.report("압축 파일을 처리하고 있습니다.");
        } else {
            self.report("압축 파일의 목록을 읽고 있습니다.");
            self.explorer.archive.start(self.window, move || {
                Ok(Reply::Catalog(path.clone(), catalog(&path)?))
            })?;
        }
        Ok(())
    }

    pub(super) fn archive_member(
        &mut self,
        archive: PathBuf,
        path: PathBuf,
        directory: bool,
    ) -> io::Result<()> {
        if directory {
            let key = (archive.clone(), path.clone());
            if !self.explorer.archive.folders.remove(&key) {
                self.explorer.archive.folders.insert(key);
            }
            self.show_files("");
            if let Some(row) = self.list_items.iter().position(|item| {
                matches!(item, ListItem::ArchiveMember { archive: source, path: entry, .. } if *source == archive && *entry == path)
            }) {
                list_select(self.list, row);
            }
        } else if self.explorer.archive.pending() {
            self.report("압축 파일을 처리하고 있습니다.");
        } else {
            self.explorer.archive.start(self.window, move || {
                let (body, encoding) = preview(&archive, &path)?;
                Ok(Reply::Preview(archive, path, body, encoding))
            })?;
        }
        Ok(())
    }

    pub(super) fn archive_extract(&mut self, path: PathBuf) -> io::Result<()> {
        if self.explorer.archive.pending() {
            self.report("압축 파일을 처리하고 있습니다.");
            return Ok(());
        }
        let prompt = format!("{} 압축을 풀 위치 선택", file_name(&path));
        if let Some(parent) = folder_dialog(self.window, path.parent(), &prompt)? {
            self.explorer.archive.start(self.window, move || {
                extract(&path, &parent).map(Reply::Extracted)
            })?;
            self.explorer.archive.extracting = true;
            self.report("압축을 풀고 있습니다.");
        }
        Ok(())
    }

    pub(super) fn archive_tick(&mut self) {
        let Some(job) = &self.explorer.archive.job else {
            return;
        };
        let result = match job.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(io::Error::other("압축 작업이 종료되었습니다."))
            }
        };
        self.explorer.archive.job = None;
        self.explorer.archive.extracting = false;
        unsafe {
            KillTimer(self.window, TIMER);
        }
        match result {
            Ok(Reply::Catalog(path, tree)) => {
                if self
                    .root
                    .as_ref()
                    .is_some_and(|root| path.starts_with(root))
                    && path.is_file()
                {
                    self.explorer.archive.opened.insert(path.clone());
                    self.explorer.archive.catalogs.insert(path, tree);
                    if self.is_tree() {
                        self.show_files("");
                    }
                    self.report("압축 파일을 펼쳤습니다.");
                }
            }
            Ok(Reply::Preview(archive, path, body, encoding)) => {
                let existing = self.documents.iter().position(|doc| {
                    doc.archive_member
                        .as_ref()
                        .is_some_and(|(source, member)| *source == archive && *member == path)
                });
                if let Some(index) = existing {
                    self.activate(index, true);
                    return;
                }
                let label = format!("{} · {}", file_name(&path), file_name(&archive));
                match self.add_document(None, label, &body, encoding) {
                    Ok(()) => {
                        let index = self.active.expect("archive document added");
                        let document = &mut self.documents[index];
                        document.language = Language::from_path(&path);
                        document.archive_member = Some((archive, path));
                        send(document.editor, EM_SETREADONLY, 1, 0);
                        self.format(index, true);
                        self.update_status();
                    }
                    Err(error) => self.show_error(error),
                }
            }
            Ok(Reply::Extracted(path)) => {
                if let Some(root) = self.root.as_deref()
                    && let Ok(files) = text::files(root)
                {
                    self.files = files;
                }
                if self.is_tree() {
                    self.show_files("");
                }
                self.request_git_refresh();
                self.report(&format!("압축을 풀었습니다. {}", path.display()));
            }
            Err(error) => self.show_error(error),
        }
    }
}
