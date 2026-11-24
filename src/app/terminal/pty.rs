use std::{
    ffi::{OsStr, c_void},
    fs::File,
    io::{self, Read, Write},
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{FromRawHandle, RawHandle},
    },
    path::PathBuf,
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};

type Handle = isize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_prompt_receives_input_in_its_own_working_directory() -> io::Result<()> {
        let dir = std::env::current_dir()?;
        let session = Session::start(Shell::CommandPrompt, dir.clone(), Size::new(94, 22))?;
        assert!(session.send(b"echo FIRST%SystemRoot%SECOND\r".to_vec()));
        assert!(session.send(b"cd\r".to_vec()));
        assert!(session.send(b"exit 7\r".to_vec()));
        let mut output = Vec::new();
        loop {
            match session
                .output
                .recv_timeout(Duration::from_secs(8))
                .expect("shell output")
            {
                Event::Data(bytes) => output.extend(bytes),
                Event::Error(error) => panic!("{error}"),
                Event::Exit(code) => {
                    assert_eq!(code, 7, "{:?}", String::from_utf8_lossy(&output));
                    break;
                }
            }
        }
        let output = String::from_utf8_lossy(&output);
        let system = std::env::var("SystemRoot").expect("Windows has a system root");
        assert!(
            output.contains(&format!("FIRST{system}SECOND")),
            "{output:?}"
        );
        assert!(
            output
                .to_lowercase()
                .contains(&dir.to_string_lossy().to_lowercase()),
            "{output:?}"
        );
        Ok(())
    }
}

const WAIT_TIMEOUT: u32 = 258;
const KILL_ON_JOB_CLOSE: u32 = 0x2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Size {
    pub cols: i16,
    pub rows: i16,
}

impl Size {
    pub fn new(cols: i32, rows: i32) -> Self {
        Self {
            cols: cols.clamp(16, 300) as i16,
            rows: rows.clamp(3, 120) as i16,
        }
    }
    fn packed(self) -> u32 {
        (self.rows as u32) << 16 | self.cols as u32
    }
}

#[derive(Clone, Copy)]
pub enum Shell {
    PowerShell,
    CommandPrompt,
}

impl Shell {
    pub fn label(self) -> &'static str {
        match self {
            Self::PowerShell => "PowerShell",
            Self::CommandPrompt => "CMD",
        }
    }
    fn command(self) -> io::Result<(PathBuf, &'static str)> {
        let system = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("Windows 경로를 찾을 수 없습니다."))?;
        let system = PathBuf::from(system).join("System32");
        Ok(match self {
            Self::PowerShell => (
                system.join(r"WindowsPowerShell\v1.0\powershell.exe"),
                "-NoLogo",
            ),
            Self::CommandPrompt => (system.join("cmd.exe"), "/D /Q"),
        })
    }

    fn environment(self) -> Option<Vec<u16>> {
        if !matches!(self, Self::PowerShell) {
            return None;
        }
        let mut env: Vec<_> = std::env::vars_os().collect();
        let (_, modules) = env
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case("PSModulePath"))?;
        // Windows PowerShell 5.1 must use its own bundled modules when Maker was
        // launched from PowerShell 7, which can put incompatible modules first.
        let retained = std::env::split_paths(modules).filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("Modules"))
                || !path
                    .parent()
                    .is_some_and(|parent| parent.join("pwsh.exe").is_file())
        });
        let filtered = std::env::join_paths(retained).ok()?;
        if filtered == *modules {
            return None;
        }
        *modules = filtered;
        env.sort_by_cached_key(|(key, _)| key.to_string_lossy().to_uppercase());
        let mut block = Vec::new();
        for (key, value) in env {
            block.extend(key.encode_wide());
            block.push(b'=' as u16);
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        Some(block)
    }
}

pub enum Event {
    Data(Vec<u8>),
    Exit(u32),
    Error(String),
}

pub struct Session {
    input: SyncSender<Vec<u8>>,
    pub output: Receiver<Event>,
    stop: Arc<AtomicBool>,
    size: Arc<AtomicU32>,
}

impl Session {
    pub fn start(shell: Shell, directory: PathBuf, size: Size) -> io::Result<Self> {
        let (input, incoming) = mpsc::sync_channel(128);
        let (outgoing, output) = mpsc::sync_channel(48);
        let stop = Arc::new(AtomicBool::new(false));
        let size = Arc::new(AtomicU32::new(size.packed()));
        let flags = (Arc::clone(&stop), Arc::clone(&size));
        thread::Builder::new()
            .name("maker-terminal".into())
            .spawn(move || {
                let result = run(shell, directory, incoming, &outgoing, flags);
                let event = match result {
                    Ok(code) => Event::Exit(code),
                    Err(error) => Event::Error(error.to_string()),
                };
                let _ = outgoing.send(event);
            })?;
        Ok(Self {
            input,
            output,
            stop,
            size,
        })
    }

    pub fn send(&self, bytes: Vec<u8>) -> bool {
        match self.input.try_send(bytes) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        }
    }

    pub fn resize(&self, size: Size) {
        self.size.store(size.packed(), Ordering::Relaxed);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

struct Owned(Handle);

impl Owned {
    fn into_file(self) -> File {
        let handle = self.0;
        std::mem::forget(self);
        // A pipe handle belongs to this thread and is transferred to File exactly once.
        unsafe { File::from_raw_handle(handle as RawHandle) }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct Console(Handle);

impl Drop for Console {
    fn drop(&mut self) {
        unsafe {
            ClosePseudoConsole(self.0);
        }
    }
}

#[repr(C)]
struct Startup {
    size: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    dimensions: [u32; 7],
    flags: u32,
    show_window: u16,
    reserved_bytes: u16,
    reserved_data: *mut u8,
    stdin: Handle,
    stdout: Handle,
    stderr: Handle,
}

#[repr(C)]
struct StartupEx {
    startup: Startup,
    attributes: *mut c_void,
}

#[repr(C)]
struct ProcessInfo {
    process: Handle,
    thread: Handle,
    process_id: u32,
    thread_id: u32,
}

struct Attributes {
    storage: Vec<usize>,
}

impl Attributes {
    fn new(console: &Console) -> io::Result<Self> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        if unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 1, 0, &mut bytes)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = Self { storage };
        if unsafe {
            UpdateProcThreadAttribute(
                attributes.ptr(),
                0,
                0x00020016,
                console.0 as *mut c_void,
                size_of::<Handle>(),
                null_mut(),
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(attributes)
    }

    fn ptr(&mut self) -> *mut c_void {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.ptr());
        }
    }
}

fn pipe() -> io::Result<(Owned, Owned)> {
    let (mut read, mut write) = (0, 0);
    if unsafe { CreatePipe(&mut read, &mut write, null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((Owned(read), Owned(write)))
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain([0]).collect()
}

fn spawn(
    shell: Shell,
    directory: PathBuf,
    console: &Console,
) -> io::Result<(Owned, Option<Owned>)> {
    let (path, args) = shell.command()?;
    let executable = wide(path.as_os_str());
    let mut command: Vec<_> = [b'"' as u16]
        .into_iter()
        .chain(path.as_os_str().encode_wide())
        .chain(format!("\" {args}").encode_utf16())
        .chain([0])
        .collect();
    let directory = wide(directory.as_os_str());
    let mut attributes = Attributes::new(console)?;
    let mut startup: StartupEx = unsafe { std::mem::zeroed() };
    startup.startup.size = size_of::<StartupEx>() as u32;
    // Suppress Windows' implicit duplication of the parent's redirected standard handles.
    // The three zero handles will be initialized by the pseudoconsole.
    startup.startup.flags = 0x100;
    startup.attributes = attributes.ptr();
    let mut result: ProcessInfo = unsafe { std::mem::zeroed() };
    let environment = shell.environment();
    let env_ptr = environment
        .as_ref()
        .map_or(null(), |block| block.as_ptr().cast());
    let spawned = unsafe {
        CreateProcessW(
            executable.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            0x00080404,
            env_ptr,
            directory.as_ptr(),
            &startup.startup,
            &mut result,
        )
    };
    if spawned == 0 {
        return Err(io::Error::last_os_error());
    }
    let process = Owned(result.process);
    let main_thread = Owned(result.thread);
    let job = job_for(&process);
    if unsafe { ResumeThread(main_thread.0) } == u32::MAX {
        let error = io::Error::last_os_error();
        unsafe {
            TerminateProcess(process.0, 1);
        }
        return Err(error);
    }
    Ok((process, job))
}

#[repr(C)]
struct JobLimits {
    user_time: [i64; 2],
    flags: u32,
    working_sets: [usize; 2],
    processes: u32,
    affinity: usize,
    priority: u32,
    schedule: u32,
    io: [u64; 6],
    memory: [usize; 4],
}

fn job_for(process: &Owned) -> Option<Owned> {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job == 0 {
        return None;
    }
    let job = Owned(job);
    let mut limits: JobLimits = unsafe { std::mem::zeroed() };
    limits.flags = KILL_ON_JOB_CLOSE;
    let ok = unsafe {
        SetInformationJobObject(
            job.0,
            9,
            (&limits as *const JobLimits).cast(),
            size_of::<JobLimits>() as u32,
        ) != 0
            && AssignProcessToJobObject(job.0, process.0) != 0
    };
    ok.then_some(job)
}

fn run(
    shell: Shell,
    directory: PathBuf,
    incoming: Receiver<Vec<u8>>,
    outgoing: &SyncSender<Event>,
    (stop, desired): (Arc<AtomicBool>, Arc<AtomicU32>),
) -> io::Result<u32> {
    let (console_read, pipe_write) = pipe()?;
    let (pipe_read, console_write) = pipe()?;
    let mut current = desired.load(Ordering::Relaxed);
    let unpack = |value: u32| Size {
        cols: value as i16,
        rows: (value >> 16) as i16,
    };
    let mut handle = 0;
    let result = unsafe {
        CreatePseudoConsole(
            unpack(current),
            console_read.0,
            console_write.0,
            0,
            &mut handle,
        )
    };
    if result < 0 {
        return Err(io::Error::other(format!(
            "Windows 터미널을 시작할 수 없습니다. ({result:#x})"
        )));
    }
    let console = Console(handle);
    drop(console_read);
    drop(console_write);

    // These synchronous pipes must be serviced independently, including during console shutdown.
    let output = outgoing.clone();
    let mut reader = pipe_read.into_file();
    let reading = thread::Builder::new()
        .name("maker-terminal-output".into())
        .spawn(move || {
            let mut buffer = [0u8; 8192];
            let mut deliver = true;
            while let Ok(size) = reader.read(&mut buffer) {
                if size == 0 {
                    break;
                }
                if deliver {
                    deliver = output.send(Event::Data(buffer[..size].to_vec())).is_ok();
                }
            }
        })?;

    let closing = Arc::clone(&stop);
    let mut writer = pipe_write.into_file();
    let writing = thread::Builder::new()
        .name("maker-terminal-input".into())
        .spawn(move || {
            while !closing.load(Ordering::Acquire) {
                match incoming.recv_timeout(Duration::from_millis(50)) {
                    Ok(bytes) if writer.write_all(&bytes).is_err() => break,
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });

    let result = (|| {
        let writing = writing?;
        let (process, job) = spawn(shell, directory, &console)?;
        loop {
            if stop.load(Ordering::Acquire) {
                if job.is_some() {
                    drop(job);
                } else {
                    unsafe {
                        TerminateProcess(process.0, 1);
                    }
                }
                break;
            }
            let state = unsafe { WaitForSingleObject(process.0, 35) };
            if state == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            if state != WAIT_TIMEOUT {
                break;
            }
            let packed = desired.load(Ordering::Relaxed);
            if current != packed {
                unsafe {
                    ResizePseudoConsole(console.0, unpack(packed));
                }
                current = packed;
            }
        }
        stop.store(true, Ordering::Release);
        let mut code = 0;
        if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // The output thread continues draining until the last ConPTY handle closes.
        drop(writing);
        Ok(code)
    })();
    stop.store(true, Ordering::Release);
    drop(console);
    let _ = reading.join();
    result
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreatePipe(
        read: *mut Handle,
        write: *mut Handle,
        attributes: *const c_void,
        size: u32,
    ) -> i32;
    fn CreatePseudoConsole(
        size: Size,
        input: Handle,
        output: Handle,
        flags: u32,
        console: *mut Handle,
    ) -> i32;
    fn ResizePseudoConsole(console: Handle, size: Size) -> i32;
    fn ClosePseudoConsole(console: Handle);
    fn CloseHandle(handle: Handle) -> i32;
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        bytes: *mut usize,
    ) -> i32;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        data: *mut c_void,
        bytes: usize,
        previous: *mut c_void,
        returned: *mut usize,
    ) -> i32;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn CreateProcessW(
        application: *const u16,
        command: *mut u16,
        process_attributes: *const c_void,
        thread_attributes: *const c_void,
        inherit: i32,
        flags: u32,
        env: *const c_void,
        directory: *const u16,
        startup: *const Startup,
        result: *mut ProcessInfo,
    ) -> i32;
    fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
    fn GetExitCodeProcess(handle: Handle, code: *mut u32) -> i32;
    fn ResumeThread(thread: Handle) -> u32;
    fn TerminateProcess(process: Handle, exit: u32) -> i32;
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: u32, info: *const c_void, size: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
}
