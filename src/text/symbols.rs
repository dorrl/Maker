use super::{Color, Language, completion::Kind, tokens};
use std::{collections::HashMap, io, ops::Range};

struct Token<'a> {
    text: &'a str,
    range: Range<usize>,
    word: bool,
    scope: usize,
    pair: Option<usize>,
    child: Option<usize>,
    starts_line: bool,
}

#[derive(Default)]
struct Scope {
    parent: Option<usize>,
    class: bool,
    function: bool,
    module: bool,
}

#[derive(Clone, Copy)]
struct Binding {
    declaration: usize,
    scope: usize,
    visible_from: usize,
    members: Option<usize>,
}

pub struct Rename {
    pub name: String,
    ranges: Vec<Range<usize>>,
    selected: usize,
    definition: usize,
}

pub struct Change {
    pub range: Range<usize>,
    pub selected: Range<usize>,
    pub replacement: String,
    pub count: usize,
}

struct Model<'a> {
    tokens: Vec<Token<'a>>,
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
    names: HashMap<(usize, &'a str), Vec<usize>>,
    declarations: Vec<Option<usize>>,
    language: Language,
}

pub(super) struct VisibleSymbol<'a> {
    pub name: &'a str,
    pub kind: Kind,
    pub distance: usize,
}

pub(super) fn visible_symbols<'a>(
    code: &'a str,
    cursor: usize,
    language: Language,
    receiver: Option<usize>,
    syntax: &[super::Token],
) -> Vec<VisibleSymbol<'a>> {
    let model = Model::from_tokens(code, syntax, language);
    let end = model
        .tokens
        .partition_point(|token| token.range.start <= cursor);
    let region = end.checked_sub(1).and_then(|index| model.tokens.get(index));
    let current = region.map_or(0, |token| token.child.unwrap_or(token.scope));
    let mut scope = if let Some(receiver) = receiver {
        let Some(index) = model
            .tokens
            .iter()
            .position(|token| token.range.start == receiver)
        else {
            return Vec::new();
        };
        let word = model.word(index);
        if matches!(word, "this" | "self" | "Self" | "cls") {
            let mut owner = current;
            while !model.scopes[owner].class {
                let Some(parent) = model.scopes[owner].parent else {
                    return Vec::new();
                };
                owner = parent;
            }
            owner
        } else {
            let Some(class) = model
                .resolve(index, None, word)
                .and_then(|id| model.bindings[id].members)
            else {
                return Vec::new();
            };
            class
        }
    } else {
        current
    };
    let mut distance = 0;
    let mut names = std::collections::HashSet::new();
    let mut entries = Vec::new();
    let mut python_function = false;
    loop {
        let frame = &model.scopes[scope];
        if receiver.is_some() || !(language == Language::Python && python_function && frame.class) {
            for (&(owner, name), bindings) in &model.names {
                if scope != owner || names.contains(name) {
                    continue;
                }
                let latest = bindings
                    .iter()
                    .copied()
                    .map(|id| model.bindings[id])
                    .filter(|binding| binding.visible_from <= end)
                    .max_by_key(|binding| (binding.visible_from, binding.declaration));
                let Some(binding) = latest else {
                    continue;
                };
                let index = binding.declaration;
                if model.tokens[index].range.start == cursor {
                    continue;
                }
                let preceding = index.checked_sub(1).map_or("", |i| model.word(i));
                let kind = if matches!(
                    preceding,
                    "class" | "enum" | "interface" | "struct" | "trait" | "type"
                ) {
                    Kind::Type
                } else if matches!(preceding, "fn" | "def" | "function" | "func")
                    || model.word(index + 1) == "("
                {
                    Kind::Function
                } else if receiver.is_some() {
                    Kind::Member
                } else {
                    Kind::Local
                };
                entries.push(VisibleSymbol {
                    name,
                    kind,
                    distance: distance.min(20),
                });
                names.insert(name);
            }
        }
        if receiver.is_some() {
            break;
        }
        let Some(parent) = frame.parent else {
            break;
        };
        python_function |= frame.function;
        scope = parent;
        distance += 1;
    }
    entries
}

impl Language {
    pub fn can_rename(self) -> bool {
        matches!(
            self,
            Self::Rust
                | Self::JavaScript
                | Self::TypeScript
                | Self::Python
                | Self::Go
                | Self::C
                | Self::CSharp
                | Self::Java
        )
    }
}

pub fn prepare(code: &str, cursor: usize, language: Language) -> io::Result<Rename> {
    if !language.can_rename() {
        return Err(error(crate::i18n::text(
            "ui.symbol.renaming.is.unavailable.for.this.language",
        )));
    }
    let model = Model::new(code, language);
    let selected = model
        .tokens
        .iter()
        .position(|token| token.word && token.range.contains(&cursor))
        .or_else(|| {
            model
                .tokens
                .iter()
                .position(|token| token.word && token.range.end == cursor)
        })
        .ok_or_else(|| {
            error(crate::i18n::text(
                "ui.place.the.caret.on.a.variable.function",
            ))
        })?;
    let binding = model
        .resolve(selected, None, model.tokens[selected].text)
        .ok_or_else(|| {
            error(crate::i18n::text(
                "ui.could.not.find.the.symbol.declaration.in",
            ))
        })?;
    let name = model.tokens[selected].text;
    let edits = model.references(binding, name);
    let selected = edits
        .iter()
        .position(|&index| index == selected)
        .expect("selected reference");
    Ok(Rename {
        name: name.to_owned(),
        selected,
        definition: model.tokens[model.bindings[binding].declaration]
            .range
            .start,
        ranges: edits
            .into_iter()
            .map(|index| model.tokens[index].range.clone())
            .collect(),
    })
}

impl Rename {
    pub fn apply(&self, code: &str, new_name: &str, language: Language) -> io::Result<Change> {
        if !is_word(new_name, language) || reserved(language, new_name) {
            return Err(error(crate::i18n::text(
                "ui.enter.an.identifier.supported.by.this.language",
            )));
        }
        let model = Model::new(code, language);
        let declaration = model
            .tokens
            .iter()
            .position(|token| token.range.start == self.definition);
        let Some(binding) = declaration.and_then(|index| model.declarations[index]) else {
            return Err(error(crate::i18n::text(
                "ui.the.file.changed.press.f2.again",
            )));
        };
        let matches = model.references(binding, &self.name);
        if matches.len() != self.ranges.len()
            || matches
                .iter()
                .zip(&self.ranges)
                .any(|(index, range)| model.tokens[*index].range != *range)
        {
            return Err(error(crate::i18n::text(
                "ui.the.file.changed.press.f2.again",
            )));
        }
        let definition_scope = model.bindings[binding].scope;
        let renamed = Some((binding, new_name));
        if model.bindings.iter().enumerate().any(|(number, existing)| {
            number != binding
                && existing.scope == definition_scope
                && model.tokens[existing.declaration].text == new_name
        }) || matches
            .iter()
            .any(|&index| model.resolve(index, renamed, new_name) != Some(binding))
            || model.tokens.iter().enumerate().any(|(index, token)| {
                token.word
                    && token.text == new_name
                    && model.resolve(index, None, new_name)
                        != model.resolve(index, renamed, new_name)
            })
        {
            return Err(error(crate::i18n::text(
                "ui.this.conflicts.with.another.symbol.of.the",
            )));
        }
        let first = self.ranges.first().expect("one edit").start;
        let end = self.ranges.last().expect("one edit").end;
        let mut replacement = String::new();
        let mut last = first;
        let mut selected = 0..0;
        for (index, range) in self.ranges.iter().enumerate() {
            replacement.push_str(&code[last..range.start]);
            if index == self.selected {
                selected = replacement.len()..replacement.len() + new_name.len();
            }
            replacement.push_str(new_name);
            last = range.end;
        }
        Ok(Change {
            range: first..end,
            selected,
            replacement,
            count: self.ranges.len(),
        })
    }
}

impl<'a> Model<'a> {
    fn new(code: &'a str, language: Language) -> Self {
        Self::from_tokens(code, &tokens(code, language), language)
    }

    fn from_tokens(code: &'a str, syntax: &[super::Token], language: Language) -> Self {
        let tokens = lex(code, syntax, language);
        let size = tokens.len();
        let mut model = Self {
            tokens,
            scopes: vec![Scope::default()],
            bindings: Vec::new(),
            names: HashMap::new(),
            declarations: vec![None; size],
            language,
        };
        model.groups();
        if language == Language::Python {
            model.python_scopes(code);
        } else {
            model.brace_scopes();
        }
        model.declare();
        model.arrow_parameters();
        model.infer_members();
        model
    }

    fn scope(&mut self, parent: usize) -> usize {
        self.scopes.push(Scope {
            parent: Some(parent),
            ..Scope::default()
        });
        self.scopes.len() - 1
    }

    fn groups(&mut self) {
        let mut stack = Vec::new();
        for i in 0..self.tokens.len() {
            let token = self.tokens[i].text;
            match token {
                "{" | "(" | "[" => stack.push(i),
                "}" | ")" | "]" => {
                    if let Some(&from) = stack.last() {
                        let open = match token {
                            "}" => "{",
                            ")" => "(",
                            _ => "[",
                        };
                        if self.tokens[from].text == open {
                            stack.pop();
                            self.tokens[i].pair = Some(from);
                            self.tokens[from].pair = Some(i);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn brace_scopes(&mut self) {
        let mut current = 0;
        for index in 0..self.tokens.len() {
            let close = self.tokens[index].text == "}" && self.tokens[index].pair.is_some();
            if close {
                current = self.scopes[current].parent.unwrap_or(0);
            }
            self.tokens[index].scope = current;
            if self.tokens[index].text == "{" {
                current = self.scope(current);
                self.tokens[index].child = Some(current);
            }
        }
    }

    fn python_scopes(&mut self, code: &str) {
        let mut stack = vec![(0usize, 0usize)];
        let mut first = 0;
        let mut pending: Option<(usize, usize)> = None;
        while first < self.tokens.len() {
            let start = self.tokens[first].range.start;
            let head = code[..start].rfind('\n').map_or(0, |index| index + 1);
            let indent = code[head..start]
                .chars()
                .map(|c| if c == '\t' { 8 } else { 1 })
                .sum::<usize>();
            let mut end = first;
            loop {
                if let Some(pair) = self.tokens[end].pair.filter(|&pair| pair > end) {
                    end = pair;
                }
                end += 1;
                if end == self.tokens.len()
                    || code[self.tokens[end - 1].range.end..self.tokens[end].range.start]
                        .contains('\n')
                {
                    break;
                }
            }
            while stack.last().is_some_and(|&(level, _)| level > indent) {
                stack.pop();
            }
            let mut current = stack.last().map_or(0, |&(_, scope)| scope);
            if let Some((header, previous_indent)) = pending.take()
                && indent > previous_indent
            {
                current = self.scope(current);
                self.tokens[header].child = Some(current);
                stack.push((indent, current));
            }
            let keyword = if self.word(first) == "async" {
                first + 1
            } else {
                first
            };
            if matches!(self.word(keyword), "def" | "class") {
                pending = Some((keyword, indent));
                if let Some(colon) = self
                    .python_header_end(keyword, end)
                    .filter(|&index| index + 1 < end)
                {
                    current = self.scope(current);
                    self.tokens[keyword].child = Some(current);
                    for token in &mut self.tokens[colon + 1..end] {
                        token.scope = current;
                    }
                    let outer = self.scopes[current].parent.unwrap_or(0);
                    for token in &mut self.tokens[first..=colon] {
                        token.scope = outer;
                    }
                    pending = None;
                    first = end;
                    continue;
                }
            }
            for token in &mut self.tokens[first..end] {
                token.scope = current;
            }
            first = end;
        }
    }

    fn python_header_end(&self, start: usize, end: usize) -> Option<usize> {
        let mut i = start + 1;
        while i < end {
            if self.word(i) == ":" {
                return Some(i);
            }
            if let Some(pair) = self.tokens[i].pair.filter(|&pair| pair > i) {
                i = pair;
            }
            i += 1;
        }
        None
    }

    fn word(&self, index: usize) -> &'a str {
        self.tokens.get(index).map_or("", |token| token.text)
    }

    fn name(&self, index: usize) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| token.word && !reserved(self.language, token.text))
    }

    fn add(&mut self, index: usize, scope: usize, visible_from: usize, merge: bool) {
        if self.declarations[index].is_some() {
            return;
        }
        let name = self.word(index);
        if merge && let Some(&id) = self.names.get(&(scope, name)).and_then(|ids| ids.first()) {
            self.declarations[index] = Some(id);
            return;
        }
        let id = self.bindings.len();
        self.bindings.push(Binding {
            declaration: index,
            scope,
            visible_from,
            members: None,
        });
        self.names.entry((scope, name)).or_default().push(id);
        self.declarations[index] = Some(id);
    }

    fn arrow_parameters(&mut self) {
        if !matches!(self.language, Language::JavaScript | Language::TypeScript) {
            return;
        }
        for i in 1..self.tokens.len() {
            if self.word(i) != "=>" || i + 1 >= self.tokens.len() {
                continue;
            }
            let param = i - 1;
            let head = if self.word(param) == ")" {
                self.tokens[param].pair
            } else if self.name(param) {
                Some(param)
            } else {
                None
            };
            let Some(head) = head else {
                continue;
            };
            let parent = self.tokens[i].scope;
            let child = if let Some(scope) = self.tokens[i + 1].child {
                scope
            } else {
                let scope = self.scope(parent);
                let mut end = i + 1;
                while end < self.tokens.len() && self.tokens[end].scope == parent {
                    if matches!(self.word(end), "," | ";" | ")" | "]" | "}") {
                        break;
                    }
                    if end > i + 1
                        && self.tokens[end].starts_line
                        && matches!(
                            self.word(end),
                            "const" | "let" | "var" | "return" | "function" | "class"
                        )
                    {
                        break;
                    }
                    if let Some(pair) = self.tokens[end].pair.filter(|&pair| pair > end) {
                        end = pair;
                    }
                    end += 1;
                }
                for token in &mut self.tokens[i + 1..end] {
                    if token.scope != parent {
                        continue;
                    }
                    if let Some(inner) = token.child {
                        self.scopes[inner].parent = Some(scope);
                    }
                    token.scope = scope;
                }
                scope
            };
            self.scopes[child].function = true;
            if head == param {
                self.add(head, child, 0, false);
            } else {
                self.parameters(head, child);
            }
        }
    }

    fn declare(&mut self) {
        for i in 0..self.tokens.len() {
            if self.tokens[i].word && !self.member(i) {
                let current = self.tokens[i].scope;
                let next = i + 1;
                match self.word(i) {
                    "fn" | "function" | "func" | "def" | "class" | "struct" | "interface"
                    | "trait" | "enum" | "type"
                        if self.name(next) =>
                    {
                        self.add(next, current, 0, false);
                        self.declare_callable(i, next);
                    }
                    "impl" if self.language == Language::Rust => {
                        if let Some(body) = self.signature(i + 1, Some("{"))
                            && let Some(scope) = self.tokens[body].child
                        {
                            self.scopes[scope].class = true;
                        }
                    }
                    "mod" if self.language == Language::Rust => {
                        if let Some(scope) = self
                            .signature(i + 1, Some("{"))
                            .and_then(|body| self.tokens[body].child)
                        {
                            self.scopes[scope].module = true;
                        }
                    }
                    "let" | "const" | "var" | "static" if self.language != Language::Python => {
                        let name = if self.word(next) == "mut" {
                            next + 1
                        } else {
                            next
                        };
                        if self.name(name) {
                            let start = if self.language == Language::Rust && self.word(i) == "let"
                            {
                                self.statement_end(name) + 1
                            } else {
                                0
                            };
                            self.add(name, current, start, false);
                        }
                    }
                    _ if self.language == Language::Python
                        && self.name(i)
                        && self.word(next) == "="
                        && self.statement_start(i) =>
                    {
                        self.add(i, current, 0, true);
                    }
                    _ if self.name(i)
                        && self.word(next) == "("
                        && self.scopes[current].class
                        && self.statement_start(i) =>
                    {
                        if let Some(body) = self.signature(next, Some("{"))
                            && let Some(child) = self.tokens[body].child
                        {
                            self.add(i, current, 0, false);
                            self.parameters(next, child);
                        }
                    }
                    _ if matches!(
                        self.language,
                        Language::Java | Language::C | Language::CSharp
                    ) && self.name(i)
                        && self.typed_declaration(i) =>
                    {
                        self.add(i, current, 0, false);
                        if self.word(next) == "(" {
                            self.declare_callable(i.saturating_sub(1), i);
                        }
                    }
                    _ if self.language == Language::Go
                        && self.name(i)
                        && self.word(next) == ":="
                        && self.statement_start(i) =>
                    {
                        self.add(i, current, next + 1, false);
                    }
                    _ => {}
                }
            }
        }
    }

    fn declare_callable(&mut self, keyword: usize, name: usize) {
        let is_class = matches!(
            self.word(keyword),
            "class" | "struct" | "interface" | "trait" | "enum"
        );
        let end = if self.language == Language::Python {
            self.tokens[keyword].child
        } else {
            self.signature(name + 1, Some("{"))
                .and_then(|body| self.tokens[body].child)
        };
        let Some(scope) = end else {
            return;
        };
        if is_class {
            self.scopes[scope].class = true;
            if let Some(binding) = self.declarations[name] {
                self.bindings[binding].members = Some(scope);
            }
            return;
        }
        if matches!(self.word(keyword), "type") {
            return;
        }
        self.scopes[scope].function = true;
        if let Some(params) = self.signature(name + 1, Some("(")) {
            self.parameters(params, scope);
        }
    }

    fn signature(&self, index: usize, find: Option<&str>) -> Option<usize> {
        let mut index = index;
        let limit = (index + 128).min(self.tokens.len());
        while index < limit {
            let text = self.word(index);
            if Some(text) == find {
                return Some(index);
            }
            if matches!(text, "{" | "}" | ";" | "=" | "=>" | ":=") {
                return None;
            }
            if self.language == Language::Python && text == ":" {
                return None;
            }
            if let Some(pair) = self.tokens[index].pair.filter(|&pair| pair > index) {
                index = pair;
            }
            index += 1;
        }
        None
    }

    fn parameters(&mut self, open: usize, scope: usize) {
        let Some(close) = self.tokens[open].pair.filter(|&end| end > open) else {
            return;
        };
        let typed = matches!(
            self.language,
            Language::Java | Language::C | Language::CSharp
        );
        let mut from = open + 1;
        while from < close {
            let mut end = from;
            let mut equals = None;
            while end < close && self.word(end) != "," {
                if self.word(end) == "=" {
                    equals.get_or_insert(end);
                }
                if let Some(pair) = self.tokens[end].pair.filter(|&pair| pair > end) {
                    end = pair;
                }
                end += 1;
            }
            let mut candidates = from..equals.unwrap_or(end);
            let name = if typed {
                candidates.rev().find(|&i| self.name(i))
            } else {
                candidates.find(|&i| self.name(i))
            };
            if let Some(name) = name
                && !matches!(self.word(from), "{" | "[" | "(" | "/")
            {
                self.add(name, scope, 0, false);
            }
            from = end + 1;
        }
    }

    fn statement_start(&self, index: usize) -> bool {
        index == 0
            || matches!(self.word(index - 1), "{" | "}" | ";" | ":")
            || self.tokens[index].starts_line
    }

    fn statement_end(&self, index: usize) -> usize {
        let mut i = index;
        while i < self.tokens.len() {
            if matches!(self.word(i), ";" | "}") {
                return i;
            }
            if let Some(pair) = self.tokens[i].pair.filter(|&pair| pair > i) {
                i = pair;
            }
            i += 1;
        }
        i
    }

    fn typed_declaration(&self, index: usize) -> bool {
        let Some(typ) = index.checked_sub(1) else {
            return false;
        };
        let before = typ.checked_sub(1).map_or("", |i| self.word(i));
        let after = self.word(index + 1);
        let prev = self.word(typ);
        (self.name(typ)
            || matches!(
                prev,
                "int"
                    | "long"
                    | "short"
                    | "byte"
                    | "bool"
                    | "boolean"
                    | "char"
                    | "float"
                    | "double"
                    | "string"
                    | "void"
                    | "auto"
                    | "var"
                    | "decimal"
                    | "uint"
                    | "usize"
            ))
            && (typ == 0
                || matches!(
                    before,
                    "{" | "}"
                        | ";"
                        | "public"
                        | "private"
                        | "protected"
                        | "internal"
                        | "static"
                        | "virtual"
                        | "override"
                        | "final"
                        | "const"
                        | "readonly"
                ))
            && (matches!(after, "=" | ";" | ",")
                || after == "(" && self.signature(index + 1, Some("{")).is_some())
    }

    fn member(&self, index: usize) -> bool {
        index > 0 && matches!(self.word(index - 1), "." | "?." | "::" | "->")
    }

    fn infer_members(&mut self) {
        for number in 0..self.bindings.len() {
            if self.bindings[number].members.is_some() {
                continue;
            }
            let index = self.bindings[number].declaration;
            let typed = if self.word(index + 1) == ":" && self.name(index + 2) {
                Some(index + 2)
            } else if self.word(index + 1) == "="
                && self.word(index + 2) == "new"
                && self.name(index + 3)
            {
                Some(index + 3)
            } else if self.language == Language::Python
                && self.word(index + 1) == "="
                && self.word(index + 3) == "("
                && self.name(index + 2)
            {
                Some(index + 2)
            } else if matches!(
                self.language,
                Language::Java | Language::C | Language::CSharp
            ) && index > 0
                && self.typed_declaration(index)
            {
                Some(index - 1)
            } else {
                None
            };
            if let Some(typ) = typed
                && let Some(binding) = self.resolve(typ, None, self.word(typ))
                && self.bindings[binding].members.is_some()
                && self.scopes[self.bindings[binding].members.expect("member scope")].class
            {
                self.bindings[number].members = self.bindings[binding].members;
            }
        }
    }

    fn resolve(&self, index: usize, rename: Option<(usize, &str)>, name: &str) -> Option<usize> {
        if let Some(binding) = self.declarations[index] {
            return Some(binding);
        }
        if !self.name(index) {
            return None;
        }
        let mut scope = self.tokens[index].scope;
        if self.member(index) {
            let owner = index.checked_sub(2).map_or("", |i| self.word(i));
            if self.language == Language::Rust
                && self.word(index - 1) == "::"
                && matches!(owner, "crate" | "self" | "super")
            {
                if owner == "crate" {
                    scope = 0;
                } else {
                    while scope != 0 && !self.scopes[scope].module {
                        scope = self.scopes[scope].parent?;
                    }
                    if owner == "super" {
                        scope = self.scopes[scope].parent?;
                        while scope != 0 && !self.scopes[scope].module {
                            scope = self.scopes[scope].parent?;
                        }
                    }
                }
            } else if matches!(owner, "self" | "this" | "Self" | "cls") {
                while !self.scopes[scope].class {
                    scope = self.scopes[scope].parent?;
                }
            } else {
                let receiver = index.checked_sub(2)?;
                if self.member(receiver) || !self.name(receiver) {
                    return None;
                }
                let receiver = self.resolve(receiver, None, owner)?;
                scope = self.bindings[receiver].members?;
            }
        } else if self.word(index + 1) == ":"
            && self.language != Language::Python
            && self.word(index + 2) != ":"
        {
            // Object/record keys and labels are distinct from normal variable references.
            return None;
        }
        let local_only = self.member(index);
        let mut in_python_function = false;
        loop {
            let frame = &self.scopes[scope];
            if !(self.language == Language::Python
                && in_python_function
                && frame.class
                && !local_only)
            {
                let existing = self
                    .names
                    .get(&(scope, name))
                    .into_iter()
                    .flatten()
                    .copied();
                let renamed = rename
                    .filter(|&(target, new)| new == name && self.bindings[target].scope == scope)
                    .map(|(target, _)| target);
                let mut candidates = existing
                    .filter(|number| !rename.is_some_and(|(target, _)| *number == target))
                    .chain(renamed)
                    .map(|number| (number, &self.bindings[number]))
                    .filter(|(_, binding)| index >= binding.visible_from);
                if let Some((number, binding)) = candidates.next() {
                    let best = candidates.try_fold((number, binding), |best, candidate| {
                        if best.1.visible_from == candidate.1.visible_from {
                            None
                        } else if best.1.visible_from < candidate.1.visible_from {
                            Some(candidate)
                        } else {
                            Some(best)
                        }
                    });
                    return best.map(|(id, _)| id);
                }
            }
            if local_only {
                return None;
            }
            in_python_function |= frame.function;
            scope = frame.parent?;
        }
    }

    fn references(&self, binding: usize, name: &str) -> Vec<usize> {
        self.tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| {
                (token.word
                    && token.text == name
                    && self.resolve(index, None, name) == Some(binding))
                .then_some(index)
            })
            .collect()
    }
}

fn lex<'a>(code: &'a str, syntax: &[super::Token], language: Language) -> Vec<Token<'a>> {
    let mut excluded = syntax
        .iter()
        .filter(|token| matches!(token.color, Color::String | Color::Comment | Color::Number))
        .peekable();
    let mut result = Vec::new();
    let mut index = 0;
    let mut previous_end = 0;
    while index < code.len() {
        let rest = &code[index..];
        let mut chars = rest.chars();
        let first = chars.next().expect("char boundary");
        if let Some(token) = excluded.peek().filter(|token| token.start == index) {
            if !matches!(token.color, Color::Comment) {
                result.push(Token {
                    text: "",
                    range: token.start..token.end,
                    word: false,
                    scope: 0,
                    pair: None,
                    child: None,
                    starts_line: code[previous_end..token.start].contains('\n'),
                });
                previous_end = token.end;
            }
            index = token.end;
            excluded.next();
            continue;
        }
        if first.is_whitespace() {
            index += first.len_utf8();
            continue;
        }
        let is_word = word_start(first, language);
        let length = if is_word {
            first.len_utf8()
                + chars
                    .take_while(|&c| word_part(c, language))
                    .map(char::len_utf8)
                    .sum::<usize>()
        } else {
            [
                "?.", "->", "::", ":=", "==", "!=", "<=", ">=", "=>", "+=", "-=", "&&", "||", "**",
                "...",
            ]
            .iter()
            .find(|&&operator| rest.starts_with(operator))
            .map_or(first.len_utf8(), |op| op.len())
        };
        let end = index + length;
        while excluded.peek().is_some_and(|token| token.start < end) {
            excluded.next();
        }
        let newline = code[previous_end..index].contains('\n');
        result.push(Token {
            text: &code[index..end],
            range: index..end,
            word: is_word,
            scope: 0,
            pair: None,
            child: None,
            starts_line: newline,
        });
        previous_end = end;
        index = end;
    }
    result
}

fn word_start(ch: char, language: Language) -> bool {
    ch == '_'
        || ch.is_alphabetic()
        || ch == '$'
            && matches!(
                language,
                Language::JavaScript | Language::TypeScript | Language::Java
            )
}

fn word_part(ch: char, language: Language) -> bool {
    word_start(ch, language) || ch.is_alphanumeric() || matches!(ch, '\u{0300}'..='\u{036f}')
}

fn is_word(value: &str, language: Language) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|ch| word_start(ch, language))
        && chars.all(|ch| word_part(ch, language))
}

fn reserved(language: Language, value: &str) -> bool {
    // The highlighter also colors library names in Rust, which are valid identifiers.
    let builtin = language == Language::Rust
        && matches!(value, "Result" | "Option" | "Some" | "None" | "Ok" | "Err");
    !builtin
        && language
            .keywords()
            .split_ascii_whitespace()
            .any(|keyword| value == keyword)
        || matches!(
            (language, value),
            (Language::Rust, "union" | "macro" | "try" | "gen" | "_")
                | (
                    Language::JavaScript | Language::TypeScript,
                    "instanceof" | "eval" | "arguments"
                )
                | (Language::Go, "defer" | "select")
        )
}

fn error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rename(code: &str, needle: &str, name: &str, language: Language) -> io::Result<String> {
        let cursor = code.find(needle).expect("test cursor");
        let edit = prepare(code, cursor, language)?.apply(code, name, language)?;
        let mut result = code.to_owned();
        result.replace_range(edit.range, &edit.replacement);
        Ok(result)
    }

    #[test]
    fn typescript_ignores_strings_properties_and_shadowing() -> io::Result<()> {
        let code = "let count = 1; function f(count: number) { return count; }\n\
            const run = (count: number) => f(count) + count;\n\
            console.log(count, item.count, \"count\"); // count\n";
        let output = rename(code, "count = ", "total", Language::TypeScript)?;
        assert_eq!(
            output,
            code.replace("count = ", "total = ")
                .replace("log(count,", "log(total,")
        );
        let code = "class Counter { next() { return 1; } call() { return this.next(); } }\nconst value = new Counter();\n";
        assert_eq!(
            rename(code, "Counter {", "Meter", Language::TypeScript)?,
            code.replace("Counter", "Meter")
        );
        assert_eq!(
            rename(code, "next()", "step", Language::TypeScript)?,
            code.replace("next()", "step()")
        );
        Ok(())
    }

    #[test]
    fn rust_resolves_shadowed_binding_and_rejects_capture() -> io::Result<()> {
        let code = "fn main() { let value = 1; let value = value + 1; consume(value); }";
        let selected = prepare(code, code.find("value = 1").unwrap(), Language::Rust)?;
        let change = selected.apply(code, "first", Language::Rust)?;
        assert_eq!(change.count, 2);
        let code = "fn add(value: i32) { let total = 5; consume(value + total); }";
        assert!(rename(code, "value:", "total", Language::Rust).is_err());
        assert_eq!(
            rename(code, "value:", "amount", Language::Rust)?,
            code.replace("value", "amount")
        );
        Ok(())
    }

    #[test]
    fn python_indentation_resolves_variables_functions_and_classes() -> io::Result<()> {
        let code = "x = 1\ndef f(x):\n    return x\nclass Item:\n    def value(self):\n        return f(x)\nprint(Item(), f(x))\n";
        let output = rename(code, "x = 1", "y", Language::Python)?;
        assert_eq!(
            output,
            code.replace("x = 1", "y = 1")
                .replace("f(x)", "f(y)")
                .replace("def f(y)", "def f(x)")
        );
        assert_eq!(
            rename(code, "Item:", "Count", Language::Python)?,
            code.replace("Item", "Count")
        );
        assert_eq!(
            rename(code, "f(x)", "run", Language::Python)?,
            code.replace("f(x)", "run(x)")
        );
        Ok(())
    }
}
