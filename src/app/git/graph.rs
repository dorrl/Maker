use super::*;
use std::collections::HashMap;

const HEADER: usize = 375;
const LIMIT: usize = 160;
const VISIBLE_LANES: usize = 6;

#[derive(Clone, PartialEq, Eq)]
struct Branch {
    name: String,
    short: String,
    upstream: String,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct Query {
    branch: Option<String>,
    comparison: Comparison,
}

#[derive(Clone, Default, PartialEq, Eq)]
enum Comparison {
    #[default]
    Automatic,
    None,
    Ref(String),
}

#[derive(PartialEq, Eq)]
struct Commit {
    id: String,
    parents: Vec<String>,
    subject: String,
    author: String,
    labels: String,
}

#[derive(PartialEq, Eq)]
struct Segment {
    from: usize,
    to: usize,
    color: usize,
    join: bool,
}

#[derive(PartialEq, Eq)]
struct Row {
    commit: Commit,
    lane: usize,
    color: usize,
    before: bool,
    edges: Vec<Segment>,
}

#[derive(Default, PartialEq, Eq)]
struct History {
    branches: Vec<Branch>,
    rows: Vec<Row>,
    width: usize,
}

struct Work {
    root: PathBuf,
    query: Query,
    result: mpsc::Receiver<io::Result<History>>,
}

pub(in crate::app) struct Graph {
    pub list: Handle,
    pub branch: Handle,
    pub compare: Handle,
    header: Handle,
    query: Query,
    history: History,
    work: Option<Work>,
    refresh: Instant,
    notice: Option<String>,
    picker: Option<(usize, Vec<String>)>,
}

impl Graph {
    pub(super) fn new(window: Handle) -> io::Result<Self> {
        let button = |id, label| -> io::Result<_> {
            let control = child(window, id, "BUTTON", label, BS_OWNERDRAW | WS_TABSTOP)?;
            chrome::watch_control(control, true);
            Ok(control)
        };
        let list = child(
            window,
            GRAPH_LIST,
            "LISTBOX",
            "",
            LBS_NOTIFY
                | LBS_OWNERDRAWFIXED
                | LBS_HASSTRINGS
                | LBS_NOINTEGRALHEIGHT
                | LBS_DISABLENOSCROLL
                | LBS_EXTENDEDSEL
                | WS_VSCROLL
                | WS_TABSTOP,
        )?;
        chrome::watch_control(list, false);
        Ok(Self {
            list,
            branch: button(GRAPH_BRANCH, "커밋 · HEAD ▾")?,
            compare: button(GRAPH_COMPARE, "비교 · 자동 ▾")?,
            header: child(
                window,
                HEADER,
                "STATIC",
                "커밋 그래프",
                SS_CENTERIMAGE | SS_NOPREFIX,
            )?,
            query: Query::default(),
            history: History::default(),
            work: None,
            refresh: Instant::now(),
            notice: None,
            picker: None,
        })
    }

    pub(super) fn reset(&mut self) {
        self.query = Query::default();
        self.history = History::default();
        self.notice = None;
        self.picker = None;
        self.work = None;
        self.populate();
        self.invalidate();
    }

    pub(super) fn invalidate(&mut self) {
        self.refresh = Instant::now();
    }

    pub(in crate::app) fn fonts(&self, paint: Paint, row_height: i32) {
        for window in [self.header, self.branch, self.compare, self.list] {
            set_font(window, paint.font);
        }
        send(self.list, LB_SETITEMHEIGHT, 0, row_height as isize);
    }

    fn populate(&self) {
        let display = |name: &str| {
            self.history
                .branches
                .iter()
                .find(|branch| branch.name == name)
                .map_or_else(|| name.to_owned(), |branch| branch.short.clone())
        };
        let branch = self
            .query
            .branch
            .as_deref()
            .map_or("HEAD".to_owned(), display);
        let compare = match &self.query.comparison {
            Comparison::Automatic => "자동".into(),
            Comparison::None => "없음".into(),
            Comparison::Ref(name) => display(name),
        };
        title(self.branch, &format!("커밋 · {branch} ▾"));
        title(self.compare, &format!("비교 · {compare} ▾"));
        chrome::reset_file_motion(self.list);
        let top = send(self.list, LB_GETTOPINDEX, 0, 0).max(0) as usize;
        let selected = list_caret(self.list);
        painting(self.list, false);
        send(self.list, LB_RESETCONTENT, 0, 0);
        if self.history.rows.is_empty() {
            list_entry(
                self.list,
                self.notice.as_deref().unwrap_or("표시할 커밋이 없습니다."),
                0,
            );
        }
        for (index, row) in self.history.rows.iter().enumerate() {
            list_entry(self.list, &row.commit.subject, index + 1);
        }
        if let Some(row) = selected.filter(|&row| row < self.history.rows.len()) {
            list_select(self.list, row);
        }
        send(
            self.list,
            LB_SETTOPINDEX,
            top.min(self.history.rows.len().saturating_sub(1)),
            0,
        );
        painting(self.list, true);
        repaint(self.list);
    }
}

impl App {
    pub(in crate::app) fn layout_git_graph(&self, area: Rect, changes_top: i32) -> i32 {
        let graph = &self.git.graph;
        let available = area.bottom - changes_top;
        let visible =
            self.sidebar && self.git.open && self.git.has_repository() && available >= self.px(175);
        let height = (available / 2).max(self.px(115));
        let top = area.bottom - height;
        let pad = self.px(6);
        let row = self.px(25);
        let button_width = (area.width() - 3 * pad).max(0) / 2;
        place(
            graph.header,
            area.left + pad,
            top + self.px(2),
            area.width() - 2 * pad,
            row,
        );
        place(graph.branch, area.left + pad, top + row, button_width, row);
        place(
            graph.compare,
            area.left + 2 * pad + button_width,
            top + row,
            button_width,
            row,
        );
        let body = top + row * 2 + pad;
        place(
            graph.list,
            area.left,
            body,
            area.width(),
            area.bottom - body,
        );
        for window in [graph.header, graph.branch, graph.compare, graph.list] {
            show(window, visible);
        }
        if visible { top - pad } else { area.bottom }
    }

    pub(in crate::app) fn graph_visible(&self) -> bool {
        let graph = &self.git.graph;
        self.sidebar
            && self.git.open
            && self.git.has_repository()
            && bounds(graph.list).height() > 0
            && unsafe { IsWindowVisible(graph.list) } != 0
    }

    pub(super) fn check_git_graph(&mut self) {
        let graph = &mut self.git.graph;
        if let Some(work) = &graph.work {
            match work.result.try_recv() {
                Err(mpsc::TryRecvError::Empty) => return,
                result => {
                    let relevant = self
                        .git
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.root == work.root)
                        && graph.query == work.query;
                    graph.work = None;
                    if relevant {
                        let result = result.unwrap_or_else(|_| {
                            Err(io::Error::other("그래프 작업이 종료되었습니다."))
                        });
                        match result {
                            Ok(history) => {
                                if graph.history != history || graph.notice.is_some() {
                                    graph.history = history;
                                    graph.notice = None;
                                    graph.populate();
                                }
                            }
                            Err(error) => {
                                let notice = error.to_string();
                                if graph.notice.as_ref() != Some(&notice) {
                                    graph.notice = Some(notice);
                                    graph.history.rows.clear();
                                    graph.populate();
                                }
                            }
                        }
                    }
                }
            }
        }
        if !self.sidebar || !self.git.open || Instant::now() < graph.refresh {
            return;
        }
        let Some(snapshot) = &self.git.snapshot else {
            return;
        };
        let window = self.window;
        let root = snapshot.root.clone();
        let query = graph.query.clone();
        let (send, result) = mpsc::channel();
        let path = root.clone();
        let criteria = query.clone();
        graph.refresh = Instant::now() + Duration::from_secs(12);
        if thread::Builder::new()
            .name("maker-graph".into())
            .spawn(move || {
                let _ = send.send(history(&path, &criteria));
                unsafe {
                    PostMessageW(window, READY, 0, 0);
                }
            })
            .is_ok()
        {
            graph.work = Some(Work {
                root,
                query,
                result,
            });
        }
    }

    pub(super) fn choose_graph_ref(&mut self, button: usize) {
        let graph = &mut self.git.graph;
        let mut names = vec![String::new()];
        let mut options = vec![if button == GRAPH_BRANCH {
            "HEAD · 현재 브랜치".to_owned()
        } else {
            "자동 · 업스트림".to_owned()
        }];
        if button == GRAPH_COMPARE {
            options.push("비교 안 함".into());
            names.push(String::new());
        }
        for branch in graph
            .history
            .branches
            .iter()
            .take(REF_PICK_END - REF_PICK_START - 1)
        {
            names.push(branch.name.clone());
            options.push(branch.short.clone());
        }
        graph.picker = Some((button, names));
        let rows: Vec<_> = options
            .iter()
            .enumerate()
            .map(|(i, name)| (REF_PICK_START + i, name.as_str()))
            .collect();
        self.open_menu(button, &rows);
    }

    pub(super) fn select_graph_ref(&mut self, index: usize) {
        let graph = &mut self.git.graph;
        let Some((button, names)) = graph.picker.take() else {
            return;
        };
        let Some(name) = names.get(index) else {
            return;
        };
        if button == GRAPH_BRANCH {
            graph.query.branch = (!name.is_empty()).then(|| name.clone());
        } else {
            graph.query.comparison = match index {
                0 => Comparison::Automatic,
                1 => Comparison::None,
                _ => Comparison::Ref(name.clone()),
            };
        }
        graph.invalidate();
        graph.populate();
        self.check_git_graph();
    }

    pub(in crate::app) fn open_graph_commit(&mut self) -> io::Result<()> {
        let graph = &self.git.graph;
        if let Some(row) = list_caret(graph.list).and_then(|index| graph.history.rows.get(index)) {
            let id = row.commit.id.clone();
            let short = &id[..id.len().min(8)];
            let subject = format!("{short} · {}", row.commit.subject);
            self.start_git_preview(Operation::ShowCommit(id, subject))?;
        }
        Ok(())
    }

    pub(in crate::app) fn paint_graph_row(&self, item: &DrawItem, paint: Paint) {
        fill_color(item.dc, item.rect, chrome::file_row_tint(item, paint));
        if item.item == u32::MAX {
            return;
        }
        let graph = &self.git.graph;
        let x =
            |lane: usize| item.rect.left + self.px(11 + lane.min(VISIBLE_LANES - 1) as i32 * 10);
        let mid = (item.rect.top + item.rect.bottom) / 2;
        let Some(row) = graph.history.rows.get((item.item) as usize) else {
            let text = list_label(item.window, item.item as usize);
            draw_label(
                item.dc,
                Rect {
                    left: x(0),
                    ..item.rect
                },
                &text,
                paint.font,
                paint.muted,
                false,
            );
            return;
        };
        let color = |i: usize| paint.icons[1 + (i % 5)];
        let node = x(row.lane);
        if row.before {
            stroke_line(
                item.dc,
                Point {
                    x: node,
                    y: item.rect.top,
                },
                Point { x: node, y: mid },
                color(row.color),
            );
        }
        for edge in &row.edges {
            stroke_line(
                item.dc,
                Point {
                    x: x(edge.from),
                    y: if edge.join { mid } else { item.rect.top },
                },
                Point {
                    x: x(edge.to),
                    y: item.rect.bottom,
                },
                color(edge.color),
            );
        }
        let dot = self.px(3).max(2);
        fill_color(
            item.dc,
            Rect::from_size(node - dot, mid - dot, dot * 2, dot * 2),
            color(row.color),
        );
        let left = x(graph.history.width.saturating_sub(1)) + self.px(10);
        let right = item.rect.right - self.px(6);
        let badge = if row.commit.labels.is_empty() {
            0
        } else {
            self.px(96).min((right - left).max(0) / 2)
        };
        let label = Rect {
            left,
            right: right - badge,
            ..item.rect
        };
        let text = format!("{}  ·  {}", row.commit.subject, row.commit.author);
        draw_label(item.dc, label, &text, paint.font, paint.foreground, false);
        if badge > 0 {
            let badge_rect = Rect {
                left: right - badge + self.px(3),
                right,
                top: item.rect.top + self.px(3),
                bottom: item.rect.bottom - self.px(3),
            };
            fill(item.dc, badge_rect, paint.control);
            draw_label(
                item.dc,
                Rect {
                    left: badge_rect.left + self.px(4),
                    ..badge_rect
                },
                &row.commit.labels,
                paint.font,
                color(row.color),
                false,
            );
        }
    }
}

fn history(root: &Path, query: &Query) -> io::Result<History> {
    let program = git_path()?;
    let output = command(&program, root)
        .args([
            "for-each-ref",
            "--format=%(refname)%00%(refname:short)%00%(upstream)",
            "refs/heads",
            "refs/remotes",
        ])
        .output()?;
    if !output.status.success() {
        return Err(command_error(&output));
    }
    let branch_text = String::from_utf8_lossy(&output.stdout);
    let branches: Vec<_> = branch_text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\0');
            let name = fields.next()?;
            let short = fields.next()?;
            let upstream = fields.next()?;
            Some(Branch {
                name: name.into(),
                short: short.into(),
                upstream: upstream.into(),
            })
        })
        .collect();
    let head_name = if query.branch.is_none() && query.comparison == Comparison::Automatic {
        let output = command(&program, root)
            .args(["symbolic-ref", "-q", "HEAD"])
            .output()?;
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        String::new()
    };
    let branch = query
        .branch
        .as_deref()
        .and_then(|name| branches.iter().find(|branch| branch.name == name));
    let compare = match &query.comparison {
        Comparison::Automatic => branch
            .or_else(|| branches.iter().find(|branch| branch.name == head_name))
            .map(|branch| branch.upstream.as_str()),
        Comparison::None => None,
        Comparison::Ref(name) => Some(name.as_str()),
    };
    let base = branch.map_or("HEAD", |branch| branch.name.as_str());
    let mut log = command(&program, root);
    log.args([
        "log",
        "--topo-order",
        "--decorate=short",
        "--encoding=UTF-8",
        "-z",
        "--format=%H%x00%P%x00%s%x00%an%x00%D",
        "-n",
        &LIMIT.to_string(),
        base,
    ]);
    if let Some(reference) =
        compare.filter(|name| *name != base && branches.iter().any(|branch| branch.name == *name))
    {
        log.arg(reference);
    }
    let output = log.arg("--").output()?;
    if !output.status.success() {
        if base == "HEAD"
            && !command(&program, root)
                .args(["rev-parse", "--verify", "HEAD"])
                .output()?
                .status
                .success()
        {
            return Ok(History {
                branches,
                ..History::default()
            });
        }
        return Err(command_error(&output));
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let fields: Vec<_> = raw.split_terminator('\0').collect();
    let commits = fields.as_chunks::<5>().0.iter().filter_map(|field| {
        let id = field[0];
        if !matches!(id.len(), 40 | 64) || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some(Commit {
            id: id.into(),
            parents: field[1]
                .split_ascii_whitespace()
                .map(str::to_owned)
                .collect(),
            subject: field[2].into(),
            author: field[3].into(),
            labels: field[4].replace("HEAD -> ", "").replace(", ", " · "),
        })
    });
    let (rows, width) = layout(commits);
    Ok(History {
        branches,
        rows,
        width,
    })
}

fn layout(commits: impl Iterator<Item = Commit>) -> (Vec<Row>, usize) {
    let mut lanes: Vec<(String, usize)> = Vec::new();
    let mut next_color = 0;
    let mut width = 1;
    let mut rows = Vec::new();
    for commit in commits {
        let existing = lanes.iter().position(|(id, _)| *id == commit.id);
        let lane = existing.unwrap_or(lanes.len());
        let color = if let Some(index) = existing {
            lanes[index].1
        } else {
            let id = next_color;
            next_color += 1;
            lanes.push((commit.id.clone(), id));
            id
        };
        let previous = lanes.clone();
        lanes.remove(lane);
        for (index, id) in commit.parents.iter().enumerate() {
            if lanes.iter().all(|known| known.0 != *id) {
                let parent_color = if index == 0 {
                    color
                } else {
                    let next = next_color;
                    next_color += 1;
                    next
                };
                lanes.insert((lane + index).min(lanes.len()), (id.clone(), parent_color));
            }
        }
        let positions: HashMap<_, _> = lanes
            .iter()
            .enumerate()
            .map(|(i, (id, color))| (id, (i, *color)))
            .collect();
        let mut edges: Vec<_> = previous
            .iter()
            .enumerate()
            .filter_map(|(from, (id, _))| {
                let (to, color) = *positions.get(id)?;
                Some(Segment {
                    from,
                    to,
                    color,
                    join: false,
                })
            })
            .collect();
        edges.extend(commit.parents.iter().filter_map(|id| {
            let (to, color) = *positions.get(id)?;
            Some(Segment {
                from: lane,
                to,
                color,
                join: true,
            })
        }));
        width = width.max(lanes.len()).max(previous.len());
        rows.push(Row {
            commit,
            lane,
            color,
            edges,
            before: existing.is_some(),
        });
    }
    (rows, width)
}
