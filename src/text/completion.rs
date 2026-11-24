use super::{Color, Language, identifier, symbols, tokens};
use std::{collections::HashMap, ops::Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Local,
    Function,
    Type,
    Member,
    Builtin,
    Keyword,
    Snippet,
    Word,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "심볼",
            Self::Function => "함수",
            Self::Type => "타입",
            Self::Member => "멤버",
            Self::Builtin => "내장",
            Self::Keyword => "키워드",
            Self::Snippet => "스니펫",
            Self::Word => "문서",
        }
    }
}

pub struct Candidate {
    pub label: String,
    pub kind: Kind,
    pub insert: String,
    pub caret: usize,
}

pub struct List {
    pub replace: Range<usize>,
    pub candidates: Vec<Candidate>,
}

fn matches(query: &str, label: &str, manual: bool) -> Option<u8> {
    if label.starts_with(query) {
        return Some(0);
    }
    let label = label.to_lowercase();
    let query = query.to_lowercase();
    if label.starts_with(&query) {
        return Some(1);
    }
    if !manual || query.chars().count() < 2 {
        return None;
    }
    let mut remaining = query.chars();
    let mut next = remaining.next();
    for ch in label.chars() {
        if next == Some(ch) {
            next = remaining.next();
        }
    }
    next.is_none().then_some(2)
}

fn word_start(code: &str, end: usize) -> usize {
    code[..end]
        .char_indices()
        .rev()
        .find(|&(_, ch)| !identifier(ch))
        .map_or(0, |(index, ch)| index + ch.len_utf8())
}

pub fn suggest(
    code: &str,
    cursor: usize,
    lang: Language,
    manual: bool,
    indent_unit: &str,
) -> Option<List> {
    let start = word_start(code, cursor);
    let end = cursor
        + code[cursor..]
            .find(|ch| !identifier(ch))
            .unwrap_or(code.len() - cursor);
    let prefix = &code[start..cursor];
    let receiver = code[..start].strip_suffix('.').and_then(|before| {
        let start = word_start(before, before.len());
        let name = &before[start..];
        (!name.is_empty()).then_some((start, name))
    });
    if !manual && (lang == Language::Plain || receiver.is_none() && prefix.chars().count() < 2) {
        return None;
    }
    let syntax = tokens(code, lang);
    if syntax.iter().any(|token| {
        matches!(token.color, Color::Comment | Color::String | Color::Number)
            && token.start < cursor
            && (cursor < token.end || cursor == token.end && token.end > start)
    }) {
        return None;
    }
    if receiver.is_none() && prefix.is_empty() && !manual {
        return None;
    }

    let mut choices: HashMap<String, (u8, usize, Candidate)> = HashMap::new();
    let already_call = code[end..].trim_start().starts_with('(');
    let mut add = |label: &str, value: &str, caret: usize, kind: Kind, rank: usize| {
        let Some(matching) = matches(prefix, label, manual) else {
            return;
        };
        if !matches!(kind, Kind::Snippet) && value == &code[start..end] {
            return;
        }
        let key = if kind == Kind::Snippet {
            format!("snippet:{label}")
        } else {
            label.to_owned()
        };
        let replace = choices
            .get(&key)
            .is_none_or(|(other, priority, _)| (matching, rank) < (*other, *priority));
        if replace {
            choices.insert(
                key,
                (
                    matching,
                    rank,
                    Candidate {
                        label: label.to_owned(),
                        kind,
                        insert: value.to_owned(),
                        caret,
                    },
                ),
            );
        }
    };

    if lang.can_rename() {
        for symbol in
            symbols::visible_symbols(code, start, lang, receiver.map(|(at, _)| at), &syntax)
        {
            let value = if symbol.kind == Kind::Function && !already_call {
                format!("{}()", symbol.name)
            } else {
                symbol.name.to_owned()
            };
            let caret = if value.ends_with("()") {
                value.len() - 1
            } else {
                value.len()
            };
            add(symbol.name, &value, caret, symbol.kind, symbol.distance);
        }
    }
    if let Some((_, name)) = receiver {
        for &(label, kind) in members(name, lang) {
            let value = if kind == Kind::Function && !already_call {
                format!("{label}()")
            } else {
                label.to_owned()
            };
            let caret = if value.ends_with("()") {
                value.len() - 1
            } else {
                value.len()
            };
            add(label, &value, caret, kind, 24);
        }
    } else {
        for &(label, kind) in builtins(lang) {
            let value = if kind == Kind::Function && !already_call {
                format!("{label}()")
            } else {
                label.to_owned()
            };
            let caret = if value.ends_with("()") {
                value.len() - 1
            } else {
                value.len()
            };
            add(label, &value, caret, kind, 30);
        }
        for label in lang.keywords().split_ascii_whitespace() {
            add(label, label, label.len(), Kind::Keyword, 40);
        }
        for &(label, template) in snippets(lang) {
            let line = code[..start].rsplit('\n').next().unwrap_or("");
            if line.trim().is_empty() {
                let (value, caret) = snippet(template, line, indent_unit);
                add(label, &value, caret, Kind::Snippet, 45);
            }
        }
        if !lang.can_rename() {
            let mut excluded = syntax
                .iter()
                .filter(|token| matches!(token.color, Color::Comment | Color::String))
                .peekable();
            for (index, ch) in code.char_indices() {
                while excluded.peek().is_some_and(|token| token.end <= index) {
                    excluded.next();
                }
                if !identifier(ch)
                    || ch.is_numeric()
                    || code[..index].chars().next_back().is_some_and(identifier)
                    || excluded.peek().is_some_and(|token| token.start <= index)
                {
                    continue;
                }
                let word = &code[index..];
                let word = &word[..word.find(|ch| !identifier(ch)).unwrap_or(word.len())];
                if word.len() <= 96 {
                    add(word, word, word.len(), Kind::Word, 60);
                }
            }
        }
    }
    let mut options: Vec<_> = choices.into_values().collect();
    options.sort_unstable_by(|a, b| {
        (a.0, a.1, a.2.label.len(), &a.2.label).cmp(&(b.0, b.1, b.2.label.len(), &b.2.label))
    });
    let candidates: Vec<_> = options
        .into_iter()
        .take(14)
        .map(|(_, _, entry)| entry)
        .collect();
    (!candidates.is_empty()).then_some(List {
        replace: start..end,
        candidates,
    })
}

fn snippet(template: &str, indentation: &str, unit: &str) -> (String, usize) {
    let mut output = String::new();
    let mut caret = None;
    for ch in template.chars() {
        match ch {
            '\x01' => caret = Some(output.len()),
            '\n' => {
                output.push('\n');
                output.push_str(indentation);
            }
            '\t' => output.push_str(unit),
            _ => output.push(ch),
        }
    }
    let caret = caret.unwrap_or(output.len());
    (output, caret)
}

fn builtins(lang: Language) -> &'static [(&'static str, Kind)] {
    use Kind::*;
    use Language::*;
    match lang {
        Rust => &[
            ("Vec", Type),
            ("String", Type),
            ("Option", Type),
            ("Result", Type),
            ("Box", Type),
            ("Some", Builtin),
            ("None", Builtin),
            ("Ok", Builtin),
            ("Err", Builtin),
            ("Default", Type),
            ("Iterator", Type),
            ("IntoIterator", Type),
            ("HashMap", Type),
            ("println!", Builtin),
            ("format!", Builtin),
            ("vec!", Builtin),
            ("assert!", Builtin),
            ("assert_eq!", Builtin),
            ("dbg!", Builtin),
            ("todo!", Builtin),
        ],
        JavaScript | TypeScript => &[
            ("Array", Type),
            ("Object", Type),
            ("Promise", Type),
            ("Map", Type),
            ("Set", Type),
            ("Date", Type),
            ("Error", Type),
            ("console", Builtin),
            ("document", Builtin),
            ("JSON", Builtin),
            ("Math", Builtin),
            ("Number", Type),
            ("fetch", Function),
            ("setTimeout", Function),
            ("parseInt", Function),
        ],
        Python => &[
            ("print", Function),
            ("len", Function),
            ("range", Function),
            ("enumerate", Function),
            ("zip", Function),
            ("sorted", Function),
            ("sum", Function),
            ("open", Function),
            ("isinstance", Function),
            ("str", Type),
            ("int", Type),
            ("bool", Type),
            ("float", Type),
            ("dict", Type),
            ("list", Type),
            ("set", Type),
            ("tuple", Type),
        ],
        Go => &[
            ("append", Function),
            ("make", Function),
            ("len", Function),
            ("cap", Function),
            ("copy", Function),
            ("delete", Function),
            ("panic", Function),
            ("recover", Function),
            ("error", Type),
            ("string", Type),
        ],
        Java => &[
            ("String", Type),
            ("System", Builtin),
            ("List", Type),
            ("Map", Type),
            ("ArrayList", Type),
            ("Objects", Builtin),
        ],
        CSharp => &[
            ("Console", Builtin),
            ("String", Type),
            ("Task", Type),
            ("List", Type),
            ("Dictionary", Type),
        ],
        C => &[
            ("printf", Function),
            ("scanf", Function),
            ("strlen", Function),
            ("malloc", Function),
            ("free", Function),
            ("size_t", Type),
            ("nullptr", Builtin),
        ],
        _ => &[],
    }
}

fn members(receiver: &str, lang: Language) -> &'static [(&'static str, Kind)] {
    use Kind::*;
    match (lang, receiver) {
        (Language::JavaScript | Language::TypeScript, "console") => &[
            ("log", Function),
            ("error", Function),
            ("warn", Function),
            ("info", Function),
            ("debug", Function),
            ("table", Function),
        ],
        (Language::JavaScript | Language::TypeScript, "document") => &[
            ("querySelector", Function),
            ("querySelectorAll", Function),
            ("getElementById", Function),
            ("createElement", Function),
            ("addEventListener", Function),
            ("body", Member),
        ],
        (Language::JavaScript | Language::TypeScript, "JSON") => {
            &[("parse", Function), ("stringify", Function)]
        }
        (Language::JavaScript | Language::TypeScript, "Array") => {
            &[("from", Function), ("isArray", Function), ("of", Function)]
        }
        (Language::JavaScript | Language::TypeScript, "Math") => &[
            ("abs", Function),
            ("ceil", Function),
            ("floor", Function),
            ("max", Function),
            ("min", Function),
            ("random", Function),
            ("round", Function),
            ("PI", Member),
        ],
        (Language::CSharp, "Console") => &[
            ("WriteLine", Function),
            ("Write", Function),
            ("ReadLine", Function),
        ],
        _ => &[],
    }
}

fn snippets(lang: Language) -> &'static [(&'static str, &'static str)] {
    use Language::*;
    match lang {
        Rust => &[
            ("fn", "fn \x01() {\n\t\n}"),
            ("if", "if \x01 {\n\t\n}"),
            ("match", "match \x01 {\n\t\n}"),
            ("test", "#[test]\nfn \x01() {\n\t\n}"),
        ],
        JavaScript | TypeScript => &[
            ("function", "function \x01() {\n\t\n}"),
            ("if", "if (\x01) {\n\t\n}"),
            ("try", "try {\n\t\x01\n} catch (error) {\n\t\n}"),
            ("class", "class \x01 {\n\t\n}"),
        ],
        Python => &[
            ("def", "def \x01():\n\tpass"),
            ("class", "class \x01:\n\tpass"),
            ("if", "if \x01:\n\tpass"),
            ("main", "if __name__ == \"__main__\":\n\t\x01"),
        ],
        Go => &[("func", "func \x01() {\n\t\n}"), ("if", "if \x01 {\n\t\n}")],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_members_and_suffix_replacement() {
        let code = "class Meter { measure() {} }\nconst meter = new Meter();\nmeter.measZZ";
        let caret = code.len() - 2;
        let list = suggest(code, caret, Language::TypeScript, false, "  ").unwrap();
        assert_eq!(&code[list.replace], "measZZ");
        assert_eq!(list.candidates[0].insert, "measure()");
        assert_eq!(list.candidates[0].caret, "measure(".len());
        let code = "const outside = 1;\nfunction f() { const outsider = 2; }\nout";
        let list = suggest(code, code.len(), Language::TypeScript, false, "  ").unwrap();
        assert_eq!(list.candidates[0].label, "outside");
        assert!(!list.candidates.iter().any(|c| c.label == "outsider"));
    }

    #[test]
    fn does_not_suggest_in_comments_and_inserts_indented_snippets() {
        let code = "fn f() {\n  // pr";
        assert!(suggest(code, code.len(), Language::Rust, false, "  ").is_none());
        let code = "fn f() {\n  fn";
        let list = suggest(code, code.len(), Language::Rust, false, "  ").unwrap();
        let candidate = list
            .candidates
            .iter()
            .find(|c| c.kind == Kind::Snippet)
            .unwrap();
        assert_eq!(candidate.insert, "fn () {\n    \n  }");
        assert_eq!(candidate.caret, 3);
    }
}
