use super::*;
mod graph;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::Read,
    os::windows::process::CommandExt,
    path::Component,
    process::{Command, Output, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

pub(super) const TIMER: usize = 7;
pub(super) const EXPLORER_VIEW: usize = 360;
pub(super) const CHANGES_VIEW: usize = 361;
const MESSAGE: usize = 362;
pub(super) const COMMIT: usize = 363;
pub(super) const STAGE: usize = 364;
pub(super) const UNSTAGE: usize = 365;
pub(super) const REFRESH: usize = 366;
pub(super) const GRAPH_BRANCH: usize = 367;
pub(super) const GRAPH_COMPARE: usize = 368;
pub(super) const GRAPH_LIST: usize = 369;
pub(super) const REF_PICK_START: usize = 2000;
pub(super) const REF_PICK_END: usize = 2399;
pub(super) const READY: u32 = 0x8004;
const MAX_DIFF: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Change {
    pub path: PathBuf,
    old: Option<PathBuf>,
    pub kind: u8,
    pub staged: bool,
}

#[derive(PartialEq, Eq)]
struct Snapshot {
    root: PathBuf,
    branch: String,
    changes: Vec<Change>,
}

enum Operation {
    Status,
    Diff(Change),
    ShowCommit(String, String),
    Index { changes: Vec<Change>, stage: bool },
    Commit(String),
}

enum Reply {
    Empty,
    Status(Snapshot, bool),
    Diff(Change, String),
    CommitPreview(String, String, String),
}

struct Job {
    root: PathBuf,
    result: mpsc::Receiver<io::Result<Reply>>,
    background: bool,
}

pub(super) struct Panel {
    pub open: bool,
    pub explorer: Handle,
    pub changes: Handle,
    pub message: Handle,
    pub graph: graph::Graph,
    commit: Handle,
    stage: Handle,
    unstage: Handle,
    refresh: Handle,
    root: Option<PathBuf>,
    snapshot: Option<Snapshot>,
    job: Option<Job>,
    preview: Option<Job>,
    pending_preview: Option<Operation>,
    pending: Option<Operation>,
    next_scan: Instant,
    notice: Option<String>,
}

impl Panel {
    pub fn new(window: Handle) -> io::Result<Self> {
        let button = |id, name| -> io::Result<Handle> {
            let handle = child(window, id, "BUTTON", name, BS_OWNERDRAW | WS_TABSTOP)?;
            chrome::watch_control(handle, true);
            Ok(handle)
        };
        let message = child(
            window,
            MESSAGE,
            "EDIT",
            "",
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
        )?;
        edit_hint(message, "커밋 메시지");
        Ok(Self {
            open: false,
            explorer: button(EXPLORER_VIEW, "탐색기")?,
            changes: button(CHANGES_VIEW, "소스 제어")?,
            message,
            graph: graph::Graph::new(window)?,
            commit: button(COMMIT, "커밋")?,
            stage: button(STAGE, "+ 스테이지")?,
            unstage: button(UNSTAGE, "− 해제")?,
            refresh: button(REFRESH, "⟳")?,
            root: None,
            snapshot: None,
            job: None,
            preview: None,
            pending_preview: None,
            pending: None,
            next_scan: Instant::now(),
            notice: None,
        })
    }

    pub fn controls(&self) -> [Handle; 7] {
        [
            self.explorer,
            self.changes,
            self.message,
            self.commit,
            self.stage,
            self.unstage,
            self.refresh,
        ]
    }

    pub fn has_repository(&self) -> bool {
        self.snapshot.is_some()
    }

    pub fn action_controls(&self) -> [Handle; 5] {
        [
            self.message,
            self.commit,
            self.stage,
            self.unstage,
            self.refresh,
        ]
    }

    fn start(&mut self, window: Handle, root: PathBuf, operation: Operation) -> io::Result<()> {
        if self.job.is_some() {
            return Err(io::Error::other("Git 작업이 진행 중입니다."));
        }
        let known = self.snapshot.as_ref().map(|snapshot| snapshot.root.clone());
        self.job = Some(spawn_job(window, root, known, operation)?);
        Ok(())
    }
}

fn spawn_job(
    window: Handle,
    root: PathBuf,
    known: Option<PathBuf>,
    operation: Operation,
) -> io::Result<Job> {
    let background = matches!(operation, Operation::Status);
    let (sender, result) = mpsc::channel();
    let directory = root.clone();
    thread::Builder::new()
        .name("maker-git".to_owned())
        .spawn(move || {
            let _ = sender.send(run(&directory, known.as_deref(), operation));
            unsafe {
                PostMessageW(window, READY, 0, 0);
            }
        })?;
    Ok(Job {
        root,
        result,
        background,
    })
}

impl App {
    pub(super) fn layout_sidebar_tabs(&self, area: Rect) {
        for (index, tab) in [self.git.explorer, self.git.changes]
            .into_iter()
            .enumerate()
        {
            place(
                tab,
                area.left,
                area.top + index as i32 * self.px(36),
                area.width(),
                self.px(36),
            );
            show(tab, self.sidebar && area.width() > 0);
        }
    }

    pub(super) fn paint_sidebar_tab(&self, item: &DrawItem, palette: Paint) {
        let is_git = item.id as usize == CHANGES_VIEW;
        let active = self.git.open == is_git;
        let emphasis = motion::value(
            motion::Key::control(item.window, motion::Effect::Selected),
            active,
        );
        let tint = chrome::button_tint(item, palette, palette.chrome_rgb, active);
        fill_color(item.dc, item.rect, tint);
        if emphasis > 0.0 {
            let mut accent = item.rect;
            if self.settings.sidebar_right {
                accent.left = accent.right - self.px(2);
            } else {
                accent.right = accent.left + self.px(2);
            }
            fill_color(item.dc, accent, motion::mix(tint, palette.accent, emphasis));
        }
        let center = (item.rect.left + item.rect.right) / 2;
        let middle = (item.rect.top + item.rect.bottom) / 2;
        let p = |px| self.px(px);
        let stroke = |x, y, width, height| {
            fill_color(
                item.dc,
                Rect::from_size(
                    center + p(x),
                    middle + p(y),
                    p(width).max(1),
                    p(height).max(1),
                ),
                motion::mix(palette.foreground, palette.accent, emphasis),
            );
        };
        if is_git {
            stroke(-5, -5, 1, 11);
            stroke(-4, 0, 8, 1);
            stroke(3, -5, 1, 6);
            for (x, y) in [(-7, -8), (-7, 4), (1, -8)] {
                stroke(x, y, 5, 5);
            }
        } else {
            stroke(-7, -5, 1, 12);
            stroke(-6, 6, 9, 1);
            stroke(-4, -8, 1, 12);
            stroke(-3, -8, 9, 1);
            stroke(5, -8, 1, 12);
            stroke(-4, 3, 10, 1);
        }
    }

    pub(super) fn layout_git_controls(&self, area: Rect, header: i32) -> i32 {
        let show_git = self.sidebar && self.git.open;
        let show_actions = show_git && self.git.snapshot.is_some();
        let gap = self.px(5);
        let margin = self.px(7);
        let row = self.px(25);
        let left = area.left + margin;
        let top = area.top + header;
        let width = (area.width() - margin * 2).max(0);
        let button = self.px(44);
        place(
            self.git.refresh,
            area.right - margin - self.px(24),
            area.top + gap,
            self.px(24),
            row,
        );
        show(self.git.refresh, show_git && self.root.is_some());
        place(self.git.message, left, top, width - button - gap, row);
        place(self.git.commit, left + width - button, top, button, row);
        let half = (width - gap).max(0) / 2;
        place(self.git.stage, left, top + row + gap, half, row);
        place(
            self.git.unstage,
            left + half + gap,
            top + row + gap,
            half,
            row,
        );
        for window in [
            self.git.message,
            self.git.commit,
            self.git.stage,
            self.git.unstage,
        ] {
            show(window, show_actions);
        }
        top + if show_actions { 2 * (row + gap) } else { 0 }
    }

    pub(super) fn set_git_view(&mut self, open: bool) {
        let previous = self.git.open;
        self.sidebar = true;
        self.git.open = open;
        if matches!(self.mode, Some(Mode::Rename)) && self.explorer.edit.is_some() {
            self.mode = None;
        }
        self.cancel_explorer_edit(false);
        if matches!(self.mode, Some(Mode::Quick | Mode::Workspace)) {
            self.mode = None;
            self.search = None;
        }
        self.show_files("");
        self.layout();
        for (button, is_git) in [(self.git.explorer, false), (self.git.changes, true)] {
            motion::animate(
                self.window,
                motion::Key::control(button, motion::Effect::Selected),
                motion::Area::Control,
                previous == is_git,
                open == is_git,
            );
        }
        unsafe {
            SetFocus(self.list);
        }
        if open {
            self.request_git_refresh();
        }
    }

    pub(super) fn request_git_refresh(&mut self) {
        let root = self.root.clone();
        if self.git.root != root {
            self.git.root = root.clone();
            self.git.snapshot = None;
            self.git.notice = None;
            self.git.pending = None;
            self.git.pending_preview = None;
            self.git.graph.reset();
            title(self.git.message, "");
            if self.git.open {
                self.show_git_items();
            }
        }
        self.git.next_scan = Instant::now();
        self.git.graph.invalidate();
        if self.sidebar && self.git.open && root.is_some() {
            unsafe {
                SetTimer(self.window, TIMER, 400, 0);
            }
            self.git_tick();
        }
    }

    pub(super) fn git_tick(&mut self) {
        self.check_git_preview();
        self.check_git_graph();
        if let Some(job) = &self.git.job {
            let result = match job.result.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err(io::Error::other("Git 작업이 종료되었습니다."))
                }
            };
            let root = job.root.clone();
            let background = job.background;
            self.git.job = None;
            if self.root.as_ref() == Some(&root) {
                match result {
                    Ok(Reply::Status(snapshot, committed)) => {
                        let first = self.git.snapshot.is_none();
                        let changed = self.git.snapshot.as_ref() != Some(&snapshot)
                            || self.git.notice.is_some();
                        self.git.snapshot = Some(snapshot);
                        self.git.notice = None;
                        if committed {
                            title(self.git.message, "");
                            self.report("커밋했습니다.");
                            self.git.graph.invalidate();
                        }
                        if self.git.open && changed {
                            self.show_git_items();
                            if first {
                                self.layout();
                            }
                        }
                    }
                    Ok(Reply::Empty) => {
                        let notice = "Git 저장소가 없습니다.".to_owned();
                        if self.git.snapshot.take().is_some()
                            || self.git.notice.as_ref() != Some(&notice)
                        {
                            self.git.notice = Some(notice);
                            if self.git.open {
                                self.show_git_items();
                                self.layout();
                            }
                        }
                    }
                    Err(error) => {
                        if background {
                            let notice = error.to_string();
                            if self.git.snapshot.take().is_some()
                                || self.git.notice.as_ref() != Some(&notice)
                            {
                                self.git.notice = Some(notice);
                                if self.git.open {
                                    self.show_git_items();
                                    self.layout();
                                }
                            }
                        } else {
                            self.show_error(error);
                        }
                    }
                    Ok(Reply::Diff(..) | Reply::CommitPreview(..)) => unreachable!(),
                }
                self.git.next_scan = Instant::now() + Duration::from_secs(3);
            }
        }
        if !self.sidebar || !self.git.open || self.root.is_none() {
            unsafe {
                KillTimer(self.window, TIMER);
            }
            self.git.pending = None;
        } else if let Some(operation) = self.git.pending.take() {
            let root = self.root.clone().expect("opened workspace");
            if let Err(error) = self.git.start(self.window, root, operation) {
                self.show_error(error);
            }
        } else if Instant::now() >= self.git.next_scan && self.git.job.is_none() {
            let root = self.root.clone().expect("opened workspace");
            if let Err(error) = self.git.start(self.window, root, Operation::Status) {
                self.report(&error.to_string());
            }
            self.git.next_scan = Instant::now() + Duration::from_secs(3);
        }
        self.check_git_graph();
    }

    pub(super) fn show_git_items(&mut self) {
        let selected: Vec<_> = list_selection(self.list)
            .into_iter()
            .filter_map(|row| match self.list_items.get(row) {
                Some(ListItem::Git(change)) => Some((change.path.clone(), change.staged)),
                _ => None,
            })
            .collect();
        let mut items = Vec::new();
        if let Some(snapshot) = &self.git.snapshot {
            title(
                self.sidebar_title,
                &format!("소스 제어 · {}", snapshot.branch),
            );
            for staged in [true, false] {
                let matches: Vec<_> = snapshot
                    .changes
                    .iter()
                    .filter(|change| change.staged == staged)
                    .cloned()
                    .collect();
                let label = if staged {
                    "스테이징된 변경"
                } else {
                    "변경 사항"
                };
                items.push(ListItem::Group(format!("{label} ({})", matches.len())));
                items.extend(matches.into_iter().map(ListItem::Git));
            }
        } else {
            title(self.sidebar_title, "소스 제어");
            items.push(ListItem::Group(self.git.notice.clone().unwrap_or_else(
                || {
                    if self.root.is_none() {
                        "작업 폴더를 열어 주세요.".into()
                    } else {
                        "변경 사항 확인 중…".into()
                    }
                },
            )));
        }
        self.set_list(items);
        let found: Vec<_> = self
            .list_items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                ListItem::Git(change)
                    if selected
                        .iter()
                        .any(|(path, staged)| path == &change.path && *staged == change.staged) =>
                {
                    Some(index)
                }
                _ => None,
            })
            .collect();
        if let Some(&first) = found.first() {
            list_select(self.list, first);
            for row in found.into_iter().skip(1) {
                list_mark(self.list, row);
            }
        }
    }

    pub(super) fn git_action(&mut self, id: usize) -> io::Result<()> {
        match id {
            EXPLORER_VIEW | CHANGES_VIEW => self.set_git_view(id == CHANGES_VIEW),
            GRAPH_BRANCH | GRAPH_COMPARE => self.choose_graph_ref(id),
            REF_PICK_START..=REF_PICK_END => self.select_graph_ref(id - REF_PICK_START),
            REFRESH => self.request_git_refresh(),
            STAGE | UNSTAGE => {
                let stage = id == STAGE;
                let changes: Vec<_> = list_selection(self.list)
                    .into_iter()
                    .filter_map(|row| {
                        if let Some(ListItem::Git(change)) =
                            self.list_items.get(row).filter(|_| self.git.open)
                            && stage != change.staged
                        {
                            Some(change.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                if changes.is_empty() {
                    self.report(if stage {
                        "스테이지할 변경 파일을 선택하세요."
                    } else {
                        "스테이징된 파일을 선택하세요."
                    });
                } else {
                    self.start_git_action(Operation::Index { changes, stage })?;
                }
            }
            COMMIT => {
                let message = caption(self.git.message);
                if message.trim().is_empty() {
                    self.report("커밋 메시지를 입력하세요.");
                    unsafe {
                        SetFocus(self.git.message);
                    }
                } else if !self
                    .git
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.changes.iter().any(|change| change.staged))
                {
                    self.report("커밋할 파일을 먼저 스테이지하세요.");
                } else {
                    self.start_git_action(Operation::Commit(message))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn preview_git(&mut self, change: Change) -> io::Result<()> {
        self.start_git_preview(Operation::Diff(change))
    }

    fn start_git_preview(&mut self, operation: Operation) -> io::Result<()> {
        let Some(root) = self.root.clone() else {
            return Ok(());
        };
        if self.git.preview.is_some() {
            self.git.pending_preview = Some(operation);
        } else {
            let known = self
                .git
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.root.clone());
            self.git.preview = Some(spawn_job(self.window, root, known, operation)?);
        }
        Ok(())
    }

    fn check_git_preview(&mut self) {
        let Some(job) = &self.git.preview else {
            return;
        };
        let result = match job.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(io::Error::other("Git 작업이 종료되었습니다."))
            }
        };
        let relevant = self.git.open && self.root.as_ref() == Some(&job.root);
        self.git.preview = None;
        let result = if let Some(next) = self.git.pending_preview.take() {
            self.start_git_preview(next)
        } else if relevant {
            result.and_then(|reply| match reply {
                Reply::Diff(change, content) => self.open_git_diff(&change, &content),
                Reply::CommitPreview(id, subject, content) => {
                    if let Some(snapshot) = &self.git.snapshot {
                        self.open_diff_tab(
                            (snapshot.root.join(".git").join(id), false),
                            subject,
                            &content,
                        )
                    } else {
                        Ok(())
                    }
                }
                _ => Ok(()),
            })
        } else {
            Ok(())
        };
        if let Err(error) = result {
            self.show_error(error);
        }
    }

    fn start_git_action(&mut self, operation: Operation) -> io::Result<()> {
        let root = self
            .root
            .clone()
            .ok_or_else(|| io::Error::other("먼저 작업 폴더를 여세요."))?;
        match &self.git.job {
            Some(_) if self.git.pending.is_none() => {
                self.git.pending = Some(operation);
            }
            Some(_) => {
                self.report("Git 작업이 진행 중입니다.");
                return Ok(());
            }
            None => self.git.start(self.window, root, operation)?,
        }
        unsafe {
            SetTimer(self.window, TIMER, 400, 0);
        }
        Ok(())
    }

    fn open_git_diff(&mut self, change: &Change, content: &str) -> io::Result<()> {
        let label = format!(
            "{} · {}",
            file_name(&change.path),
            if change.staged {
                "스테이지"
            } else {
                "Git 변경"
            }
        );
        let path = self.git.snapshot.as_ref().map_or_else(
            || change.path.clone(),
            |snapshot| snapshot.root.join(&change.path),
        );
        self.open_diff_tab((path, change.staged), label, content)
    }

    fn open_diff_tab(
        &mut self,
        name: (PathBuf, bool),
        label: String,
        content: &str,
    ) -> io::Result<()> {
        if let Some(index) = self
            .documents
            .iter()
            .position(|doc| doc.git_diff.as_ref() == Some(&name))
        {
            let editor = self.documents[index].editor;
            set_editor_text(editor, content);
            send(editor, EM_SETMODIFY, 0, 0);
            self.documents[index].dirty = false;
            self.format(index, true);
            self.activate(index, true);
        } else {
            self.add_document(None, label, content, Encoding::default())?;
            let index = self.active.expect("diff document just added");
            let document = &mut self.documents[index];
            document.git_diff = Some(name);
            document.language = Language::Diff;
            send(document.editor, EM_SETREADONLY, 1, 0);
            self.format(index, true);
            self.update_status();
        }
        Ok(())
    }
}

fn run(workspace: &Path, known: Option<&Path>, operation: Operation) -> io::Result<Reply> {
    let program = git_path()?;
    if let Some(root) = known.filter(|_| !matches!(operation, Operation::Status)) {
        return run_at(&program, root, operation);
    }
    let located = command(&program, workspace)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !located.status.success() {
        let reason = String::from_utf8_lossy(&located.stderr);
        if reason.contains("not a git repository") {
            return Ok(Reply::Empty);
        }
        return Err(command_error(&located));
    }
    let path = String::from_utf8_lossy(&located.stdout);
    let root = PathBuf::from(path.trim_end_matches(['\r', '\n']));
    run_at(&program, &root, operation)
}

fn run_at(program: &Path, root: &Path, operation: Operation) -> io::Result<Reply> {
    match operation {
        Operation::Status => Ok(Reply::Status(status(program, root)?, false)),
        Operation::Diff(change) => {
            let mut diff = command(program, root);
            diff.args(["diff", "--no-ext-diff", "--no-textconv", "--no-color"]);
            if change.kind == b'?' {
                diff.args(["--no-index", "--", "/dev/null"])
                    .arg(&change.path);
            } else {
                if change.staged {
                    diff.arg("--cached");
                }
                diff.arg("--").arg(&change.path);
                if let Some(old) = &change.old {
                    diff.arg(old);
                }
            }
            let text = diff_output(diff, change.kind == b'?')?;
            Ok(Reply::Diff(change, text))
        }
        Operation::Index { changes, stage } => {
            let mut paths = BTreeSet::new();
            for change in changes {
                paths.insert(change.path);
                if let Some(old) = change.old {
                    paths.insert(old);
                }
            }
            let mut update = command(program, root);
            let unborn = !stage
                && !command(program, root)
                    .args(["rev-parse", "--verify", "HEAD"])
                    .output()?
                    .status
                    .success();
            if stage {
                update.args(["add", "--"]);
            } else if unborn {
                update.args(["rm", "--cached", "-f", "--ignore-unmatch", "--"]);
            } else {
                update.args(["restore", "--staged", "--"]);
            }
            let output = update.args(paths).output()?;
            if !output.status.success() {
                return Err(command_error(&output));
            }
            Ok(Reply::Status(status(program, root)?, false))
        }
        Operation::Commit(message) => {
            let output = command(program, root)
                .args(["commit", "-m", &message])
                .output()?;
            if !output.status.success() {
                return Err(command_error(&output));
            }
            Ok(Reply::Status(status(program, root)?, true))
        }
        Operation::ShowCommit(id, subject) => {
            let mut git = command(program, root);
            git.args([
                "show",
                "--stat",
                "--patch",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                &id,
                "--",
            ]);
            let content = diff_output(git, false)?;
            Ok(Reply::CommitPreview(id, subject, content))
        }
    }
}

fn diff_output(mut command: Command, differs_is_ok: bool) -> io::Result<String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped git stdout");
    let mut stderr = child.stderr.take().expect("piped git stderr");
    thread::scope(|scope| -> io::Result<_> {
        let errors = scope.spawn(move || {
            let mut messages = Vec::new();
            let _ = stderr.by_ref().take(8 * 1024).read_to_end(&mut messages);
            let _ = io::copy(&mut stderr, &mut io::sink());
            messages
        });
        let mut bytes = Vec::new();
        let read = stdout.take(MAX_DIFF as u64 + 1).read_to_end(&mut bytes);
        let truncated = bytes.len() > MAX_DIFF;
        if truncated || read.is_err() {
            let _ = child.kill();
        }
        let status = child.wait()?;
        read?;
        if !(truncated || status.success() || differs_is_ok && status.code() == Some(1)) {
            return Err(command_error(&Output {
                status,
                stdout: bytes,
                stderr: errors.join().unwrap_or_default(),
            }));
        }
        bytes.truncate(MAX_DIFF);
        let mut text = String::from_utf8_lossy(&bytes)
            .replace("\r\n", "\n")
            .replace('\0', " ");
        if truncated {
            text.push_str("\n… 변경 내용이 길어 일부만 표시했습니다.\n");
        }
        if text.is_empty() {
            text = "표시할 텍스트 변경이 없습니다.".into();
        }
        Ok(text)
    })
}

fn git_path() -> io::Result<PathBuf> {
    static GIT_PATH: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = GIT_PATH.get() {
        return Ok(path.clone());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let common: Option<OsString> = std::env::var_os("ProgramFiles");
    let path = std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .chain(common.map(|path| PathBuf::from(path).join("Git").join("cmd")))
        .map(|dir| dir.join("git.exe"))
        .find(|file| file.is_file())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Git 실행 파일을 찾을 수 없습니다.")
        })?;
    let _ = GIT_PATH.set(path.clone());
    Ok(path)
}

fn command(program: &Path, root: &Path) -> Command {
    let mut git = Command::new(program);
    git.creation_flags(0x08000000)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .args([
            "--no-pager",
            "--no-optional-locks",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-C",
        ])
        .arg(root);
    git
}

fn status(program: &Path, root: &Path) -> io::Result<Snapshot> {
    let output = command(program, root)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--branch",
            "--untracked-files=all",
            "--renames",
        ])
        .output()?;
    if !output.status.success() {
        return Err(command_error(&output));
    }
    Ok(parse_status(root, &output.stdout))
}

fn parse_status(root: &Path, bytes: &[u8]) -> Snapshot {
    let mut entries = bytes.split(|byte| *byte == 0);
    let mut branch = "HEAD".to_owned();
    let mut changes = Vec::new();
    while let Some(entry) = entries.next() {
        if let Some(raw) = entry.strip_prefix(b"## ") {
            let attempt = String::from_utf8_lossy(raw);
            let label = attempt
                .strip_prefix("No commits yet on ")
                .unwrap_or(&attempt);
            branch = label.split("...").next().unwrap_or(label).to_owned();
            continue;
        }
        if entry.len() < 4 || entry[2] != b' ' {
            continue;
        }
        let (staged, work) = (entry[0], entry[1]);
        let renamed = matches!(staged, b'R' | b'C') || matches!(work, b'R' | b'C');
        let old = if renamed {
            entries.next().and_then(git_relative_path)
        } else {
            None
        };
        let Some(path) = git_relative_path(&entry[3..]) else {
            continue;
        };
        if staged == b'?' && work == b'?' {
            changes.push(Change {
                path,
                old: None,
                kind: b'?',
                staged: false,
            });
        } else if matches!(
            (staged, work),
            (b'D', b'D') | (b'A', b'A') | (b'U', _) | (_, b'U')
        ) {
            changes.push(Change {
                path,
                old,
                kind: b'U',
                staged: false,
            });
        } else {
            if staged != b' ' {
                changes.push(Change {
                    path: path.clone(),
                    old: old.clone(),
                    kind: staged,
                    staged: true,
                });
            }
            if work != b' ' {
                changes.push(Change {
                    path,
                    old,
                    kind: work,
                    staged: false,
                });
            }
        }
    }
    Snapshot {
        root: root.to_owned(),
        branch,
        changes,
    }
}

fn git_relative_path(bytes: &[u8]) -> Option<PathBuf> {
    let path = PathBuf::from(std::str::from_utf8(bytes).ok()?);
    (!path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_))))
    .then_some(path)
}

fn command_error(output: &Output) -> io::Error {
    let raw = String::from_utf8_lossy(&output.stderr);
    let raw = if raw.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout)
    } else {
        raw
    };
    let message: String = raw.trim().chars().take(950).collect();
    io::Error::other(if message.is_empty() {
        format!("Git 작업 실패 ({})", output.status)
    } else {
        message
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_staged_unstaged_and_rename_paths() {
        let snapshot = parse_status(Path::new("C:\\repo"),
            "## main...origin/main [ahead 1]\0MM 한글.txt\0R  new file.rs\0old file.rs\0?? space name.ts\0".as_bytes());
        assert_eq!(snapshot.branch, "main");
        assert_eq!(snapshot.changes.len(), 4);
        assert_eq!(snapshot.changes[0].path, Path::new("한글.txt"));
        assert!(snapshot.changes[0].staged && !snapshot.changes[1].staged);
        assert_eq!(
            snapshot.changes[2].old.as_deref(),
            Some(Path::new("old file.rs"))
        );
        assert_eq!(snapshot.changes[3].kind, b'?');
    }
}
