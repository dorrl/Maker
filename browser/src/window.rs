use std::{
    cell::RefCell,
    ffi::c_void,
    io,
    mem::size_of,
    path::PathBuf,
    ptr::{null, null_mut},
    sync::Arc,
    time::Instant,
};

use blitz_traits::net::Url;
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{Dialogs::*, *},
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    w,
};

use crate::{
    net::{self, Loading, Network},
    page::Page,
    tabs::Tab,
};

const CLASS: PCWSTR = w!("MakerLightBrowser");
const TITLE: PCWSTR = w!("Maker Browser");
const WAKE: u32 = WM_APP + 1;
const PICK_FILE: u32 = WM_APP + 2;
const EXPIRY_TIMER: usize = 1;
const BACK: usize = 101;
const FORWARD: usize = 102;
const RELOAD: usize = 103;
const OPEN: usize = 104;
const NEW: usize = 105;
const CLOSE: usize = 106;
const ADDRESS: usize = 107;
const TABS: usize = 108;
const CONTENT_TOP: i32 = 70;
const STATUS_HEIGHT: i32 = 24;
const MAX_TABS: usize = 32;

type PCWSTR = *const u16;

thread_local! {
    static BROWSER: RefCell<Option<Browser>> = const { RefCell::new(None) };
}

enum DocumentState {
    Loading(Loading),
    Ready(Box<Page>),
    Failed(String),
}

struct LivePage {
    generation: usize,
    state: DocumentState,
}

struct Browser {
    window: HWND,
    tab_strip: HWND,
    address: HWND,
    status: HWND,
    buttons: [HWND; 6],
    tabs: Vec<Tab<LivePage>>,
    active: usize,
    next_tab: u64,
    generation: usize,
    network: Network,
    frame: Option<Frame>,
}

struct Frame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

pub fn run() -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .max_blocking_threads(2)
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let network = Network::new(runtime.handle().clone()).map_err(|error| error.to_string())?;
    let window = create_window().map_err(|error| error.to_string())?;
    let mut browser = Browser::new(window, network).map_err(|error| error.to_string())?;
    let argument = std::env::args().nth(1);
    let initial = argument.as_deref().map(net::address).transpose();
    browser.add_tab(
        initial
            .as_ref()
            .ok()
            .and_then(|url| url.clone())
            .unwrap_or_else(net::blank),
    );
    if let Err(error) = initial {
        browser.set_status(&error);
    }
    BROWSER.with(|state| state.replace(Some(browser)));
    with_browser(|browser| browser.layout());
    unsafe {
        ShowWindow(window, SW_SHOWNORMAL);
        UpdateWindow(window);
    }
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result <= 0 {
            BROWSER.with(|state| state.replace(None));
            if result < 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            runtime.shutdown_background();
            return Ok(());
        }
        let mut handled = false;
        if matches!(message.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
            with_browser(|browser| handled = browser.key(message.hwnd, message.wParam));
        }
        if !handled {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

pub fn show_error(error: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(error).as_ptr(),
            TITLE,
            MB_OK | MB_ICONERROR,
        );
    }
}

fn with_browser(action: impl FnOnce(&mut Browser)) {
    BROWSER.with(|state| {
        if let Ok(mut slot) = state.try_borrow_mut()
            && let Some(browser) = slot.as_mut()
        {
            action(browser);
        }
    });
}

fn create_window() -> io::Result<HWND> {
    unsafe {
        SetProcessDPIAware();
        let controls = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_TAB_CLASSES,
        };
        InitCommonControlsEx(&controls);
        let instance = GetModuleHandleW(null());
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(io::Error::last_os_error());
        }
        checked(CreateWindowExW(
            0,
            CLASS,
            TITLE,
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            960,
            680,
            null_mut(),
            null_mut(),
            instance,
            null(),
        ))
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_DESTROY => unsafe { PostQuitMessage(0) },
        WM_SIZE => with_browser(|browser| browser.layout()),
        WM_TIMER if wparam == EXPIRY_TIMER => with_browser(|browser| browser.sweep()),
        WAKE => with_browser(|browser| browser.wake(wparam)),
        PICK_FILE => {
            if let Some(path) = pick_file(window) {
                with_browser(|browser| match Url::from_file_path(path) {
                    Ok(url) => browser.navigate(url),
                    Err(_) => browser.set_status("올바른 로컬 경로가 아닙니다."),
                });
            }
        }
        WM_COMMAND if lparam != 0 && wparam >> 16 == 0 => {
            with_browser(|browser| browser.command(wparam & 0xffff));
        }
        WM_NOTIFY if lparam != 0 => {
            let notice = unsafe { &*(lparam as *const NMHDR) };
            if notice.idFrom == TABS && notice.code == TCN_SELCHANGE {
                with_browser(|browser| {
                    let index = unsafe { SendMessageW(browser.tab_strip, TCM_GETCURSEL, 0, 0) };
                    if index >= 0 {
                        browser.select(index as usize);
                    }
                });
            }
        }
        WM_LBUTTONUP => with_browser(|browser| {
            browser.click(i32::from(lparam as i16), i32::from((lparam >> 16) as i16));
        }),
        WM_MOUSEWHEEL => with_browser(|browser| {
            browser.scroll(-f64::from((wparam >> 16) as i16) * 0.75);
        }),
        WM_PAINT => unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(window, &mut paint);
            with_browser(|browser| browser.paint(dc));
            EndPaint(window, &paint);
        },
        WM_PRINTCLIENT => with_browser(|browser| browser.paint(wparam as HDC)),
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

impl Browser {
    fn new(window: HWND, network: Network) -> io::Result<Self> {
        let tab_strip = control(window, 0, w!("SysTabControl32"), "", WS_TABSTOP, TABS)?;
        let address = control(
            window,
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            "",
            WS_TABSTOP | ES_AUTOHSCROLL as u32,
            ADDRESS,
        )?;
        let status = control(window, 0, w!("STATIC"), "", 0, 0)?;
        let mut buttons = [null_mut(); 6];
        for (button, (label, id)) in buttons.iter_mut().zip([
            ("뒤로", BACK),
            ("앞으로", FORWARD),
            ("새로고침", RELOAD),
            ("파일 열기", OPEN),
            ("+", NEW),
            ("×", CLOSE),
        ]) {
            *button = control(window, 0, w!("BUTTON"), label, WS_TABSTOP, id)?;
        }
        Ok(Self {
            window,
            tab_strip,
            address,
            status,
            buttons,
            tabs: Vec::new(),
            active: 0,
            next_tab: 0,
            generation: 0,
            network,
            frame: None,
        })
    }

    fn add_tab(&mut self, url: Url) {
        if self.tabs.len() >= MAX_TABS {
            self.set_status("탭은 최대 32개까지 열 수 있습니다.");
            return;
        }
        self.next_tab += 1;
        self.tabs.push(Tab::new(self.next_tab, url, Instant::now()));
        self.select(self.tabs.len() - 1);
    }

    fn select(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let now = Instant::now();
        if let Some(selected) = self.tabs.get(self.active) {
            let old_id = selected.id;
            self.tabs[index].unload_if_expired(old_id, now);
        }
        self.active = index;
        self.tabs[index].select(now);
        self.sweep();
        self.sync_address();
        self.load_active();
        self.refresh();
    }

    fn sweep(&mut self) {
        let active_id = self.tabs[self.active].id;
        let now = Instant::now();
        let mut changed = false;
        for tab in &mut self.tabs {
            changed |= tab.unload_if_expired(active_id, now);
        }
        let next = self
            .tabs
            .iter()
            .filter_map(|tab| tab.deadline(active_id))
            .min();
        unsafe {
            KillTimer(self.window, EXPIRY_TIMER);
            if let Some(next) = next {
                let delay = next.saturating_duration_since(Instant::now());
                SetTimer(
                    self.window,
                    EXPIRY_TIMER,
                    (delay.as_millis() as u32 + 1).max(10),
                    None,
                );
            }
        }
        if changed {
            self.sync_tabs();
        }
    }

    fn close_active(&mut self) {
        self.sweep();
        self.tabs.remove(self.active);
        if self.tabs.is_empty() {
            self.add_tab(net::blank());
        } else {
            self.select(self.active.min(self.tabs.len() - 1));
        }
    }

    fn navigate(&mut self, url: Url) {
        self.tabs[self.active].navigate(url);
        self.tabs[self.active].select(Instant::now());
        self.sync_address();
        self.load_active();
        self.refresh();
    }

    fn load_active(&mut self) {
        let tab = &mut self.tabs[self.active];
        if tab.page.is_none() && tab.url().as_str() != "about:blank" {
            self.generation += 1;
            let generation = self.generation;
            let window = self.window as usize;
            let wake = Arc::new(move || unsafe {
                PostMessageW(window as HWND, WAKE, generation, 0);
            });
            let loading = self.network.open(tab.url().clone(), wake);
            tab.page = Some(LivePage {
                generation,
                state: DocumentState::Loading(loading),
            });
        }
    }

    fn wake(&mut self, generation: usize) {
        if self.tabs[self.active]
            .page
            .as_ref()
            .is_some_and(|page| page.generation == generation)
        {
            self.finish_loading();
            self.update_status();
            self.invalidate_content();
        }
    }

    fn finish_loading(&mut self) {
        let tab = &mut self.tabs[self.active];
        let fragment = tab.url().fragment().map(str::to_owned);
        let Some(live) = tab.page.as_mut() else {
            return;
        };
        let DocumentState::Loading(loading) = &live.state else {
            return;
        };
        let result = match loading.receive.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("페이지 로드가 중단되었습니다.".into())
            }
        };
        let DocumentState::Loading(loading) =
            std::mem::replace(&mut live.state, DocumentState::Failed(String::new()))
        else {
            unreachable!("state checked above");
        };
        let page = match result {
            Ok(mut response) => {
                if response.url.fragment().is_none() {
                    response.url.set_fragment(fragment.as_deref());
                }
                Page::new(response, loading.requests)
            }
            Err(error) => {
                live.state = DocumentState::Failed(error);
                return;
            }
        };
        let url = page.url.clone();
        tab.title = page.title();
        live.state = DocumentState::Ready(Box::new(page));
        tab.set_resolved_url(url);
        self.sync_address();
        self.sync_tabs();
    }

    fn refresh(&mut self) {
        self.finish_loading();
        self.sync_tabs();
        self.update_status();
        self.invalidate_content();
    }

    fn update_status(&self) {
        let tab = &self.tabs[self.active];
        let status = match tab.page.as_ref().map(|live| &live.state) {
            Some(DocumentState::Loading(_)) => "불러오는 중…",
            Some(DocumentState::Ready(_)) => tab.url().as_str(),
            Some(DocumentState::Failed(error)) => error,
            None => "주소를 입력하거나 HTML 파일을 여세요.",
        };
        self.set_status(status);
    }

    fn key(&mut self, target: HWND, key: usize) -> bool {
        let pressed = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key)) < 0 };
        if pressed(VK_CONTROL) {
            match key as u32 {
                0x54 => {
                    self.command(NEW);
                    return true;
                }
                0x57 => {
                    self.command(CLOSE);
                    return true;
                }
                0x4f => {
                    self.command(OPEN);
                    return true;
                }
                0x4c => {
                    self.focus_address();
                    return true;
                }
                key if key == u32::from(VK_TAB) => {
                    let step = if pressed(VK_SHIFT) {
                        self.tabs.len() - 1
                    } else {
                        1
                    };
                    self.select((self.active + step) % self.tabs.len());
                    return true;
                }
                _ => {}
            }
        }
        let key = key as VIRTUAL_KEY;
        match key {
            VK_RETURN if target == self.address => {
                match net::address(&caption(self.address)) {
                    Ok(url) => self.navigate(url),
                    Err(error) => self.set_status(&error),
                }
                true
            }
            VK_F5 => {
                self.command(RELOAD);
                true
            }
            VK_LEFT if pressed(VK_MENU) => {
                self.command(BACK);
                true
            }
            VK_RIGHT if pressed(VK_MENU) => {
                self.command(FORWARD);
                true
            }
            VK_UP | VK_DOWN | VK_PRIOR | VK_NEXT if target == self.window => {
                self.scroll(match key {
                    VK_UP => -45.0,
                    VK_DOWN => 45.0,
                    VK_PRIOR => -450.0,
                    _ => 450.0,
                });
                true
            }
            _ => false,
        }
    }

    fn command(&mut self, id: usize) {
        match id {
            NEW => {
                self.add_tab(net::blank());
                self.focus_address();
            }
            CLOSE => self.close_active(),
            OPEN => unsafe {
                PostMessageW(self.window, PICK_FILE, 0, 0);
            },
            BACK | FORWARD => {
                let offset = if id == BACK { -1 } else { 1 };
                if self.tabs[self.active].move_history(offset) {
                    self.select(self.active);
                }
            }
            RELOAD => {
                let url = self.tabs[self.active].url().clone();
                self.navigate(url);
            }
            _ => {}
        }
    }

    fn sync_tabs(&self) {
        unsafe {
            SendMessageW(self.tab_strip, TCM_DELETEALLITEMS, 0, 0);
        }
        for (index, tab) in self.tabs.iter().enumerate() {
            let label = if !tab.title.is_empty() {
                tab.title.clone()
            } else {
                tab.url()
                    .host_str()
                    .unwrap_or_else(|| {
                        tab.url()
                            .path_segments()
                            .and_then(Iterator::last)
                            .filter(|name| !name.is_empty())
                            .unwrap_or("새 탭")
                    })
                    .to_owned()
            };
            let sleeping = tab.page.is_none() && tab.url().as_str() != "about:blank";
            let label: String = label.chars().take(22).collect();
            let mut label = wide(&if sleeping {
                format!("{label} · 절전")
            } else {
                label
            });
            let item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: label.as_mut_ptr(),
                ..Default::default()
            };
            unsafe {
                SendMessageW(
                    self.tab_strip,
                    TCM_INSERTITEMW,
                    index,
                    &item as *const _ as LPARAM,
                );
            }
        }
        unsafe {
            SendMessageW(self.tab_strip, TCM_SETCURSEL, self.active, 0);
        }
    }

    fn sync_address(&self) {
        let url = self.tabs[self.active].url();
        let display = url
            .to_file_path()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                if url.as_str() == "about:blank" {
                    String::new()
                } else {
                    url.to_string()
                }
            });
        unsafe {
            SetWindowTextW(self.address, wide(&display).as_ptr());
        }
    }

    fn set_status(&self, text: &str) {
        unsafe {
            SetWindowTextW(self.status, wide(text).as_ptr());
        }
    }

    fn focus_address(&self) {
        unsafe {
            SetFocus(self.address);
            SendMessageW(self.address, EM_SETSEL, 0, -1);
        }
    }

    fn layout(&mut self) {
        let area = client_rect(self.window);
        let width = area.right;
        let height = area.bottom;
        place(self.tab_strip, 8, 5, (width - 88).max(1), 27);
        place(self.buttons[4], width - 72, 5, 28, 25);
        place(self.buttons[5], width - 39, 5, 28, 25);
        place(self.buttons[0], 8, 37, 43, 26);
        place(self.buttons[1], 55, 37, 50, 26);
        place(self.buttons[2], 109, 37, 67, 26);
        place(self.buttons[3], 180, 37, 66, 26);
        place(self.address, 251, 37, (width - 260).max(1), 26);
        place(
            self.status,
            9,
            height - STATUS_HEIGHT + 3,
            (width - 18).max(1),
            STATUS_HEIGHT - 3,
        );
        self.invalidate_content();
    }

    fn content_rect(&self) -> RECT {
        let mut rect = client_rect(self.window);
        rect.top = CONTENT_TOP;
        rect.bottom = (rect.bottom - STATUS_HEIGHT).max(rect.top);
        rect
    }

    fn invalidate_content(&mut self) {
        self.frame = None;
        unsafe {
            InvalidateRect(self.window, &self.content_rect(), 0);
        }
    }

    fn ready_page(&mut self) -> Option<&mut Page> {
        match &mut self.tabs[self.active].page.as_mut()?.state {
            DocumentState::Ready(page) => Some(page),
            _ => None,
        }
    }

    fn click(&mut self, x: i32, y: i32) {
        let area = self.content_rect();
        if y < area.top || y >= area.bottom {
            return;
        }
        unsafe {
            SetFocus(self.window);
        }
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        let x = x as f32 * frame.width as f32 / (area.right - area.left) as f32;
        let y = (y - area.top) as f32 * frame.height as f32 / (area.bottom - area.top) as f32;
        if let Some(page) = self.ready_page() {
            if let Some(url) = page.click(x, y) {
                self.navigate(url);
            } else {
                self.invalidate_content();
            }
        }
    }

    fn scroll(&mut self, delta: f64) {
        if let Some(page) = self.ready_page() {
            page.scroll(delta);
            self.invalidate_content();
        }
    }

    fn paint(&mut self, dc: HDC) {
        let area = self.content_rect();
        let width = area.right - area.left;
        let height = area.bottom - area.top;
        if width <= 0 || height <= 0 {
            return;
        }
        if self.frame.is_none()
            && let Some(page) = self.ready_page()
        {
            let scale = (1600.0 / f64::from(width))
                .min(1000.0 / f64::from(height))
                .min(1.0);
            let width = (f64::from(width) * scale).round().max(1.0) as u32;
            let height = (f64::from(height) * scale).round().max(1.0) as u32;
            let mut pixels = page.render(width, height);
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            self.frame = Some(Frame {
                width,
                height,
                pixels,
            });
        }
        if let Some(frame) = &self.frame {
            let bitmap = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: frame.width as i32,
                    biHeight: -(frame.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            unsafe {
                StretchDIBits(
                    dc,
                    area.left,
                    area.top,
                    width,
                    height,
                    0,
                    0,
                    frame.width as i32,
                    frame.height as i32,
                    frame.pixels.as_ptr().cast::<c_void>(),
                    &bitmap,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
            }
        } else {
            unsafe {
                FillRect(dc, &area, GetSysColorBrush(COLOR_WINDOW));
            }
        }
    }
}

fn control(
    parent: HWND,
    ex_style: u32,
    class: PCWSTR,
    label: &str,
    style: u32,
    id: usize,
) -> io::Result<HWND> {
    unsafe {
        let window = checked(CreateWindowExW(
            ex_style,
            class,
            wide(label).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            1,
            1,
            parent,
            id as HMENU,
            GetModuleHandleW(null()),
            null(),
        ))?;
        SendMessageW(
            window,
            WM_SETFONT,
            GetStockObject(DEFAULT_GUI_FONT) as usize,
            1,
        );
        Ok(window)
    }
}

fn checked(window: HWND) -> io::Result<HWND> {
    if window.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(window)
    }
}

fn place(window: HWND, x: i32, y: i32, width: i32, height: i32) {
    unsafe {
        MoveWindow(window, x, y, width, height, 1);
    }
}

fn client_rect(window: HWND) -> RECT {
    let mut rect = RECT::default();
    unsafe {
        GetClientRect(window, &mut rect);
    }
    rect
}

fn caption(window: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(window) }.max(0);
    let mut value = vec![0; len as usize + 1];
    let read = unsafe { GetWindowTextW(window, value.as_mut_ptr(), len + 1) };
    String::from_utf16_lossy(&value[..read.max(0) as usize])
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn pick_file(window: HWND) -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};

    let mut path = vec![0; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: window,
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: path.len() as u32,
        lpstrFilter: w!("HTML 파일\0*.html;*.htm;*.xhtml\0모든 파일\0*.*\0"),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_EXPLORER,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return None;
    }
    let end = path.iter().position(|&unit| unit == 0)?;
    Some(PathBuf::from(OsString::from_wide(&path[..end])))
}
