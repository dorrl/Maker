pub(crate) mod completion;
pub(crate) mod symbols;

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Default)]
pub struct Encoding {
    pub bom: bool,
    pub crlf: bool,
}

pub fn read(path: &Path) -> io::Result<(String, Encoding)> {
    decode(&fs::read(path)?)
}

pub fn decode(bytes: &[u8]) -> io::Result<(String, Encoding)> {
    let bom = bytes.starts_with(b"\xef\xbb\xbf");
    let body = if bom { &bytes[3..] } else { bytes };
    let decoded = std::str::from_utf8(body)
        .ok()
        .filter(|text| !text.contains('\0'))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "UTF-8 텍스트 파일만 열 수 있습니다.",
            )
        })?;
    Ok((
        decoded.replace("\r\n", "\n").replace('\r', "\n"),
        Encoding {
            bom,
            crlf: decoded.contains("\r\n"),
        },
    ))
}

pub fn save(path: &Path, text: &str, encoding: Encoding) -> io::Result<()> {
    let output = if encoding.crlf {
        std::borrow::Cow::Owned(text.replace('\n', "\r\n"))
    } else {
        std::borrow::Cow::Borrowed(text)
    };
    write_atomically(path, |file| {
        if encoding.bom {
            file.write_all(b"\xef\xbb\xbf")?;
        }
        file.write_all(output.as_bytes())
    })
}

pub(crate) fn write_atomically(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = if path.exists() {
        path.canonicalize()?
    } else {
        path.to_owned()
    };
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_file_name(format!(".maker-{}-{id}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let written = write(&mut file).and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| fs::rename(&temp, &path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[derive(Clone)]
pub struct Settings {
    pub dark: bool,
    pub font: String,
    pub points: i32,
    pub wrap: bool,
    pub spaces: usize,
    pub sidebar_width: i32,
    pub sidebar_right: bool,
    pub terminal_dock: TerminalDock,
    pub terminal_height: i32,
    pub terminal_width: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TerminalDock {
    Bottom,
    Left,
    Right,
}

impl TerminalDock {
    fn name(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark: true,
            font: "Cascadia Code".into(),
            points: 11,
            wrap: false,
            spaces: 2,
            sidebar_width: 244,
            sidebar_right: false,
            terminal_dock: TerminalDock::Bottom,
            terminal_height: 242,
            terminal_width: 414,
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        Some(
            PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
                .join("Maker")
                .join("settings.txt"),
        )
    }

    pub fn load() -> Self {
        let mut settings = Self::default();
        let Some(contents) = Self::path().and_then(|path| fs::read_to_string(path).ok()) else {
            return settings;
        };
        for (key, value) in contents.lines().filter_map(|line| line.split_once('=')) {
            match key {
                "dark" => settings.dark = value != "false",
                "wrap" => settings.wrap = value == "true",
                "sidebar_right" => settings.sidebar_right = value == "true",
                "terminal_dock" => {
                    settings.terminal_dock = match value {
                        "left" => TerminalDock::Left,
                        "right" => TerminalDock::Right,
                        _ => TerminalDock::Bottom,
                    };
                }
                "terminal_height" => {
                    settings.terminal_height = value
                        .parse::<i32>()
                        .unwrap_or(settings.terminal_height)
                        .clamp(112, 1600);
                }
                "terminal_width" => {
                    settings.terminal_width = value
                        .parse::<i32>()
                        .unwrap_or(settings.terminal_width)
                        .clamp(196, 2400);
                }
                "sidebar_width" => {
                    settings.sidebar_width = value
                        .parse::<i32>()
                        .unwrap_or(settings.sidebar_width)
                        .clamp(144, 1200);
                }
                "font" if !value.is_empty() => settings.font = value.to_owned(),
                "points" => {
                    settings.points = value.parse::<i32>().unwrap_or(settings.points).clamp(8, 36)
                }
                "spaces" => {
                    settings.spaces = value.parse::<usize>().unwrap_or(settings.spaces).min(8)
                }
                _ => {}
            }
        }
        settings
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path().ok_or_else(|| io::Error::other("설정 경로를 찾을 수 없습니다."))?;
        fs::create_dir_all(path.parent().expect("settings has a parent directory"))?;
        write_atomically(&path, |file| {
            write!(
                file,
                "dark={}\nwrap={}\nfont={}\npoints={}\nspaces={}\nsidebar_width={}\nsidebar_right={}\nterminal_dock={}\nterminal_height={}\nterminal_width={}\n",
                self.dark,
                self.wrap,
                self.font,
                self.points,
                self.spaces,
                self.sidebar_width,
                self.sidebar_right,
                self.terminal_dock.name(),
                self.terminal_height,
                self.terminal_width
            )
        })
    }
}

pub fn files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_owned()];
    let mut paths = Vec::new();
    let mut visited = 0;
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if directory == root => return Err(error),
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if paths.len() >= 1500 || visited >= 8000 {
                break;
            }
            visited += 1;
            let name = entry.file_name();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && !skipped_directory(&name.to_string_lossy()) {
                pending.push(entry.path());
            } else if kind.is_file() {
                paths.push(entry.path());
            }
        }
        if paths.len() >= 1500 || visited >= 8000 {
            break;
        }
    }
    paths.sort_by_cached_key(|path| path.to_string_lossy().to_lowercase());
    Ok(paths)
}

fn skipped_directory(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".hg" | "node_modules" | "target" | ".venv" | "__pycache__" | "dist" | "build"
    )
}

pub fn wide_offset(text: &str, byte: usize) -> i32 {
    text[..byte].encode_utf16().count() as i32
}

pub fn byte_offset(text: &str, wide: i32) -> usize {
    let mut length = 0;
    for (byte, ch) in text.char_indices() {
        if length >= wide || length + ch.len_utf16() as i32 > wide {
            return byte;
        }
        length += ch.len_utf16() as i32;
    }
    text.len()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Plain,
    Diff,
    Rust,
    JavaScript,
    TypeScript,
    Json,
    Html,
    Css,
    Python,
    Go,
    Java,
    C,
    CSharp,
    Ruby,
    Php,
    Xml,
    Markdown,
    Yaml,
    Shell,
    Config,
    Sql,
}

impl Language {
    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "rs" => Self::Rust,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "json" | "jsonc" => Self::Json,
            "html" | "htm" => Self::Html,
            "css" | "scss" | "less" => Self::Css,
            "py" | "pyi" | "pyw" => Self::Python,
            "go" => Self::Go,
            "java" => Self::Java,
            "c" | "h" | "cpp" | "hpp" | "cxx" | "cc" | "hh" => Self::C,
            "cs" => Self::CSharp,
            "rb" => Self::Ruby,
            "php" => Self::Php,
            "xml" | "xaml" | "svg" => Self::Xml,
            "md" | "markdown" => Self::Markdown,
            "yaml" | "yml" => Self::Yaml,
            "sh" | "bash" | "zsh" => Self::Shell,
            "ini" | "cfg" | "toml" => Self::Config,
            "sql" => Self::Sql,
            _ => Self::Plain,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Plain => "일반 텍스트",
            Self::Diff => "Git 변경",
            Self::Rust => "Rust",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Json => "JSON",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::Java => "Java",
            Self::C => "C / C++",
            Self::CSharp => "C#",
            Self::Ruby => "Ruby",
            Self::Php => "PHP",
            Self::Xml => "XML",
            Self::Markdown => "Markdown",
            Self::Yaml => "YAML",
            Self::Shell => "Shell",
            Self::Config => "INI / TOML",
            Self::Sql => "SQL",
        }
    }

    pub fn line_comment(self) -> Option<&'static str> {
        match self {
            Self::Python | Self::Ruby | Self::Php | Self::Shell | Self::Yaml | Self::Config => {
                Some("#")
            }
            Self::Sql => Some("--"),
            Self::Rust
            | Self::JavaScript
            | Self::TypeScript
            | Self::Json
            | Self::Go
            | Self::Java
            | Self::C
            | Self::CSharp => Some("//"),
            _ => None,
        }
    }

    fn block_comment(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Html | Self::Xml | Self::Markdown => Some(("<!--", "-->")),
            Self::Rust
            | Self::JavaScript
            | Self::TypeScript
            | Self::Json
            | Self::Css
            | Self::Go
            | Self::Java
            | Self::C
            | Self::CSharp
            | Self::Php
            | Self::Sql => Some(("/*", "*/")),
            _ => None,
        }
    }

    fn keywords(self) -> &'static str {
        match self {
            Self::Rust => {
                "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while Result Option Some None Ok Err"
            }
            Self::JavaScript | Self::TypeScript => {
                "async await break case catch class const continue default delete do else enum export extends false finally for from function if implements import in instanceof interface let new null of private protected public readonly return static super switch this throw true try type typeof undefined var void while yield"
            }
            Self::Python => {
                "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield"
            }
            Self::Go => {
                "break case chan const continue default defer else fallthrough false for func go goto if import interface map nil package range return select struct switch true type var"
            }
            Self::Java | Self::C | Self::CSharp => {
                "abstract auto bool boolean break byte case catch char class const continue default do double else enum extern false final finally float for if import int interface long namespace new null override private protected public return short sizeof static string struct switch this throw true try typedef using var virtual void while"
            }
            Self::Ruby | Self::Php | Self::Shell => {
                "begin break case class def do done elif else elsif end export false fi for foreach function if in module nil null return then true unless until when while yield"
            }
            Self::Sql => {
                "alter and as create delete distinct drop from group having insert into join limit not null on or order select set table union update values where"
            }
            Self::Json | Self::Yaml | Self::Config => "false null true",
            _ => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Keyword,
    Comment,
    String,
    Number,
    Markup,
    Function,
    Type,
    Property,
    Literal,
    Bracket(u8),
    UnmatchedBracket,
    DiffAdded,
    DiffRemoved,
}

pub struct Token {
    pub start: usize,
    pub end: usize,
    pub color: Color,
}

fn identifier(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphanumeric()
}

pub fn tokens(text: &str, lang: Language) -> Vec<Token> {
    if lang == Language::Plain {
        return Vec::new();
    }
    if lang == Language::Diff {
        let mut start = 0;
        return text
            .split_inclusive('\n')
            .filter_map(|line| {
                let at = start;
                start += line.len();
                let color = if line.starts_with("+++ ")
                    || line.starts_with("--- ")
                    || line.starts_with("diff ")
                    || line.starts_with("@@")
                {
                    Color::Markup
                } else if line.starts_with('+') {
                    Color::DiffAdded
                } else if line.starts_with('-') {
                    Color::DiffRemoved
                } else {
                    return None;
                };
                Some(Token {
                    start: at,
                    end: at + line.trim_end_matches('\n').len(),
                    color,
                })
            })
            .collect();
    }
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut previous = "";
    while let Some(ch) = text[start..].chars().next() {
        let tail = &text[start..];
        let word_len = || tail.find(|ch| !identifier(ch)).unwrap_or(tail.len());
        let block = lang
            .block_comment()
            .filter(|(open, _)| tail.starts_with(open));
        let line = lang
            .line_comment()
            .filter(|prefix| tail.starts_with(prefix));
        let token = if let Some((open, close)) = block {
            let len = block_comment_len(tail, open, close, lang);
            Some((len, Color::Comment))
        } else if line.is_some()
            || lang == Language::Php && tail.starts_with("//")
            || matches!(
                lang,
                Language::JavaScript | Language::TypeScript | Language::Go | Language::Rust
            ) && start == 0
                && tail.starts_with("#!")
                && !tail.starts_with("#![")
        {
            Some((tail.find('\n').unwrap_or(tail.len()), Color::Comment))
        } else if let Some(len) = special_string_len(tail, lang) {
            Some((len, Color::String))
        } else if ch == '/'
            && matches!(lang, Language::JavaScript | Language::TypeScript)
            && regex_context(previous)
            && let Some(len) = regex_len(tail)
        {
            Some((len, Color::String))
        } else if quoted(tail, ch, lang) {
            Some((quoted_len(tail, ch, lang), Color::String))
        } else if ch.is_ascii_digit() {
            Some((number_len(tail), Color::Number))
        } else if matches!(lang, Language::Html | Language::Xml) && ch == '<' {
            start += markup_tag(tail, start, &mut tokens);
            continue;
        } else if lang == Language::Markdown
            && ch == '#'
            && (start == 0 || text.as_bytes()[start - 1] == b'\n')
        {
            Some((tail.find('\n').unwrap_or(tail.len()), Color::Markup))
        } else if identifier(ch) {
            let len = word_len();
            let word = &tail[..len];
            if let Some(color) = word_color(word, previous, &tail[len..], lang) {
                Some((len, color))
            } else {
                previous = word;
                start += len;
                continue;
            }
        } else {
            None
        };
        if let Some((length, color)) = token {
            tokens.push(Token {
                start,
                end: start + length,
                color,
            });
            if color != Color::Comment {
                previous = if matches!(color, Color::String | Color::Number) {
                    "value"
                } else {
                    &tail[..length]
                };
            }
            start += length;
        } else {
            let len = if matches!(ch, '+' | '-')
                && tail.starts_with(if ch == '+' { "++" } else { "--" })
            {
                2
            } else {
                ch.len_utf8()
            };
            if !ch.is_whitespace() {
                previous = &tail[..len];
            }
            start += len;
        }
    }
    tokens
}

fn word_color(word: &str, previous: &str, next: &str, lang: Language) -> Option<Color> {
    use Language::*;
    let next = next.trim_start();
    let code_language = matches!(
        lang,
        Rust | JavaScript | TypeScript | Python | Go | Java | C | CSharp | Ruby | Php
    );
    let literal = match lang {
        Python => matches!(word, "True" | "False" | "None"),
        Rust => matches!(word, "true" | "false" | "None" | "Some"),
        Go | Ruby => matches!(word, "true" | "false" | "nil"),
        JavaScript | TypeScript => matches!(
            word,
            "true" | "false" | "null" | "undefined" | "NaN" | "Infinity"
        ),
        Sql => matches!(
            word.to_ascii_lowercase().as_str(),
            "true" | "false" | "null"
        ),
        Json | Java | C | CSharp | Php | Yaml | Config => {
            matches!(word, "true" | "false" | "null" | "nullptr")
        }
        _ => false,
    };
    if literal {
        return Some(Color::Literal);
    }
    let builtin_type = match lang {
        Rust => matches!(
            word,
            "bool"
                | "char"
                | "str"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "String"
                | "Vec"
                | "Result"
                | "Option"
        ),
        JavaScript | TypeScript => matches!(
            word,
            "number"
                | "string"
                | "boolean"
                | "symbol"
                | "unknown"
                | "never"
                | "any"
                | "bigint"
                | "void"
        ),
        Python => matches!(
            word,
            "int"
                | "float"
                | "str"
                | "bool"
                | "bytes"
                | "list"
                | "dict"
                | "set"
                | "tuple"
                | "object"
        ),
        Go => matches!(
            word,
            "int"
                | "int8"
                | "int16"
                | "int32"
                | "int64"
                | "uint"
                | "uint8"
                | "uint16"
                | "uint32"
                | "uint64"
                | "uintptr"
                | "float32"
                | "float64"
                | "string"
                | "bool"
                | "error"
                | "byte"
                | "rune"
                | "any"
        ),
        Java | C | CSharp => matches!(
            word,
            "void"
                | "bool"
                | "boolean"
                | "char"
                | "byte"
                | "short"
                | "int"
                | "long"
                | "float"
                | "double"
                | "string"
                | "decimal"
                | "unsigned"
                | "signed"
                | "uint"
                | "object"
        ),
        _ => false,
    };
    if builtin_type {
        return Some(Color::Type);
    }
    if lang.keywords().split_ascii_whitespace().any(|keyword| {
        if lang == Sql {
            keyword.eq_ignore_ascii_case(word)
        } else {
            keyword == word
        }
    }) {
        return Some(Color::Keyword);
    }
    if code_language
        && (matches!(
            previous,
            "class" | "enum" | "interface" | "struct" | "trait" | "type" | "new"
        ) || word.starts_with(char::is_uppercase)
            && word.chars().skip(1).any(char::is_lowercase))
    {
        return Some(Color::Type);
    }
    if code_language
        && (matches!(previous, "fn" | "def" | "function" | "func")
            || next.starts_with('(')
            || lang == Rust && next.starts_with("!("))
    {
        return Some(Color::Function);
    }
    if matches!(lang, JavaScript | TypeScript)
        && matches!(previous, "{" | ",")
        && next.starts_with(':')
        || matches!(lang, Yaml | Css) && next.starts_with(':')
        || lang == Config && next.starts_with('=')
    {
        return Some(Color::Property);
    }
    None
}

fn number_len(tail: &str) -> usize {
    let mut end = 0;
    let mut previous = '\0';
    for (index, ch) in tail.char_indices() {
        let next = tail[index + ch.len_utf8()..].chars().next();
        if identifier(ch)
            || ch == '.' && next.is_some_and(|ch| ch.is_ascii_digit())
            || matches!(ch, '+' | '-')
                && matches!(previous, 'e' | 'E' | 'p' | 'P')
                && next.is_some_and(|ch| ch.is_ascii_digit())
        {
            end = index + ch.len_utf8();
            previous = ch;
        } else {
            break;
        }
    }
    end
}

fn block_comment_len(tail: &str, open: &str, close: &str, lang: Language) -> usize {
    let body = &tail[open.len()..];
    if lang != Language::Rust {
        return body
            .find(close)
            .map_or(tail.len(), |at| open.len() + at + close.len());
    }
    let mut depth = 1;
    let mut offset = open.len();
    while let Some(at) = tail[offset..].find(['/', '*']) {
        offset += at;
        let rest = &tail[offset..];
        if rest.starts_with(open) {
            depth += 1;
            offset += open.len();
        } else if rest.starts_with(close) {
            depth -= 1;
            offset += close.len();
            if depth == 0 {
                return offset;
            }
        } else {
            offset += 1;
        }
    }
    tail.len()
}

fn regex_context(previous: &str) -> bool {
    matches!(
        previous,
        "" | "="
            | "("
            | "["
            | "{"
            | ","
            | ":"
            | ";"
            | "!"
            | "?"
            | "&"
            | "|"
            | ">"
            | "return"
            | "throw"
            | "yield"
            | "case"
            | "typeof"
            | "void"
            | "delete"
    )
}

fn regex_len(tail: &str) -> Option<usize> {
    let mut class = false;
    let mut escaped = false;
    for (index, ch) in tail.char_indices().skip(1) {
        if matches!(ch, '\n' | '\r') {
            return None;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => class = true,
            ']' => class = false,
            '/' if !class => {
                let flags = tail[index + 1..]
                    .bytes()
                    .take_while(u8::is_ascii_alphabetic)
                    .count();
                return Some(index + 1 + flags);
            }
            _ => {}
        }
    }
    None
}

fn special_string_len(tail: &str, lang: Language) -> Option<usize> {
    match lang {
        Language::Java if tail.starts_with("\"\"\"") => Some(triple_quote_len(tail, '"')),
        Language::Sql if tail.starts_with(['\'', '"']) => {
            Some(1 + doubled_quote_len(&tail[1..], tail.chars().next().expect("starts with quote")))
        }
        Language::Rust => {
            if let Some(body) = tail
                .strip_prefix("br")
                .or_else(|| tail.strip_prefix("cr"))
                .or_else(|| tail.strip_prefix('r'))
            {
                let hashes = body.bytes().take_while(|&b| b == b'#').count();
                if hashes > 255 || body.as_bytes().get(hashes) != Some(&b'"') {
                    return None;
                }
                let prefix = tail.len() - body.len() + hashes + 1;
                let ending = format!("\"{}", "#".repeat(hashes));
                return Some(
                    tail[prefix..]
                        .find(&ending)
                        .map_or(tail.len(), |at| prefix + at + ending.len()),
                );
            }
            let quote = tail
                .strip_prefix('b')
                .or_else(|| tail.strip_prefix('c'))?
                .chars()
                .next()?;
            if quote == '"' || quote == '\'' && tail.starts_with('b') {
                Some(1 + quoted_len(&tail[1..], quote, lang))
            } else {
                None
            }
        }
        Language::C => {
            let prefix = ["u8R\"", "LR\"", "uR\"", "UR\"", "R\""]
                .into_iter()
                .find(|prefix| tail.starts_with(prefix))?;
            let offset = prefix.len();
            let end = tail[offset..].find('(')?;
            let delimiter = &tail[offset..offset + end];
            if end > 16
                || delimiter
                    .chars()
                    .any(|ch| ch.is_whitespace() || matches!(ch, '\\' | ')'))
            {
                return None;
            }
            let closing = format!("){delimiter}\"");
            let start = offset + end + 1;
            Some(
                tail[start..]
                    .find(&closing)
                    .map_or(tail.len(), |at| start + at + closing.len()),
            )
        }
        Language::CSharp => {
            let text = tail.strip_prefix('$').unwrap_or(tail);
            let dollars = tail.len() - text.len();
            if let Some(body) = text
                .strip_prefix("@\"")
                .or_else(|| tail.strip_prefix("@$\""))
            {
                let prefix = tail.len() - body.len();
                return Some(prefix + doubled_quote_len(body, '"'));
            }
            let width = text.bytes().take_while(|&b| b == b'"').count();
            if width < 3 {
                return None;
            }
            let ending = &text[..width];
            Some(
                text[width..]
                    .find(ending)
                    .map_or(tail.len(), |at| dollars + width + at + width),
            )
        }
        Language::Python => {
            let length = tail
                .bytes()
                .take_while(|byte| {
                    matches!(byte.to_ascii_lowercase(), b'r' | b'b' | b'f' | b'u' | b't')
                })
                .count();
            if !(1..=2).contains(&length) {
                return None;
            }
            let quote = tail[length..].chars().next()?;
            matches!(quote, '\'' | '"').then(|| length + quoted_len(&tail[length..], quote, lang))
        }
        _ => None,
    }
}

fn doubled_quote_len(body: &str, quote: char) -> usize {
    let mut start = 0;
    while let Some(at) = body[start..].find(quote) {
        start += at + quote.len_utf8();
        if !body[start..].starts_with(quote) {
            return start;
        }
        start += quote.len_utf8();
    }
    body.len()
}

fn markup_tag(tail: &str, start: usize, tokens: &mut Vec<Token>) -> usize {
    if tail.starts_with("<![CDATA[") {
        let length = tail.find("]]>").map_or(tail.len(), |at| at + 3);
        tokens.push(Token {
            start,
            end: start + length,
            color: Color::String,
        });
        return length;
    }
    let prefix = ["</", "<?", "<!"]
        .into_iter()
        .find(|prefix| tail.starts_with(prefix))
        .map_or(1, str::len);
    let name = |ch: char| ch.is_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.');
    if !tail[prefix..].starts_with(char::is_alphabetic) {
        return 1;
    }
    tokens.push(Token {
        start,
        end: start + prefix,
        color: Color::Bracket(0),
    });
    let mut at = prefix;
    let mut first = true;
    while let Some(ch) = tail[at..].chars().next() {
        let rest = &tail[at..];
        let style = if rest.starts_with("/>") || rest.starts_with("?>") || ch == '>' {
            let length = if ch == '>' { 1 } else { 2 };
            tokens.push(Token {
                start: start + at,
                end: start + at + length,
                color: Color::Bracket(0),
            });
            return at + length;
        } else if matches!(ch, '\'' | '"') {
            let length = rest[1..].find(ch).map_or(rest.len(), |end| end + 2);
            Some((length, Color::String))
        } else if name(ch) {
            let length = rest.find(|ch| !name(ch)).unwrap_or(rest.len());
            let style = if first {
                Color::Markup
            } else {
                Color::Property
            };
            first = false;
            Some((length, style))
        } else {
            None
        };
        if let Some((length, color)) = style {
            tokens.push(Token {
                start: start + at,
                end: start + at + length,
                color,
            });
            at += length;
        } else {
            at += ch.len_utf8();
        }
    }
    at
}

fn quoted(tail: &str, ch: char, lang: Language) -> bool {
    match ch {
        '"' => true,
        '\'' if lang == Language::Rust => {
            let after = &tail[1..];
            let mut chars = after.chars();
            chars
                .next()
                .is_some_and(|first| first == '\\' || chars.next() == Some('\''))
        }
        '\'' => !matches!(lang, Language::Json | Language::Markdown),
        '\x60' => matches!(
            lang,
            Language::JavaScript
                | Language::TypeScript
                | Language::Go
                | Language::Shell
                | Language::Markdown
        ),
        _ => false,
    }
}

fn quoted_len(tail: &str, quote: char, lang: Language) -> usize {
    if lang == Language::Python && tail.chars().take(3).all(|ch| ch == quote) && tail.len() >= 3 {
        return triple_quote_len(tail, quote);
    }
    let multiline = quote == '\x60'
        || lang == Language::Rust && quote == '"'
        || matches!(
            lang,
            Language::Php | Language::Ruby | Language::Shell | Language::Yaml
        );
    let mut escaped = false;
    for (index, ch) in tail.char_indices().skip(1) {
        if !escaped && ch == quote {
            return index + ch.len_utf8();
        }
        if ch == '\n' && !multiline && !escaped {
            return index;
        }
        escaped = ch == '\\' && !escaped;
    }
    tail.len()
}

fn triple_quote_len(tail: &str, quote: char) -> usize {
    let delimiter = quote.to_string().repeat(3);
    let mut escaped = false;
    for (index, ch) in tail[3..].char_indices() {
        if !escaped && ch == quote && tail[index + 3..].starts_with(&delimiter) {
            return index + 6;
        }
        escaped = ch == '\\' && !escaped;
    }
    tail.len()
}

pub fn highlighting(text: &str, lang: Language) -> Vec<Token> {
    let syntax = tokens(text, lang);
    let (brackets, _) = bracket_tokens(text, lang, &syntax);
    let mut syntax = syntax.into_iter().peekable();
    let mut highlights = Vec::new();
    for token in brackets {
        while syntax.peek().is_some_and(|next| next.start < token.start) {
            highlights.push(syntax.next().expect("peeked token"));
        }
        highlights.push(token);
    }
    highlights.extend(syntax);
    highlights
}

pub fn brackets(text: &str, lang: Language) -> Vec<(usize, &'static str)> {
    let (_, mut issues) = bracket_tokens(text, lang, &tokens(text, lang));
    issues.truncate(160);
    issues
}

type BracketIssues = Vec<(usize, &'static str)>;

fn bracket_tokens(text: &str, lang: Language, syntax: &[Token]) -> (Vec<Token>, BracketIssues) {
    if matches!(
        lang,
        Language::Plain | Language::Diff | Language::Markdown | Language::Html | Language::Xml
    ) {
        return (Vec::new(), Vec::new());
    }
    let mut syntax = syntax.iter().peekable();
    let mut opens: Vec<(usize, usize, u8)> = Vec::new();
    let mut open_counts = [0; 3];
    let mut styles: Vec<Token> = Vec::new();
    let mut issues = Vec::new();
    for (index, ch) in text.char_indices() {
        let (kind, opening) = match ch {
            '(' => (0, true),
            ')' => (0, false),
            '[' => (1, true),
            ']' => (1, false),
            '{' => (2, true),
            '}' => (2, false),
            _ => continue,
        };
        while syntax.peek().is_some_and(|token| token.end <= index) {
            syntax.next();
        }
        if syntax.peek().is_some_and(|token| token.start <= index) {
            continue;
        }
        let position = styles.len();
        if opening {
            let depth = (opens.len() % 3) as u8;
            opens.push((position, kind, depth));
            open_counts[kind] += 1;
        } else if open_counts[kind] > 0 {
            let mut other = opens.pop().expect("opening bracket exists");
            while other.1 != kind {
                open_counts[other.1] -= 1;
                issues.push((styles[other.0].start, "닫히지 않은 괄호"));
                other = opens.pop().expect("matching opening bracket exists");
            }
            open_counts[kind] -= 1;
            let color = Color::Bracket(other.2);
            styles[other.0].color = color;
            styles.push(Token {
                start: index,
                end: index + 1,
                color,
            });
            continue;
        } else {
            issues.push((index, "짝이 맞지 않는 닫는 괄호"));
        }
        styles.push(Token {
            start: index,
            end: index + 1,
            color: Color::UnmatchedBracket,
        });
    }
    issues.extend(
        opens
            .into_iter()
            .map(|(position, _, _)| (styles[position].start, "닫히지 않은 괄호")),
    );
    issues.sort_unstable_by_key(|&(index, _)| index);
    (styles, issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_match_rich_edit_utf16_positions() {
        let text = "a😀\n한글";
        assert_eq!(wide_offset(text, 6), 4);
        assert_eq!(byte_offset(text, 4), 6);
        assert_eq!(byte_offset(text, 2), 1);
    }

    #[test]
    fn colors_and_brackets_ignore_comments_and_quotes() {
        let code = "fn main() { let s = \"{\"; // )\n}\n]";
        let tokens = tokens(code, Language::Rust);
        assert!(
            tokens
                .iter()
                .any(|token| matches!(token.color, Color::String))
        );
        assert_eq!(
            brackets(code, Language::Rust),
            vec![(code.len() - 1, "짝이 맞지 않는 닫는 괄호")]
        );
    }

    #[test]
    fn nested_bracket_pairs_keep_their_depth_and_ignore_raw_literals() {
        let code =
            "fn render() { call([other({ 7 })]); let raw = r#\"([\")]}\"#; /* [( /* } */ )] */ }";
        let styled = highlighting(code, Language::Rust);
        assert!(styled.windows(2).all(|pair| pair[0].end <= pair[1].start));
        let pairs: Vec<_> = styled
            .iter()
            .filter_map(|token| match token.color {
                Color::Bracket(depth) => Some((&code[token.start..token.end], depth)),
                Color::UnmatchedBracket => panic!("unmatched bracket in valid code"),
                _ => None,
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("(", 0),
                (")", 0),
                ("{", 0),
                ("(", 1),
                ("[", 2),
                ("(", 0),
                ("{", 1),
                ("}", 1),
                (")", 0),
                ("]", 2),
                (")", 1),
                ("}", 0),
            ]
        );
        assert!(brackets(code, Language::Rust).is_empty());
    }

    #[test]
    fn syntax_distinguishes_markup_attributes_regex_and_broken_pairs() {
        let code = "const pattern = /[()){}]/g;\nfunction find(x: number) { return true; ] }";
        let spans: Vec<_> = highlighting(code, Language::TypeScript)
            .into_iter()
            .map(|token| (&code[token.start..token.end], token.color))
            .collect();
        for expected in [
            ("/[()){}]/g", Color::String),
            ("find", Color::Function),
            ("number", Color::Type),
            ("true", Color::Literal),
            ("]", Color::UnmatchedBracket),
        ] {
            assert!(spans.contains(&expected), "{expected:?}");
        }
        let html = "<item title=\"a > (b)\">";
        let spans: Vec<_> = highlighting(html, Language::Html)
            .into_iter()
            .map(|token| (&html[token.start..token.end], token.color))
            .collect();
        assert!(spans.contains(&("title", Color::Property)));
        assert!(spans.contains(&("\"a > (b)\"", Color::String)));
    }

    #[test]
    fn preserves_bom_and_crlf_when_replacing_file() -> io::Result<()> {
        let path = std::env::temp_dir().join(format!("maker-test-{}.txt", std::process::id()));
        let result = (|| {
            fs::write(&path, b"\xef\xbb\xbfa\r\nb")?;
            let (text, encoding) = read(&path)?;
            assert_eq!(text, "a\nb");
            save(&path, "한글\nsecond", encoding)?;
            assert_eq!(
                fs::read(&path)?,
                b"\xef\xbb\xbf\xed\x95\x9c\xea\xb8\x80\r\nsecond"
            );
            Ok(())
        })();
        let _ = fs::remove_file(&path);
        result
    }
}
