use std::{
    ffi::{OsStr, OsString, c_void},
    io,
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
    ptr::{null, null_mut},
};

pub type Handle = isize;
pub type Proc = extern "system" fn(Handle, u32, usize, isize) -> isize;

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_MOVE: u32 = 0x0003;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_DRAWITEM: u32 = 0x002b;
pub const WM_NOTIFY: u32 = 0x004e;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_CTLCOLOREDIT: u32 = 0x0133;
pub const WM_CTLCOLORLISTBOX: u32 = 0x0134;
pub const WM_CTLCOLORSTATIC: u32 = 0x0138;
pub const WM_DROPFILES: u32 = 0x0233;
pub const WM_SETTINGCHANGE: u32 = 0x001a;
pub const WM_DPICHANGED: u32 = 0x02e0;
pub const WM_PAINT: u32 = 0x000f;
pub const WM_PRINTCLIENT: u32 = 0x0318;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_MBUTTONDOWN: u32 = 0x0207;
pub const WM_MOUSEWHEEL: u32 = 0x020a;
pub const WM_CAPTURECHANGED: u32 = 0x0215;
pub const WM_MOUSELEAVE: u32 = 0x02a3;
pub const WM_VSCROLL: u32 = 0x0115;
pub const WM_HSCROLL: u32 = 0x0114;

pub const WS_CHILD: u32 = 0x40000000;
pub const WS_TABSTOP: u32 = 0x00010000;
pub const WS_VSCROLL: u32 = 0x00200000;
pub const WS_HSCROLL: u32 = 0x00100000;
pub const WS_BORDER: u32 = 0x00800000;
pub const ES_MULTILINE: u32 = 0x0004;
pub const ES_AUTOHSCROLL: u32 = 0x0080;
pub const ES_AUTOVSCROLL: u32 = 0x0040;
pub const ES_NOHIDESEL: u32 = 0x0100;
pub const ES_WANTRETURN: u32 = 0x1000;
pub const ES_DISABLENOSCROLL: u32 = 0x2000;
pub const BS_OWNERDRAW: u32 = 0x000b;
pub const LBS_NOTIFY: u32 = 0x0001;
pub const LBS_OWNERDRAWFIXED: u32 = 0x0010;
pub const LBS_HASSTRINGS: u32 = 0x0040;
pub const LBS_EXTENDEDSEL: u32 = 0x0800;
pub const LBS_NOINTEGRALHEIGHT: u32 = 0x0100;
pub const LBS_DISABLENOSCROLL: u32 = 0x1000;
pub const SS_CENTERIMAGE: u32 = 0x0200;
pub const SS_NOPREFIX: u32 = 0x0080;

pub const EM_SETMODIFY: u32 = 0x00b9;
pub const EM_GETMODIFY: u32 = 0x00b8;
pub const EM_SCROLLCARET: u32 = 0x00b7;
pub const EM_EMPTYUNDOBUFFER: u32 = 0x00cd;
pub const EM_REDO: u32 = 0x0400 + 84;
pub const EM_UNDO: u32 = 0x00c7;
pub const EM_EXLIMITTEXT: u32 = 0x0400 + 53;
pub const EM_SETEVENTMASK: u32 = 0x0400 + 69;
pub const EM_SETBKGNDCOLOR: u32 = 0x0400 + 67;
pub const EM_SETTARGETDEVICE: u32 = 0x0400 + 72;
pub const EM_SETUNDOLIMIT: u32 = 0x0400 + 82;
pub const EM_STOPGROUPTYPING: u32 = 0x0400 + 88;
pub const EM_GETSCROLLPOS: u32 = 0x0400 + 221;
pub const EM_SETSCROLLPOS: u32 = 0x0400 + 222;
pub const EN_CHANGE: u16 = 0x0300;
pub const EN_SETFOCUS: u16 = 0x0100;
pub const EN_VSCROLL: u16 = 0x0602;
pub const EN_SELCHANGE: u32 = 0x0702;
pub const ENM_CHANGE: isize = 1;
pub const ENM_SCROLL: isize = 4;
pub const ENM_SELCHANGE: isize = 0x00080000;
pub const LB_RESETCONTENT: u32 = 0x0184;
pub const LB_ADDSTRING: u32 = 0x0180;
pub const LB_GETTOPINDEX: u32 = 0x018e;
pub const LB_SETTOPINDEX: u32 = 0x0197;
pub const LB_SETITEMHEIGHT: u32 = 0x01a0;
pub const LB_SETITEMDATA: u32 = 0x019a;
const LB_SETSEL: u32 = 0x0185;
const LB_GETSELCOUNT: u32 = 0x0190;
const LB_GETSELITEMS: u32 = 0x0191;
const LB_SETCARETINDEX: u32 = 0x019e;
const LB_GETCARETINDEX: u32 = 0x019f;
pub const MB_ICONERROR: u32 = 0x10;
pub const MB_YESNOCANCEL: u32 = 3;
pub const MB_ICONWARNING: u32 = 0x30;
pub const MB_OKCANCEL: u32 = 1;
pub const MB_DEFBUTTON2: u32 = 0x100;
pub const EM_SETREADONLY: u32 = 0x00cf;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Copy)]
pub struct SavedWindow {
    pub normal: Rect,
    pub maximized: bool,
}

#[repr(C)]
struct WindowPlacement {
    length: u32,
    flags: u32,
    show: u32,
    min: Point,
    max: Point,
    normal: Rect,
}

#[repr(C)]
pub struct Msg {
    pub window: Handle,
    pub message: u32,
    pub wparam: usize,
    pub lparam: isize,
    time: u32,
    point: Point,
    private: u32,
}

#[repr(C)]
struct WindowClass {
    size: u32,
    style: u32,
    proc: Option<Proc>,
    class_extra: i32,
    window_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu: *const u16,
    name: *const u16,
    icon_small: Handle,
}

#[repr(C)]
pub struct Notify {
    pub window: Handle,
    pub id: usize,
    pub code: u32,
}

#[repr(C)]
pub struct DrawItem {
    pub kind: u32,
    pub id: u32,
    pub item: u32,
    pub action: u32,
    pub state: u32,
    pub window: Handle,
    pub dc: Handle,
    pub rect: Rect,
    pub data: usize,
}

#[repr(C)]
struct OpenFile {
    size: u32,
    owner: Handle,
    instance: Handle,
    filter: *const u16,
    custom_filter: *mut u16,
    max_custom: u32,
    filter_index: u32,
    file: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_title: u32,
    initial_dir: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    extension: u16,
    default_extension: *const u16,
    param: isize,
    hook: usize,
    template: *const u16,
    reserved: *mut c_void,
    reserved_size: u32,
    flags_ex: u32,
}

#[repr(C)]
struct InitControls {
    size: u32,
    classes: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct Selection {
    pub start: i32,
    pub end: i32,
}

#[repr(C)]
struct TextLength {
    flags: u32,
    codepage: u32,
}

#[repr(C)]
struct GetText {
    bytes: u32,
    flags: u32,
    codepage: u32,
    default: *const u8,
    used: *mut i32,
}

#[repr(C)]
struct CharFormat {
    size: u32,
    mask: u32,
    effects: u32,
    height: i32,
    offset: i32,
    color: u32,
    charset: u8,
    family: u8,
    face: [u16; 32],
}

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);

#[repr(C)]
struct TextDocumentVtable {
    query: unsafe extern "system" fn(*mut TextDocument, *const Guid, *mut *mut TextDocument) -> i32,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut TextDocument) -> u32,
    before_freeze: [usize; 15],
    freeze: unsafe extern "system" fn(*mut TextDocument, *mut i32) -> i32,
    unfreeze: unsafe extern "system" fn(*mut TextDocument, *mut i32) -> i32,
    before_undo: [usize; 2],
    undo: unsafe extern "system" fn(*mut TextDocument, i32, *mut i32) -> i32,
    redo: usize,
    range: unsafe extern "system" fn(*mut TextDocument, i32, i32, *mut *mut TomRange) -> i32,
}

#[repr(C)]
struct TomRange {
    vtable: *const TomRangeVtable,
}

#[repr(C)]
struct TomRangeVtable {
    query: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut TomRange) -> u32,
    before_start: [usize; 11],
    get_start: unsafe extern "system" fn(*mut TomRange, *mut i32) -> i32,
    before_font: [usize; 3],
    get_font: unsafe extern "system" fn(*mut TomRange, *mut *mut TomFont) -> i32,
    before_index: [usize; 7],
    get_index: unsafe extern "system" fn(*mut TomRange, i32, *mut i32) -> i32,
    set_index: unsafe extern "system" fn(*mut TomRange, i32, i32, i32) -> i32,
    set_range: unsafe extern "system" fn(*mut TomRange, i32, i32) -> i32,
}

#[repr(C)]
struct TomFont {
    vtable: *const TomFontVtable,
}

#[repr(C)]
struct TomFontVtable {
    query: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut TomFont) -> u32,
    before_foreground: [usize; 22],
    set_foreground: unsafe extern "system" fn(*mut TomFont, i32) -> i32,
}

pub struct RangeColor {
    range: *mut TomRange,
    font: *mut TomFont,
}

impl RangeColor {
    pub fn set(&self, start: i32, end: i32, color: u32) -> bool {
        unsafe {
            ((*(*self.range).vtable).set_range)(self.range, start, end) >= 0
                && ((*(*self.font).vtable).set_foreground)(self.font, color as i32) >= 0
        }
    }
}

impl Drop for RangeColor {
    fn drop(&mut self) {
        unsafe {
            ((*(*self.font).vtable).release)(self.font);
            ((*(*self.range).vtable).release)(self.range);
        }
    }
}

#[repr(C)]
struct UnknownVtable {
    query: unsafe extern "system" fn(*mut Unknown, *const Guid, *mut *mut TextDocument) -> i32,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut Unknown) -> u32,
}

#[repr(C)]
struct Unknown {
    vtable: *const UnknownVtable,
}

#[repr(C)]
struct FileDialogVtable {
    query: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut FileDialog) -> u32,
    show: unsafe extern "system" fn(*mut FileDialog, Handle) -> i32,
    before_options: [usize; 5],
    set_options: unsafe extern "system" fn(*mut FileDialog, u32) -> i32,
    get_options: unsafe extern "system" fn(*mut FileDialog, *mut u32) -> i32,
    set_default_folder: usize,
    set_folder: unsafe extern "system" fn(*mut FileDialog, *mut ShellItem) -> i32,
    before_title: [usize; 4],
    set_title: unsafe extern "system" fn(*mut FileDialog, *const u16) -> i32,
    before_result: [usize; 2],
    get_result: unsafe extern "system" fn(*mut FileDialog, *mut *mut ShellItem) -> i32,
}

#[repr(C)]
struct FileDialog {
    vtable: *const FileDialogVtable,
}

#[repr(C)]
struct ShellItemVtable {
    query: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut ShellItem) -> u32,
    bind: usize,
    parent: usize,
    display_name: unsafe extern "system" fn(*mut ShellItem, u32, *mut *mut u16) -> i32,
}

#[repr(C)]
struct ShellItem {
    vtable: *const ShellItemVtable,
}

#[repr(C)]
struct FileOperation {
    vtable: *const FileOperationVtable,
}

#[repr(C)]
struct FileOperationVtable {
    query: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut FileOperation) -> u32,
    advise: usize,
    unadvise: usize,
    flags: unsafe extern "system" fn(*mut FileOperation, u32) -> i32,
    progress_message: usize,
    progress_dialog: usize,
    properties: usize,
    owner: unsafe extern "system" fn(*mut FileOperation, Handle) -> i32,
    before_copy: [usize; 6],
    copy: unsafe extern "system" fn(
        *mut FileOperation,
        *mut ShellItem,
        *mut ShellItem,
        *const u16,
        *mut c_void,
    ) -> i32,
    copy_items: usize,
    delete: unsafe extern "system" fn(*mut FileOperation, *mut ShellItem, *mut c_void) -> i32,
    delete_items: usize,
    new_item: usize,
    perform: unsafe extern "system" fn(*mut FileOperation) -> i32,
    aborted: unsafe extern "system" fn(*mut FileOperation, *mut i32) -> i32,
}

const SHELL_ITEM_IID: Guid = Guid(
    0x43826d1e,
    0xe718,
    0x42ee,
    [0xbc, 0x55, 0xa1, 0xe2, 0x61, 0xc3, 0x7b, 0xfe],
);

#[repr(C)]
struct TextDocument {
    vtable: *const TextDocumentVtable,
}

pub struct FormattingPause {
    document: *mut TextDocument,
    frozen: bool,
}

impl FormattingPause {
    pub fn range_color(&self) -> Option<RangeColor> {
        unsafe {
            let range = tom_range(self.document, 0)?;
            let mut font = null_mut();
            if ((*(*range).vtable).get_font)(range, &mut font) != 0 || font.is_null() {
                ((*(*range).vtable).release)(range);
                return None;
            }
            Some(RangeColor { range, font })
        }
    }
}

impl Drop for FormattingPause {
    fn drop(&mut self) {
        // ITextDocument::Undo(tomResume) resumes normal edit recording.
        unsafe {
            let table = &*(*self.document).vtable;
            (table.undo)(self.document, -9999994, null_mut());
            if self.frozen {
                (table.unfreeze)(self.document, &mut 0);
            }
            (table.release)(self.document);
        }
    }
}

#[repr(C)]
struct PaintStruct {
    dc: Handle,
    erase: i32,
    rect: Rect,
    restore: i32,
    update: i32,
    reserved: [u8; 32],
}

#[repr(C)]
struct TrackMouse {
    size: u32,
    flags: u32,
    window: Handle,
    hover_time: u32,
}

#[repr(C)]
pub struct ScrollInfo {
    size: u32,
    mask: u32,
    pub min: i32,
    pub max: i32,
    pub page: u32,
    pub position: i32,
    track: i32,
}

pub type SubclassProc = extern "system" fn(Handle, u32, usize, isize, usize, usize) -> isize;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemDirectoryW(buffer: *mut u16, length: u32) -> u32;
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn LoadLibraryW(name: *const u16) -> Handle;
    fn LoadLibraryExW(name: *const u16, file: Handle, flags: u32) -> Handle;
    pub fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
    fn MoveFileW(source: *const u16, target: *const u16) -> i32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    fn GlobalLock(handle: Handle) -> *mut c_void;
    fn GlobalUnlock(handle: Handle) -> i32;
    fn GlobalSize(handle: Handle) -> usize;
    fn GlobalFree(handle: Handle) -> Handle;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WindowClass) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        text: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: *const c_void,
    ) -> Handle;
    pub fn DefWindowProcW(window: Handle, message: u32, wp: usize, lp: isize) -> isize;
    pub fn DestroyWindow(window: Handle) -> i32;
    pub fn PostQuitMessage(exit: i32);
    pub fn GetMessageW(message: *mut Msg, window: Handle, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(message: *const Msg) -> i32;
    pub fn DispatchMessageW(message: *const Msg) -> isize;
    fn SendMessageW(window: Handle, message: u32, wp: usize, lp: isize) -> isize;
    fn ShowWindow(window: Handle, show: i32) -> i32;
    fn SystemParametersInfoW(action: u32, parameter: u32, output: *mut c_void, flags: u32) -> i32;
    pub fn IsWindowVisible(window: Handle) -> i32;
    fn GetClientRect(window: Handle, rect: *mut Rect) -> i32;
    fn SetWindowTextW(window: Handle, value: *const u16) -> i32;
    fn GetWindowTextLengthW(window: Handle) -> i32;
    fn GetWindowTextW(window: Handle, text: *mut u16, max: i32) -> i32;
    fn WindowFromPoint(point: Point) -> Handle;
    fn GetAncestor(window: Handle, flags: u32) -> Handle;
    fn GetWindowLongW(window: Handle, index: i32) -> i32;
    fn SetWindowLongW(window: Handle, index: i32, value: i32) -> i32;
    fn MonitorFromWindow(window: Handle, flags: u32) -> Handle;
    fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfo) -> i32;
    pub fn SetForegroundWindow(window: Handle) -> i32;
    pub fn SetFocus(window: Handle) -> Handle;
    pub fn GetFocus() -> Handle;
    fn GetDC(window: Handle) -> Handle;
    fn ReleaseDC(window: Handle, dc: Handle) -> i32;
    fn GetClipboardSequenceNumber() -> u32;
    fn OpenClipboard(window: Handle) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
    fn GetClipboardData(format: u32) -> Handle;
    fn SetClipboardData(format: u32, handle: Handle) -> Handle;
    fn GetNextDlgTabItem(window: Handle, control: Handle, previous: i32) -> Handle;
    pub fn GetDlgItem(window: Handle, id: i32) -> Handle;
    pub fn GetDlgCtrlID(window: Handle) -> i32;
    pub fn GetParent(window: Handle) -> Handle;
    pub fn SetCapture(window: Handle) -> Handle;
    pub fn ReleaseCapture() -> i32;
    pub fn PostMessageW(window: Handle, message: u32, wp: usize, lp: isize) -> i32;
    fn SetCursor(cursor: Handle) -> Handle;
    fn TrackMouseEvent(track: *const TrackMouse) -> i32;
    fn MapWindowPoints(from: Handle, to: Handle, points: *mut Point, count: u32) -> i32;
    fn BeginPaint(window: Handle, paint: *mut PaintStruct) -> Handle;
    fn EndPaint(window: Handle, paint: *const PaintStruct) -> i32;
    fn GetScrollInfo(window: Handle, axis: i32, info: *mut ScrollInfo) -> i32;
    fn SetScrollInfo(window: Handle, axis: i32, info: *const ScrollInfo, redraw: i32) -> i32;
    pub fn GetDpiForWindow(window: Handle) -> u32;
    fn SetProcessDpiAwarenessContext(context: Handle) -> i32;
    fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
    fn GetWindowPlacement(window: Handle, placement: *mut WindowPlacement) -> i32;
    fn SetWindowPlacement(window: Handle, placement: *const WindowPlacement) -> i32;
    fn MessageBoxW(window: Handle, body: *const u16, title: *const u16, style: u32) -> i32;
    pub fn GetKeyState(key: i32) -> i16;
    pub fn SetTimer(window: Handle, id: usize, ms: u32, callback: usize) -> usize;
    pub fn KillTimer(window: Handle, id: usize) -> i32;
    fn InvalidateRect(window: Handle, rect: *const Rect, erase: i32) -> i32;
    pub fn UpdateWindow(window: Handle) -> i32;
    fn FillRect(dc: Handle, rect: *const Rect, brush: Handle) -> i32;
    fn DrawTextW(dc: Handle, text: *const u16, len: i32, rect: *mut Rect, format: u32) -> i32;
    fn GetWindowRect(window: Handle, rect: *mut Rect) -> i32;
    pub fn SetWindowPos(
        window: Handle,
        after: Handle,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    fn SetWindowRgn(window: Handle, region: Handle, redraw: i32) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> Handle;
    fn CreateCompatibleDC(dc: Handle) -> Handle;
    fn CreateCompatibleBitmap(dc: Handle, width: i32, height: i32) -> Handle;
    fn DeleteDC(dc: Handle) -> i32;
    fn BitBlt(
        dst: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        src: Handle,
        sx: i32,
        sy: i32,
        op: u32,
    ) -> i32;
    fn CreateRectRgn(left: i32, top: i32, right: i32, bottom: i32) -> Handle;
    pub fn DeleteObject(object: Handle) -> i32;
    pub fn SetTextColor(dc: Handle, color: u32) -> u32;
    pub fn SetBkColor(dc: Handle, color: u32) -> u32;
    fn SetBkMode(dc: Handle, mode: i32) -> i32;
    fn GetTextExtentPoint32W(dc: Handle, text: *const u16, len: i32, size: *mut Point) -> i32;
    fn SelectObject(dc: Handle, object: Handle) -> Handle;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        output: u32,
        clip: u32,
        quality: u32,
        family: u32,
        face: *const u16,
    ) -> Handle;
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn InitCommonControlsEx(controls: *const InitControls) -> i32;
    fn SetWindowSubclass(window: Handle, callback: SubclassProc, id: usize, data: usize) -> i32;
    pub fn DefSubclassProc(window: Handle, message: u32, wp: usize, lp: isize) -> isize;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(window: Handle, attribute: u32, value: *const u32, bytes: u32) -> i32;
}

#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(info: *mut OpenFile) -> i32;
    fn GetSaveFileNameW(info: *mut OpenFile) -> i32;
    fn CommDlgExtendedError() -> u32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub fn DragAcceptFiles(window: Handle, accept: i32);
    fn ShellExecuteW(
        window: Handle,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> Handle;
    fn DragQueryFileW(drop: Handle, index: u32, path: *mut u16, len: u32) -> u32;
    fn DragFinish(drop: Handle);
    fn SHCreateItemFromParsingName(
        name: *const u16,
        context: *mut c_void,
        interface: *const Guid,
        item: *mut *mut ShellItem,
    ) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn OleInitialize(reserved: *mut c_void) -> i32;
    fn OleUninitialize();
    fn CoTaskMemFree(ptr: *mut c_void);
    fn CoCreateInstance(
        class: *const Guid,
        outer: *mut c_void,
        context: u32,
        interface: *const Guid,
        object: *mut *mut c_void,
    ) -> i32;
}

pub struct Ole;
impl Ole {
    pub fn init() -> io::Result<Self> {
        let code = unsafe { OleInitialize(null_mut()) };
        if code < 0 {
            return Err(io::Error::other(format!("OLE 초기화 실패 ({code:#x})")));
        }
        Ok(Self)
    }
}
impl Drop for Ole {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
    }
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

pub fn system_library(name: &str) -> Handle {
    unsafe { LoadLibraryExW(wide(name).as_ptr(), 0, 0x800) }
}

pub fn setup(class: &str, proc: Proc) -> io::Result<()> {
    let rich = wide("Msftedit.dll");
    // Both handles remain loaded for the lifetime of all native controls.
    unsafe {
        SetProcessDpiAwarenessContext(-4);
        if LoadLibraryW(rich.as_ptr()) == 0 {
            return Err(io::Error::last_os_error());
        }
        let common = InitControls {
            size: size_of::<InitControls>() as u32,
            classes: 8,
        };
        if InitCommonControlsEx(&common) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    register_class(class, proc)
}

pub fn register_class(class: &str, proc: Proc) -> io::Result<()> {
    let class = wide(class);
    unsafe {
        let description = WindowClass {
            size: size_of::<WindowClass>() as u32,
            style: 0x000b,
            proc: Some(proc),
            class_extra: 0,
            window_extra: 0,
            instance: GetModuleHandleW(null()),
            icon: LoadIconW(0, 32512 as *const u16),
            cursor: LoadCursorW(0, 32512 as *const u16),
            background: 0,
            menu: null(),
            name: class.as_ptr(),
            icon_small: 0,
        };
        if RegisterClassExW(&description) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn main_window(class: &str) -> io::Result<Handle> {
    let default = i32::MIN;
    let (class, title) = (wide(class), wide("Maker"));
    let handle = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0x02cf0000,
            default,
            default,
            1140,
            760,
            0,
            0,
            GetModuleHandleW(null()),
            null(),
        )
    };
    if handle == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(handle)
    }
}

pub fn tool_window(class: &str, label: &str, parent: Handle, size: Point) -> io::Result<Handle> {
    let frame = relative_rect(parent, 0);
    let (class, label) = (wide(class), wide(label));
    let handle = unsafe {
        CreateWindowExW(
            0x80,
            class.as_ptr(),
            label.as_ptr(),
            0x02c80000,
            frame.left + ((frame.width() - size.x) / 2).max(0),
            frame.top + ((frame.height() - size.y) / 2).max(0),
            size.x,
            size.y,
            parent,
            0,
            GetModuleHandleW(null()),
            null(),
        )
    };
    if handle == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(handle)
    }
}

pub fn next_dialog_control(window: Handle, previous: bool) {
    unsafe {
        SetFocus(GetNextDlgTabItem(window, GetFocus(), i32::from(previous)));
    }
}

pub fn is_child(parent: Handle, window: Handle) -> bool {
    parent != 0 && (window == parent || unsafe { GetParent(window) } == parent)
}

pub fn child(
    parent: Handle,
    id: usize,
    class: &str,
    title: &str,
    style: u32,
) -> io::Result<Handle> {
    let (class, title) = (wide(class), wide(title));
    let handle = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_CHILD | 0x04000000 | style,
            0,
            0,
            0,
            0,
            parent,
            id as Handle,
            GetModuleHandleW(null()),
            null(),
        )
    };
    if handle == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(handle)
    }
}

pub fn system_program(name: &str) -> io::Result<std::path::PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let size = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if size == 0 || size >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    use std::os::windows::ffi::OsStringExt;
    Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..size])).join(name))
}

pub fn send(window: Handle, message: u32, wp: usize, lp: isize) -> isize {
    unsafe { SendMessageW(window, message, wp, lp) }
}
pub fn show(window: Handle, visible: bool) {
    unsafe {
        let shown = GetWindowLongW(window, -16) as u32 & 0x10000000 != 0;
        if shown != visible {
            ShowWindow(window, if visible { 8 } else { 0 });
        }
    }
}

pub fn window_at(from: Handle, point: Point) -> (Handle, Point) {
    let screen = map_point(from, 0, point);
    let target = unsafe { GetAncestor(WindowFromPoint(screen), 2) };
    (target, map_point(0, target, screen))
}

#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
}

pub fn fullscreen(window: Handle, saved: Option<SavedWindow>) -> bool {
    const GWL_STYLE: i32 = -16;
    const FRAME: i32 = 0x00cf0000;
    unsafe {
        let style = GetWindowLongW(window, GWL_STYLE);
        if let Some(saved) = saved {
            SetWindowLongW(window, GWL_STYLE, style | FRAME);
            ShowWindow(window, 9);
            SetWindowPos(
                window,
                0,
                saved.normal.left,
                saved.normal.top,
                saved.normal.width(),
                saved.normal.height(),
                0x0034,
            );
            if saved.maximized {
                ShowWindow(window, 3);
            }
        } else {
            let mut info = MonitorInfo {
                size: size_of::<MonitorInfo>() as u32,
                monitor: Rect::default(),
                work: Rect::default(),
                flags: 0,
            };
            if GetMonitorInfoW(MonitorFromWindow(window, 2), &mut info) == 0 {
                return false;
            }
            if style & 0x01000000 != 0 {
                ShowWindow(window, 9);
            }
            SetWindowLongW(
                window,
                GWL_STYLE,
                GetWindowLongW(window, GWL_STYLE) & !FRAME,
            );
            let rect = info.monitor;
            SetWindowPos(
                window,
                0,
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                0x0034,
            );
        }
    }
    true
}

pub fn window_placement(window: Handle) -> Option<SavedWindow> {
    let mut placement = WindowPlacement {
        length: std::mem::size_of::<WindowPlacement>() as u32,
        flags: 0,
        show: 0,
        min: Point::default(),
        max: Point::default(),
        normal: Rect::default(),
    };
    (unsafe { GetWindowPlacement(window, &mut placement) } != 0).then_some(SavedWindow {
        normal: placement.normal,
        maximized: placement.show == 3 || placement.flags & 2 != 0,
    })
}

pub fn position_main_window(window: Handle, saved: SavedWindow) {
    if i64::from(saved.normal.right) - i64::from(saved.normal.left) < 320
        || i64::from(saved.normal.bottom) - i64::from(saved.normal.top) < 240
    {
        return;
    }
    let placement = WindowPlacement {
        length: std::mem::size_of::<WindowPlacement>() as u32,
        flags: 0,
        show: 0,
        min: Point { x: -1, y: -1 },
        max: Point { x: -1, y: -1 },
        normal: saved.normal,
    };
    unsafe {
        SetWindowPlacement(window, &placement);
    }
}

pub fn show_main_window(window: Handle, maximized: bool) {
    unsafe {
        ShowWindow(window, if maximized { 3 } else { 5 });
    }
}

pub fn show_overlay(window: Handle, rect: Rect) {
    unsafe {
        let flags = if relative_rect(window, GetParent(window)) == rect {
            0x0053
        } else {
            0x0050
        };
        SetWindowPos(
            window,
            0,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            flags,
        );
    }
}

impl Rect {
    pub fn from_size(left: i32, top: i32, width: i32, height: i32) -> Self {
        Self {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    pub fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }
    pub fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }
    pub fn has(self, point: Point) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
    }
}

impl Point {
    pub fn packed(value: isize) -> Self {
        Self {
            x: value as i16 as i32,
            y: (value >> 16) as i16 as i32,
        }
    }
}

pub fn map_point(from: Handle, to: Handle, mut point: Point) -> Point {
    unsafe {
        MapWindowPoints(from, to, &mut point, 1);
    }
    point
}

pub fn relative_rect(window: Handle, parent: Handle) -> Rect {
    let mut rect = Rect::default();
    unsafe {
        GetWindowRect(window, &mut rect);
        MapWindowPoints(0, parent, (&mut rect as *mut Rect).cast(), 2);
    }
    rect
}

pub fn clip_client(window: Handle, rect: Rect) {
    unsafe {
        let region = CreateRectRgn(rect.left, rect.top, rect.right, rect.bottom);
        if region != 0 && SetWindowRgn(window, region, 1) == 0 {
            DeleteObject(region);
        }
    }
}

pub fn clear_clip(window: Handle) {
    unsafe {
        SetWindowRgn(window, 0, 1);
    }
}

pub fn paint(window: Handle, draw: impl FnOnce(Handle, Rect)) {
    let mut context: PaintStruct = unsafe { std::mem::zeroed() };
    let dc = unsafe { BeginPaint(window, &mut context) };
    if dc != 0 {
        draw(dc, bounds(window));
    }
    unsafe {
        EndPaint(window, &context);
    }
}

pub fn buffered_paint(window: Handle, draw: impl FnOnce(Handle, Rect)) {
    paint(window, |dc, area| {
        if area.width() == 0 || area.height() == 0 {
            return;
        }
        unsafe {
            let buffer = CreateCompatibleDC(dc);
            let bitmap = CreateCompatibleBitmap(dc, area.width(), area.height());
            if buffer != 0 && bitmap != 0 {
                let previous = SelectObject(buffer, bitmap);
                draw(buffer, area);
                BitBlt(
                    dc,
                    0,
                    0,
                    area.width(),
                    area.height(),
                    buffer,
                    0,
                    0,
                    0x00cc0020,
                );
                SelectObject(buffer, previous);
            } else {
                draw(dc, area);
            }
            if bitmap != 0 {
                DeleteObject(bitmap);
            }
            if buffer != 0 {
                DeleteDC(buffer);
            }
        }
    });
}

pub fn watch_mouse(window: Handle) {
    let track = TrackMouse {
        size: size_of::<TrackMouse>() as u32,
        flags: 2,
        window,
        hover_time: 0,
    };
    unsafe {
        TrackMouseEvent(&track);
    }
}

pub fn cursor(id: usize) {
    unsafe {
        SetCursor(LoadCursorW(0, id as *const u16));
    }
}

pub fn subclass(window: Handle, proc: SubclassProc, data: usize) {
    unsafe {
        SetWindowSubclass(window, proc, 1, data);
    }
}

pub fn scrollbar(window: Handle, horizontal: bool) -> Option<ScrollInfo> {
    let mut info = ScrollInfo {
        size: size_of::<ScrollInfo>() as u32,
        mask: 7,
        min: 0,
        max: 0,
        page: 0,
        position: 0,
        track: 0,
    };
    let result = unsafe { GetScrollInfo(window, i32::from(!horizontal), &mut info) };
    (result != 0).then_some(info)
}

#[cfg(test)]
impl ScrollInfo {
    pub fn sample(min: i32, max: i32, page: u32, position: i32) -> Self {
        Self {
            size: 0,
            mask: 0,
            track: 0,
            min,
            max,
            page,
            position,
        }
    }
}

pub fn set_scroll_thumb(window: Handle, horizontal: bool, position: i32) {
    let info = ScrollInfo {
        size: size_of::<ScrollInfo>() as u32,
        mask: 4,
        min: 0,
        max: 0,
        page: 0,
        position,
        track: 0,
    };
    unsafe {
        SetScrollInfo(window, i32::from(!horizontal), &info, 0);
    }
}

pub fn title_theme(window: Handle, dark: bool, background: u32, foreground: u32) {
    for (attribute, value) in [(20, u32::from(dark)), (35, background), (36, foreground)] {
        unsafe {
            DwmSetWindowAttribute(window, attribute, &value, 4);
        }
    }
}
pub fn place(window: Handle, x: i32, y: i32, width: i32, height: i32) {
    let area = Rect::from_size(x, y, width.max(0), height.max(0));
    unsafe {
        if relative_rect(window, GetParent(window)) != area {
            SetWindowPos(window, 0, x, y, area.width(), area.height(), 0x14);
        }
    }
}
pub fn bounds(window: Handle) -> Rect {
    let mut rect = Rect::default();
    unsafe { GetClientRect(window, &mut rect) };
    rect
}
pub fn redraw(window: Handle) {
    unsafe {
        InvalidateRect(window, null(), 1);
    }
}

pub fn repaint(window: Handle) {
    unsafe {
        InvalidateRect(window, null(), 0);
    }
}

pub fn repaint_rect(window: Handle, rect: Rect) {
    unsafe {
        InvalidateRect(window, &rect, 0);
    }
}

pub fn client_animations() -> bool {
    let mut enabled = 1;
    unsafe {
        SystemParametersInfoW(0x1042, 0, (&mut enabled as *mut i32).cast(), 0);
    }
    enabled != 0
}

pub fn editor_character_at(window: Handle, point: Point) -> i32 {
    send(window, 0x00d7, 0, &point as *const _ as isize).max(0) as i32
}

pub fn editor_range(window: Handle, start: i32, end: i32) -> String {
    #[repr(C)]
    struct TextRange {
        range: Selection,
        output: *mut u16,
    }
    let mut buffer = vec![0u16; (end - start).max(0) as usize + 1];
    let range = TextRange {
        range: Selection { start, end },
        output: buffer.as_mut_ptr(),
    };
    let read = send(window, 0x0400 + 75, 0, &range as *const _ as isize).max(0) as usize;
    String::from_utf16_lossy(&buffer[..read.min(buffer.len() - 1)]).replace('\r', "\n")
}

pub fn move_path(source: &std::path::Path, target: &std::path::Path) -> io::Result<()> {
    let from = wide(source);
    let to = wide(target);
    if unsafe { MoveFileW(from.as_ptr(), to.as_ptr()) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
pub fn painting(window: Handle, enabled: bool) {
    send(window, 0x000b, usize::from(enabled), 0);
}

pub fn title(window: Handle, value: &str) {
    unsafe { SetWindowTextW(window, wide(value).as_ptr()) };
}
pub fn edit_hint(window: Handle, value: &str) {
    let hint = wide(value);
    send(window, 0x1501, 0, hint.as_ptr() as isize);
}
pub fn caption(window: Handle) -> String {
    let count = unsafe { GetWindowTextLengthW(window) }.max(0) + 1;
    let mut bytes = vec![0u16; count as usize];
    let copied = unsafe { GetWindowTextW(window, bytes.as_mut_ptr(), count) }.max(0);
    String::from_utf16_lossy(&bytes[..copied as usize])
}

pub fn message(window: Handle, body: &str, title: &str, style: u32) -> i32 {
    let body = wide(body);
    let title = wide(title);
    unsafe { MessageBoxW(window, body.as_ptr(), title.as_ptr(), style) }
}

pub fn run_application(window: Handle, path: &std::path::Path) -> io::Result<()> {
    let file = wide(path);
    let directory = path.parent().map(wide);
    let result = unsafe {
        ShellExecuteW(
            window,
            wide("open").as_ptr(),
            file.as_ptr(),
            null(),
            directory
                .as_ref()
                .map_or(null(), |directory| directory.as_ptr()),
            1,
        )
    };
    if result <= 32 {
        Err(io::Error::other(format!(
            "실행 파일을 열 수 없습니다. ({result})"
        )))
    } else {
        Ok(())
    }
}

pub fn recycle_paths(window: Handle, paths: &[PathBuf]) -> io::Result<bool> {
    shell_file_operation(window, paths, None)
}

pub fn copy_paths(window: Handle, paths: &[PathBuf], folder: &std::path::Path) -> io::Result<bool> {
    shell_file_operation(window, paths, Some(folder))
}

fn shell_file_operation(
    window: Handle,
    paths: &[PathBuf],
    folder: Option<&std::path::Path>,
) -> io::Result<bool> {
    const CLASS: Guid = Guid(
        0x3ad05575,
        0x8857,
        0x4850,
        [0x92, 0x77, 0x11, 0xb8, 0x5b, 0xdb, 0x8e, 0x09],
    );
    const IID: Guid = Guid(
        0x947aab5f,
        0x0a5c,
        0x4c13,
        [0xb4, 0xd6, 0x4b, 0xf7, 0x83, 0x6f, 0xc9, 0xf8],
    );
    let check = |result: i32| {
        if result < 0 {
            let message = if folder.is_some() {
                "파일을 복사할 수 없습니다."
            } else {
                "휴지통으로 이동할 수 없습니다."
            };
            Err(io::Error::other(format!("{message} ({result:#x})")))
        } else {
            Ok(())
        }
    };
    unsafe {
        let mut operation: *mut FileOperation = null_mut();
        check(CoCreateInstance(
            &CLASS,
            null_mut(),
            1,
            &IID,
            (&mut operation as *mut *mut FileOperation).cast(),
        ))?;
        let api = &*(*operation).vtable;
        let item = |path: &std::path::Path| {
            let mut item = null_mut();
            check(SHCreateItemFromParsingName(
                wide(path).as_ptr(),
                null_mut(),
                &SHELL_ITEM_IID,
                &mut item,
            ))?;
            Ok::<_, io::Error>(item)
        };
        let result = (|| {
            (api.owner)(operation, window);
            let flags = if folder.is_some() {
                0x0008
            } else {
                0x0008_0000 | 0x0040
            };
            check((api.flags)(operation, flags))?;
            let target = folder.map(item).transpose()?;
            let queued = (|| {
                for path in paths {
                    let source = item(path)?;
                    let result = if let Some(target) = target {
                        (api.copy)(operation, source, target, null(), null_mut())
                    } else {
                        (api.delete)(operation, source, null_mut())
                    };
                    ((*(*source).vtable).release)(source);
                    check(result)?;
                }
                Ok::<(), io::Error>(())
            })();
            if let Some(target) = target {
                ((*(*target).vtable).release)(target);
            }
            queued?;
            check((api.perform)(operation))?;
            let mut aborted = 0;
            check((api.aborted)(operation, &mut aborted))?;
            Ok(aborted == 0)
        })();
        (api.release)(operation);
        result
    }
}

pub fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
pub fn brush(color: u32) -> Handle {
    unsafe { CreateSolidBrush(color) }
}
pub fn fill(dc: Handle, rect: Rect, brush: Handle) {
    unsafe {
        FillRect(dc, &rect, brush);
    }
}
pub fn font(height: i32) -> Handle {
    named_font("Segoe UI", height)
}

pub fn named_font(family: &str, height: i32) -> Handle {
    let face = wide(family);
    unsafe { CreateFontW(-height, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr()) }
}

pub fn cell_size(window: Handle, font: Handle) -> Point {
    let mut measure = Point { x: 8, y: 17 };
    unsafe {
        let dc = GetDC(window);
        if dc != 0 {
            let old = SelectObject(dc, font);
            GetTextExtentPoint32W(dc, [b'M' as u16].as_ptr(), 1, &mut measure);
            if old != 0 {
                SelectObject(dc, old);
            }
            ReleaseDC(window, dc);
        }
    }
    measure
}

pub fn fill_color(dc: Handle, rect: Rect, color: u32) {
    const DC_BRUSH: Handle = 18;
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetStockObject(id: Handle) -> Handle;
        fn SetDCBrushColor(dc: Handle, color: u32) -> u32;
    }
    unsafe {
        SetDCBrushColor(dc, color);
    }
    fill(dc, rect, unsafe { GetStockObject(DC_BRUSH) });
}

pub fn fill_rounded(dc: Handle, rect: Rect, radius: i32, color: u32) {
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetStockObject(id: Handle) -> Handle;
        fn SetDCBrushColor(dc: Handle, color: u32) -> u32;
        fn RoundRect(
            dc: Handle,
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
            x: i32,
            y: i32,
        ) -> i32;
    }
    if rect.width() <= 0 || rect.height() <= 0 {
        return;
    }
    unsafe {
        let old_pen = SelectObject(dc, GetStockObject(8));
        let old_brush = SelectObject(dc, GetStockObject(18));
        SetDCBrushColor(dc, color);
        RoundRect(
            dc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius * 2,
            radius * 2,
        );
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
    }
}

pub fn stroke_line(dc: Handle, start: Point, end: Point, color: u32) {
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetStockObject(id: Handle) -> Handle;
        fn SetDCPenColor(dc: Handle, color: u32) -> u32;
        fn MoveToEx(dc: Handle, x: i32, y: i32, previous: *mut Point) -> i32;
        fn LineTo(dc: Handle, x: i32, y: i32) -> i32;
    }
    unsafe {
        let previous = SelectObject(dc, GetStockObject(19)); // DC_PEN
        SetDCPenColor(dc, color);
        MoveToEx(dc, start.x, start.y, null_mut());
        LineTo(dc, end.x, end.y);
        SelectObject(dc, previous);
    }
}

pub fn clipboard_text(window: Handle) -> Option<String> {
    unsafe {
        if OpenClipboard(window) == 0 {
            return None;
        }
        let memory = GetClipboardData(13);
        let result = if memory != 0 {
            let data: *const u16 = GlobalLock(memory).cast();
            if data.is_null() {
                None
            } else {
                let count = (GlobalSize(memory) / 2).min(128 * 1024);
                let units = std::slice::from_raw_parts(data, count);
                let end = units.iter().position(|&ch| ch == 0).unwrap_or(count);
                let value = String::from_utf16_lossy(&units[..end]);
                GlobalUnlock(memory);
                Some(value)
            }
        } else {
            None
        };
        CloseClipboard();
        result
    }
}

pub fn copy_text(window: Handle, text: &str) {
    let text = wide(text);
    unsafe {
        if OpenClipboard(window) == 0 {
            return;
        }
        let memory = GlobalAlloc(2, text.len() * 2);
        if memory != 0 {
            let data: *mut u16 = GlobalLock(memory).cast();
            let mut saved = false;
            if !data.is_null() {
                data.copy_from_nonoverlapping(text.as_ptr(), text.len());
                GlobalUnlock(memory);
                saved = EmptyClipboard() != 0 && SetClipboardData(13, memory) != 0;
            }
            if !saved {
                GlobalFree(memory);
            }
        }
        CloseClipboard();
    }
}
pub fn set_font(window: Handle, font: Handle) {
    send(window, 0x0030, font as usize, 1);
}
pub fn draw_label(dc: Handle, mut rect: Rect, text: &str, font: Handle, color: u32, center: bool) {
    let text = wide(text);
    let flags = 0x0004 | 0x0020 | 0x0800 | 0x8000 | if center { 1 } else { 0 };
    unsafe {
        let before = SelectObject(dc, font);
        SetBkMode(dc, 1);
        SetTextColor(dc, color);
        DrawTextW(dc, text.as_ptr(), text.len() as i32 - 1, &mut rect, flags);
        if before != 0 {
            SelectObject(dc, before);
        }
    }
}
pub fn list_entry(list: Handle, label: &str, data: usize) {
    let label = wide(label);
    let index = send(list, LB_ADDSTRING, 0, label.as_ptr() as isize);
    if index >= 0 {
        send(list, LB_SETITEMDATA, index as usize, data as isize);
    }
}

pub fn list_label(list: Handle, index: usize) -> String {
    let len = send(list, 0x018a, index, 0);
    if len < 0 {
        return String::new();
    }
    let mut label = vec![0; len as usize + 1];
    send(list, 0x0189, index, label.as_mut_ptr() as isize);
    String::from_utf16_lossy(&label[..len as usize])
}

pub fn list_at(list: Handle, packed: isize) -> Option<usize> {
    let hit = send(list, 0x01a9, 0, packed) as usize;
    if hit >> 16 != 0 {
        return None;
    }
    let index = hit & 0xffff;
    list_item_rect(list, index)
        .filter(|rect| rect.has(Point::packed(packed)))
        .map(|_| index)
}

pub fn list_item_rect(list: Handle, index: usize) -> Option<Rect> {
    let mut rect = Rect::default();
    let got = send(list, 0x0198, index, &mut rect as *mut _ as isize);
    (got >= 0).then_some(rect)
}

pub fn list_caret(list: Handle) -> Option<usize> {
    usize::try_from(send(list, LB_GETCARETINDEX, 0, 0)).ok()
}

pub fn list_select(list: Handle, row: usize) {
    send(list, LB_SETSEL, 0, -1);
    list_mark(list, row);
    list_set_caret(list, row);
    send(list, 0x019c, row, 0);
}

pub fn list_modified_click(list: Handle, row: usize, shift: bool, ctrl: bool) {
    if shift {
        let anchor = usize::try_from(send(list, 0x019d, 0, 0)).unwrap_or(row);
        if !ctrl {
            send(list, LB_SETSEL, 0, -1);
        }
        send(list, 0x0183, anchor.min(row), anchor.max(row) as isize);
    } else {
        let selected = send(list, 0x0187, row, 0) > 0;
        send(list, LB_SETSEL, usize::from(!selected), row as isize);
        send(list, 0x019c, row, 0);
    }
    list_set_caret(list, row);
}

pub fn list_mark(list: Handle, row: usize) {
    send(list, LB_SETSEL, 1, row as isize);
}

pub fn list_set_caret(list: Handle, row: usize) {
    send(list, LB_SETCARETINDEX, row, 0);
}

pub fn list_selection(list: Handle) -> Vec<usize> {
    let count = send(list, LB_GETSELCOUNT, 0, 0).max(0) as usize;
    if count == 0 {
        return Vec::new();
    }
    let mut rows = vec![0_i32; count];
    let count = send(list, LB_GETSELITEMS, count, rows.as_mut_ptr() as isize).max(0) as usize;
    rows.into_iter()
        .take(count)
        .filter_map(|row| usize::try_from(row).ok())
        .collect()
}

pub fn file_dialog(window: Handle, save: bool, suggestion: &OsStr) -> io::Result<Vec<PathBuf>> {
    let mut buffer = vec![0u16; 32768];
    let suggestion = wide(suggestion);
    let amount = suggestion.len().min(buffer.len() - 1);
    buffer[..amount].copy_from_slice(&suggestion[..amount]);
    let filter = wide("모든 파일\0*.*\0");
    let mut info: OpenFile = unsafe { std::mem::zeroed() };
    info.size = size_of::<OpenFile>() as u32;
    info.owner = window;
    info.filter = filter.as_ptr();
    info.filter_index = 1;
    info.file = buffer.as_mut_ptr();
    info.max_file = buffer.len() as u32;
    info.flags = 0x00080000 | 0x00000008 | 0x00000800 | if save { 2 } else { 0x00001200 };
    let success = unsafe {
        if save {
            GetSaveFileNameW(&mut info)
        } else {
            GetOpenFileNameW(&mut info)
        }
    };
    if success == 0 {
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 {
            Ok(Vec::new())
        } else {
            Err(io::Error::other(format!("파일 선택 실패 ({error:#x})")))
        };
    }
    let mut parts = buffer
        .split(|&ch| ch == 0)
        .take_while(|part| !part.is_empty())
        .map(OsString::from_wide);
    let Some(first) = parts.next() else {
        return Ok(Vec::new());
    };
    let path = PathBuf::from(first);
    let names: Vec<_> = parts.collect();
    Ok(if names.is_empty() {
        vec![path]
    } else {
        names.into_iter().map(|name| path.join(name)).collect()
    })
}

pub fn folder_dialog(
    window: Handle,
    initial: Option<&std::path::Path>,
    prompt: &str,
) -> io::Result<Option<PathBuf>> {
    const CLASS: Guid = Guid(
        0xdc1c5a9c,
        0xe88a,
        0x4dde,
        [0xa5, 0xa1, 0x60, 0xf8, 0x2a, 0x20, 0xae, 0xf7],
    );
    const IID: Guid = Guid(
        0xd57c7288,
        0xd4ad,
        0x4768,
        [0xbe, 2, 0x9d, 0x96, 0x95, 0x32, 0xd9, 0x60],
    );
    fn check(result: i32) -> io::Result<()> {
        if result >= 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "폴더 선택을 열 수 없습니다. ({result:#x})"
            )))
        }
    }
    // The UI thread owns the COM apartment; release each interface and the path on that thread.
    unsafe {
        let mut dialog: *mut FileDialog = null_mut();
        check(CoCreateInstance(
            &CLASS,
            null_mut(),
            1,
            &IID,
            (&mut dialog as *mut *mut FileDialog).cast(),
        ))?;
        let api = &*(*dialog).vtable;
        let result = (|| {
            let mut flags = 0;
            check((api.get_options)(dialog, &mut flags))?;
            check((api.set_options)(dialog, flags | 0x20 | 0x40 | 0x8))?;
            check((api.set_title)(dialog, wide(prompt).as_ptr()))?;
            if let Some(path) = initial {
                let mut folder: *mut ShellItem = null_mut();
                let opened = SHCreateItemFromParsingName(
                    wide(path).as_ptr(),
                    null_mut(),
                    &SHELL_ITEM_IID,
                    &mut folder,
                );
                if opened >= 0 && !folder.is_null() {
                    (api.set_folder)(dialog, folder);
                    ((*(*folder).vtable).release)(folder);
                }
            }
            let shown = (api.show)(dialog, window);
            if shown as u32 == 0x800704c7 {
                return Ok(None);
            }
            check(shown)?;
            let mut item: *mut ShellItem = null_mut();
            check((api.get_result)(dialog, &mut item))?;
            let item_api = &*(*item).vtable;
            let mut path = null_mut();
            let got = (item_api.display_name)(item, 0x80058000, &mut path);
            (item_api.release)(item);
            check(got)?;
            let len = (0..)
                .find(|&i| *path.add(i) == 0)
                .expect("COM path is terminated");
            let name = OsString::from_wide(std::slice::from_raw_parts(path, len));
            CoTaskMemFree(path.cast());
            Ok(Some(PathBuf::from(name)))
        })();
        (api.release)(dialog);
        result
    }
}

pub fn clipboard_sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

pub fn paths_to_clipboard(window: Handle, paths: &[PathBuf]) -> io::Result<()> {
    const CF_HDROP: u32 = 15;
    fn data(format: u32, bytes: &[u8]) -> io::Result<()> {
        unsafe {
            let memory = GlobalAlloc(2, bytes.len());
            if memory != 0 {
                let dest: *mut u8 = GlobalLock(memory).cast();
                if !dest.is_null() {
                    dest.copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
                    GlobalUnlock(memory);
                    if SetClipboardData(format, memory) != 0 {
                        return Ok(());
                    }
                }
                GlobalFree(memory);
            }
            Err(io::Error::last_os_error())
        }
    }
    let mut buffer: Vec<u8> = [20u32, 0, 0, 0, 1]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    for path in paths {
        buffer.extend(wide(path).into_iter().flat_map(u16::to_le_bytes));
    }
    buffer.extend_from_slice(&[0, 0]);
    unsafe {
        if OpenClipboard(window) == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = if EmptyClipboard() != 0 {
            data(CF_HDROP, &buffer)
        } else {
            Err(io::Error::last_os_error())
        };
        if result.is_ok() {
            let copy = RegisterClipboardFormatW(wide("Preferred DropEffect").as_ptr());
            if copy != 0 {
                let _ = data(copy, &1u32.to_le_bytes());
            }
        }
        CloseClipboard();
        result
    }
}

pub fn clipboard_paths(window: Handle) -> io::Result<Vec<PathBuf>> {
    unsafe {
        if OpenClipboard(window) == 0 {
            return Err(io::Error::last_os_error());
        }
        let data = GetClipboardData(15);
        let paths = if data != 0 {
            file_drop_paths(data)
        } else {
            Vec::new()
        };
        CloseClipboard();
        Ok(paths)
    }
}

pub fn dropped_files(handle: Handle) -> Vec<PathBuf> {
    let files = file_drop_paths(handle);
    unsafe {
        DragFinish(handle);
    }
    files
}

fn file_drop_paths(handle: Handle) -> Vec<PathBuf> {
    let count = unsafe { DragQueryFileW(handle, u32::MAX, null_mut(), 0) };
    (0..count)
        .map(|index| {
            let size = unsafe { DragQueryFileW(handle, index, null_mut(), 0) } + 1;
            let mut path = vec![0u16; size as usize];
            let len = unsafe { DragQueryFileW(handle, index, path.as_mut_ptr(), size) };
            PathBuf::from(OsString::from_wide(&path[..len as usize]))
        })
        .collect()
}

pub fn selection(window: Handle) -> Selection {
    let mut range = Selection::default();
    send(window, 0x0400 + 52, 0, &mut range as *mut _ as isize);
    range
}

pub fn select(window: Handle, start: i32, end: i32) {
    let range = Selection { start, end };
    send(window, 0x0400 + 55, 0, &range as *const _ as isize);
}

pub fn set_editor_text(window: Handle, text: &str) {
    title(window, &text.replace('\n', "\r\n"));
}

pub fn editor_size(window: Handle) -> usize {
    let length = TextLength {
        flags: 2 | 8,
        codepage: 1200,
    };
    send(window, 0x0400 + 95, &length as *const _ as usize, 0).max(0) as usize
}

pub fn editor_text(window: Handle) -> String {
    let count = editor_size(window) + 1;
    let mut bytes = vec![0u16; count];
    let info = GetText {
        bytes: (count * 2) as u32,
        flags: 0,
        codepage: 1200,
        default: null(),
        used: null_mut(),
    };
    let written = send(
        window,
        0x0400 + 94,
        &info as *const _ as usize,
        bytes.as_mut_ptr() as isize,
    )
    .max(0) as usize;
    String::from_utf16_lossy(&bytes[..written.min(count)]).replace('\r', "\n")
}

pub fn replace_selection(window: Handle, value: &str) {
    let value = wide(value.replace('\n', "\r\n"));
    send(window, 0x00c2, 1, value.as_ptr() as isize);
}

pub fn char_format(window: Handle, scope: usize, color: u32, font: Option<(&str, i32)>) {
    let mut spec = CharFormat {
        size: size_of::<CharFormat>() as u32,
        mask: 0x40000000,
        effects: 0,
        height: 0,
        offset: 0,
        color,
        charset: 1,
        family: 0,
        face: [0; 32],
    };
    if let Some((name, points)) = font {
        spec.mask |= 0xa0000000;
        spec.height = points * 20;
        for (slot, unit) in spec.face.iter_mut().take(31).zip(name.encode_utf16()) {
            *slot = unit;
        }
    }
    send(
        window,
        0x0400 + 68,
        scope | 0x20,
        &spec as *const _ as isize,
    );
}

#[cfg(test)]
pub fn selection_foreground(window: Handle) -> u32 {
    let mut format: CharFormat = unsafe { std::mem::zeroed() };
    format.size = size_of::<CharFormat>() as u32;
    send(window, 0x0400 + 58, 1, &mut format as *mut _ as isize);
    format.color
}

fn text_document(window: Handle) -> Option<*mut TextDocument> {
    const IID: Guid = Guid(
        0x8cc497c0,
        0xa1df,
        0x11ce,
        [0x80, 0x98, 0, 0xaa, 0, 0x47, 0xbe, 0x5d],
    );
    let mut rich: *mut Unknown = null_mut();
    send(window, 0x0400 + 60, 0, &mut rich as *mut _ as isize);
    if rich.is_null() {
        return None;
    }
    // IRichEditOle and ITextDocument both start with the three IUnknown methods.
    unsafe {
        let mut doc: *mut TextDocument = null_mut();
        let rich_table = &*(*rich).vtable;
        let queried = (rich_table.query)(rich, &IID, &mut doc);
        (rich_table.release)(rich);
        if queried < 0 || doc.is_null() {
            return None;
        }
        Some(doc)
    }
}

unsafe fn tom_range(document: *mut TextDocument, position: i32) -> Option<*mut TomRange> {
    let mut range = null_mut();
    let status = unsafe { ((*(*document).vtable).range)(document, position, position, &mut range) };
    (status == 0 && !range.is_null()).then_some(range)
}

pub fn editor_paragraph(window: Handle, position: i32) -> Option<(usize, i32)> {
    const TOM_PARAGRAPH: i32 = 4;
    const TOM_MOVE: i32 = 0;
    let document = text_document(window)?;
    unsafe {
        let range = tom_range(document, position);
        ((*(*document).vtable).release)(document);
        let range = range?;
        let table = &*(*range).vtable;
        let mut index = 0;
        let mut start = 0;
        let ok = (table.get_index)(range, TOM_PARAGRAPH, &mut index) == 0
            && index > 0
            && (table.set_index)(range, TOM_PARAGRAPH, index, TOM_MOVE) == 0
            && (table.get_start)(range, &mut start) == 0
            && start <= position;
        (table.release)(range);
        ok.then(|| (index as usize - 1, start))
    }
}

pub fn pause_formatting(window: Handle) -> Option<FormattingPause> {
    let doc = text_document(window)?;
    unsafe {
        let table = &*(*doc).vtable;
        if (table.undo)(doc, -9999995, null_mut()) < 0 {
            (table.release)(doc);
            return None;
        }
        let frozen = (table.freeze)(doc, &mut 0) == 0;
        Some(FormattingPause {
            document: doc,
            frozen,
        })
    }
}
