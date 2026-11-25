use std::{
    fmt::{self, Write},
    io,
};

use serde_json::value::RawValue;
use yaml_rust2::{
    parser::{Event, Parser, Tag},
    scanner::TScalarStyle,
};

use crate::text::{Color, Language};

pub(super) const MAX_INPUT: usize = 8 * 1024 * 1024;
const MAX_OUTPUT: usize = 24 * 1024 * 1024;
const MAX_DEPTH: usize = 128;
const MAX_NODES: usize = 200_000;

pub(super) struct Model {
    pub pretty: Option<String>,
    pub entries: Vec<Entry>,
    pub roots: Vec<usize>,
}

pub(super) struct Entry {
    pub key: String,
    pub value: String,
    pub color: Color,
    pub depth: usize,
    pub parent: Option<usize>,
    pub end: usize,
}

impl Entry {
    pub fn expandable(&self, index: usize) -> bool {
        self.end > index + 1
    }
}

struct ParsedDocument<'a> {
    root: Node,
    version: Option<&'a str>,
}

struct Node {
    value: Value,
    decoration: String,
}

enum Value {
    Scalar(String, Color),
    Seq(Vec<Node>),
    Map(Vec<(Node, Node)>),
}

impl Node {
    fn plain(value: &str, color: Color) -> Self {
        Self {
            value: Value::Scalar(value.to_owned(), color),
            decoration: String::new(),
        }
    }

    fn container(value: Value) -> Self {
        Self {
            value,
            decoration: String::new(),
        }
    }

    fn compound(&self) -> bool {
        match &self.value {
            Value::Seq(children) => !children.is_empty(),
            Value::Map(children) => !children.is_empty(),
            Value::Scalar(..) => false,
        }
    }
}

pub(super) fn parse(code: &str, language: Language) -> io::Result<Model> {
    if code.len() > MAX_INPUT {
        return Err(io::Error::other("JSON/YAML preview is limited to 8 MiB."));
    }
    let docs = if language == Language::Json {
        serde_json::from_str::<&RawValue>(code).map_err(io::Error::other)?;
        let mut reader = Json {
            rest: code,
            count: 0,
        };
        vec![ParsedDocument {
            root: reader.node(0)?,
            version: None,
        }]
    } else {
        read_yaml(code)?
    };
    let mut output = Output::default();
    let pretty = print_documents(&mut output, &docs, language == Language::Json)
        .is_ok()
        .then_some(output.0);
    let mut entries = Vec::new();
    let mut roots = Vec::new();
    for (index, doc) in docs.iter().enumerate() {
        roots.push(entries.len());
        let key = if docs.len() > 1 {
            format!("문서 {}", index + 1)
        } else {
            String::new()
        };
        flatten(&mut entries, &doc.root, key, 0, None);
    }
    Ok(Model {
        pretty,
        entries,
        roots,
    })
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn check_budget(count: &mut usize, depth: usize) -> io::Result<()> {
    *count += 1;
    if *count > MAX_NODES || depth >= MAX_DEPTH {
        return Err(io::Error::other(
            "JSON/YAML structure exceeds the preview limit.",
        ));
    }
    Ok(())
}

fn scalar_color(text: &str) -> Color {
    if text.starts_with(['"', '\'']) {
        Color::String
    } else if text.starts_with(|ch: char| ch.is_ascii_digit() || ch == '-') {
        Color::Number
    } else {
        Color::Literal
    }
}

struct Json<'a> {
    rest: &'a str,
    count: usize,
}

impl<'a> Json<'a> {
    fn take(&mut self, delimiter: char) -> bool {
        if let Some(rest) = self.rest.trim_start().strip_prefix(delimiter) {
            self.rest = rest;
            true
        } else {
            false
        }
    }

    fn string(&mut self) -> io::Result<&'a str> {
        let text = self.rest.trim_start();
        if !text.starts_with('"') {
            return Err(invalid("Expected a JSON string"));
        }
        let mut escaped = false;
        for (index, byte) in text.bytes().enumerate().skip(1) {
            if !escaped && byte == b'"' {
                self.rest = &text[index + 1..];
                return Ok(&text[..index + 1]);
            }
            escaped = !escaped && byte == b'\\';
        }
        Err(invalid("Unclosed JSON string"))
    }

    fn node(&mut self, depth: usize) -> io::Result<Node> {
        check_budget(&mut self.count, depth)?;
        if self.take('{') {
            let mut entries = Vec::new();
            if !self.take('}') {
                loop {
                    let key = Node::plain(self.string()?, Color::String);
                    if !self.take(':') {
                        return Err(invalid("Expected a JSON ':'"));
                    }
                    entries.push((key, self.node(depth + 1)?));
                    if self.take('}') {
                        break;
                    }
                    if !self.take(',') {
                        return Err(invalid("Expected a JSON ','"));
                    }
                }
            }
            Ok(Node::container(Value::Map(entries)))
        } else if self.take('[') {
            let mut entries = Vec::new();
            if !self.take(']') {
                loop {
                    entries.push(self.node(depth + 1)?);
                    if self.take(']') {
                        break;
                    }
                    if !self.take(',') {
                        return Err(invalid("Expected a JSON ','"));
                    }
                }
            }
            Ok(Node::container(Value::Seq(entries)))
        } else {
            let text = self.rest.trim_start();
            let token = if text.starts_with('"') {
                self.string()?
            } else {
                let end = text
                    .find(|ch: char| ch.is_ascii_whitespace() || matches!(ch, ',' | '}' | ']'))
                    .unwrap_or(text.len());
                self.rest = &text[end..];
                &text[..end]
            };
            if token.is_empty() {
                return Err(invalid("Expected a JSON value"));
            }
            Ok(Node::plain(token, scalar_color(token)))
        }
    }
}

fn read_yaml(code: &str) -> io::Result<Vec<ParsedDocument<'_>>> {
    let mut reader = Parser::new_from_str(code);
    let mut count = 0;
    yaml_event(&mut reader, Event::StreamStart)?;
    let mut docs = Vec::new();
    let mut lines = code.lines().enumerate().peekable();
    loop {
        match reader.next_token().map_err(io::Error::other)? {
            (Event::StreamEnd, _) => return Ok(docs),
            (Event::DocumentStart, marker) => {
                let mut version = None;
                while lines
                    .peek()
                    .is_some_and(|(index, _)| *index + 1 < marker.line())
                {
                    let (_, line) = lines.next().unwrap();
                    if let Some(tail) = line.strip_prefix("%YAML")
                        && tail.starts_with(char::is_whitespace)
                    {
                        version = tail.split_ascii_whitespace().next();
                    }
                }
                let event = next_yaml(&mut reader)?;
                let root = yaml_node(&mut reader, event, 0, &mut count)?;
                docs.push(ParsedDocument { root, version });
                yaml_event(&mut reader, Event::DocumentEnd)?;
            }
            _ => return Err(invalid("Expected a YAML document")),
        }
    }
}

fn next_yaml<T: Iterator<Item = char>>(parser: &mut Parser<T>) -> io::Result<Event> {
    parser
        .next_token()
        .map(|(event, _)| event)
        .map_err(io::Error::other)
}

fn yaml_event<T: Iterator<Item = char>>(parser: &mut Parser<T>, expected: Event) -> io::Result<()> {
    if next_yaml(parser)? == expected {
        Ok(())
    } else {
        Err(invalid("Unexpected YAML event"))
    }
}

fn decorated(anchor: usize, tag: Option<Tag>) -> String {
    let mut value = String::new();
    if let Some(tag) = tag {
        if tag.handle == "!" {
            value.push('!');
            value.push_str(&tag.suffix);
        } else if tag.handle.is_empty() && tag.suffix == "!" {
            value.push('!');
        } else {
            write!(value, "!<{}{}>", tag.handle, tag.suffix).unwrap();
        }
        value.push(' ');
    }
    if anchor != 0 {
        write!(value, "&a{anchor} ").unwrap();
    }
    value
}

fn yaml_node<T: Iterator<Item = char>>(
    parser: &mut Parser<T>,
    event: Event,
    depth: usize,
    count: &mut usize,
) -> io::Result<Node> {
    check_budget(count, depth)?;
    let (value, anchor, tag) = match event {
        Event::Alias(anchor) => return Ok(Node::plain(&format!("*a{anchor}"), Color::Property)),
        Event::Scalar(text, style, anchor, tag) => {
            let plain = style == TScalarStyle::Plain;
            let value = if !plain {
                serde_json::to_string(&text).map_err(io::Error::other)?
            } else if text.is_empty() {
                "null".to_owned()
            } else {
                text
            };
            let color = if plain {
                scalar_color(&value)
            } else {
                Color::String
            };
            (Value::Scalar(value, color), anchor, tag)
        }
        Event::SequenceStart(anchor, tag) => {
            let mut nodes = Vec::new();
            loop {
                let event = next_yaml(parser)?;
                if event == Event::SequenceEnd {
                    break;
                }
                nodes.push(yaml_node(parser, event, depth + 1, count)?);
            }
            (Value::Seq(nodes), anchor, tag)
        }
        Event::MappingStart(anchor, tag) => {
            let mut nodes = Vec::new();
            loop {
                let event = next_yaml(parser)?;
                if event == Event::MappingEnd {
                    break;
                }
                let key = yaml_node(parser, event, depth + 1, count)?;
                let event = next_yaml(parser)?;
                let value = yaml_node(parser, event, depth + 1, count)?;
                nodes.push((key, value));
            }
            (Value::Map(nodes), anchor, tag)
        }
        _ => return Err(invalid("Expected a YAML value")),
    };
    Ok(Node {
        value,
        decoration: decorated(anchor, tag),
    })
}

#[derive(Default)]
struct Output(String);

impl Write for Output {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.0.len() + text.len() > MAX_OUTPUT {
            return Err(fmt::Error);
        }
        self.0.push_str(text);
        Ok(())
    }
}

fn indent(out: &mut Output, depth: usize) -> fmt::Result {
    const SPACES: &str = "                                                                ";
    let mut remaining = depth * 2;
    while remaining >= SPACES.len() {
        out.write_str(SPACES)?;
        remaining -= SPACES.len();
    }
    out.write_str(&SPACES[..remaining])
}

fn print_documents(out: &mut Output, docs: &[ParsedDocument<'_>], json: bool) -> fmt::Result {
    for (index, document) in docs.iter().enumerate() {
        let doc = &document.root;
        let separator = docs.len() > 1 || document.version.is_some();
        if let Some(version) = document.version {
            if index > 0 {
                out.write_str("...\n")?;
            }
            writeln!(out, "%YAML {version}")?;
        }
        if json {
            flow(out, doc, Some(0))?;
            out.write_char('\n')?;
        } else if doc.compound() {
            if !doc.decoration.is_empty() {
                writeln!(out, "--- {}", doc.decoration.trim_end())?;
            } else if separator {
                out.write_str("---\n")?;
            }
            block(out, doc, 0)?;
        } else {
            if separator {
                out.write_str("--- ")?;
            }
            yaml_leaf(out, doc)?;
            out.write_char('\n')?;
        }
    }
    Ok(())
}

fn flow(out: &mut Output, doc: &Node, depth: Option<usize>) -> fmt::Result {
    out.write_str(&doc.decoration)?;
    let divider = |out: &mut Output, index| {
        if index > 0 {
            out.write_char(',')?;
        }
        if let Some(depth) = depth {
            out.write_char('\n')?;
            indent(out, depth + 1)?;
        } else if index > 0 {
            out.write_char(' ')?;
        }
        Ok(())
    };
    let next = depth.map(|depth| depth + 1);
    let (close, count) = match &doc.value {
        Value::Scalar(value, _) => return out.write_str(value),
        Value::Seq(children) => {
            out.write_char('[')?;
            for (index, value) in children.iter().enumerate() {
                divider(out, index)?;
                flow(out, value, next)?;
            }
            (']', children.len())
        }
        Value::Map(children) => {
            out.write_char('{')?;
            for (index, (key, value)) in children.iter().enumerate() {
                divider(out, index)?;
                if depth.is_none() && key.compound() {
                    out.write_str("? ")?;
                }
                flow(out, key, next)?;
                out.write_str(": ")?;
                flow(out, value, next)?;
            }
            ('}', children.len())
        }
    };
    if count != 0
        && let Some(depth) = depth
    {
        out.write_char('\n')?;
        indent(out, depth)?;
    }
    out.write_char(close)
}

fn yaml_leaf(out: &mut Output, node: &Node) -> fmt::Result {
    out.write_str(&node.decoration)?;
    match &node.value {
        Value::Scalar(text, _) => out.write_str(text),
        Value::Map(_) => out.write_str("{}"),
        Value::Seq(_) => out.write_str("[]"),
    }
}

fn block_value(out: &mut Output, node: &Node, depth: usize) -> fmt::Result {
    if node.compound() {
        if !node.decoration.is_empty() {
            write!(out, " {}", node.decoration.trim_end())?;
        }
        out.write_char('\n')?;
        block(out, node, depth + 1)
    } else {
        out.write_char(' ')?;
        yaml_leaf(out, node)?;
        out.write_char('\n')
    }
}

fn block(out: &mut Output, node: &Node, depth: usize) -> fmt::Result {
    match &node.value {
        Value::Seq(children) => {
            for value in children {
                indent(out, depth)?;
                out.write_char('-')?;
                block_value(out, value, depth)?;
            }
        }
        Value::Map(children) => {
            for (key, value) in children {
                indent(out, depth)?;
                if key.compound() || !key.decoration.is_empty() {
                    out.write_char('?')?;
                    block_value(out, key, depth)?;
                    indent(out, depth)?;
                } else {
                    yaml_leaf(out, key)?;
                }
                out.write_char(':')?;
                block_value(out, value, depth)?;
            }
        }
        Value::Scalar(..) => return Err(fmt::Error),
    }
    Ok(())
}

fn preview(text: &str, limit: usize) -> String {
    let mut text = text.chars();
    let mut result: String = text
        .by_ref()
        .take(limit)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if text.next().is_some() {
        result.push('…');
    }
    result
}

fn key_label(key: &Node) -> String {
    if let Value::Scalar(text, _) = &key.value {
        let decoded = if text.starts_with('"') {
            serde_json::from_str::<String>(text).ok()
        } else {
            None
        };
        let text = decoded.as_deref().unwrap_or(text);
        if text.is_empty() {
            "\"\"".to_owned()
        } else {
            preview(text, 128)
        }
    } else {
        let mut text = Output::default();
        if flow(&mut text, key, None).is_ok() {
            preview(&text.0, 128)
        } else {
            "…".to_owned()
        }
    }
}

fn flatten(
    entries: &mut Vec<Entry>,
    node: &Node,
    key: String,
    depth: usize,
    parent: Option<usize>,
) {
    let id = entries.len();
    let (value, color) = match &node.value {
        Value::Scalar(value, color) => (preview(value, 512), *color),
        Value::Seq(children) => (format!("[{}]", children.len()), Color::Bracket(0)),
        Value::Map(children) => (format!("{{{}}}", children.len()), Color::Bracket(0)),
    };
    entries.push(Entry {
        key,
        value: format!("{}{value}", node.decoration),
        color,
        depth,
        parent,
        end: 0,
    });
    match &node.value {
        Value::Seq(children) => {
            for (index, value) in children.iter().enumerate() {
                flatten(entries, value, index.to_string(), depth + 1, Some(id));
            }
        }
        Value::Map(children) => {
            for (key, value) in children {
                flatten(entries, value, key_label(key), depth + 1, Some(id));
            }
        }
        Value::Scalar(..) => {}
    }
    entries[id].end = entries.len();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_pretty_preserves_order_duplicates_and_number_spelling() {
        let model = parse(
            r#"{"z":1e-11,"a":[true,{"한글":"x\n"}],"a":12345678901234567890123456789}"#,
            Language::Json,
        )
        .unwrap();
        let pretty = model.pretty.unwrap();
        assert!(pretty.contains("\"z\": 1e-11,\n  \"a\": ["));
        assert_eq!(pretty.matches("\"a\":").count(), 2);
        assert!(pretty.contains("12345678901234567890123456789"));
        assert_eq!(model.entries[1].key, "z");
        assert_eq!(model.entries[2].end, 6);
        assert!(parse("{\"bad\": [1,]}", Language::Json).is_err());
    }

    #[test]
    fn yaml_pretty_preserves_multiple_documents_nested_arrays_and_alias_relationships() {
        let source = "---\ncommon: &c\n  - {enabled: true, message: 'hi'}\ncopy: *c\n---\nroot:\n  ? [one, two]\n  : |\n    hello\n";
        let model = parse(source, Language::Yaml).unwrap();
        assert_eq!(model.roots.len(), 2);
        assert!(model.entries.iter().any(|e| e.value == "*a1"));
        let pretty = model.pretty.unwrap();
        assert!(pretty.contains("common: &a1\n  -\n    enabled: true"));
        assert!(pretty.contains("copy: *a1"));
        let round_trip = parse(&pretty, Language::Yaml).unwrap();
        assert_eq!(round_trip.roots, model.roots);
        assert!(round_trip.entries.iter().any(|e| e.value == "\"hello\\n\""));
        assert!(parse("broken: [1,\n", Language::Yaml).is_err());
    }
    #[test]
    fn yaml_pretty_keeps_version_directives_and_non_specific_tags() {
        let source = "%YAML 1.1\n---\nflag: on\ntext: ! plain\n...\n%YAML 1.2\n---\nflag: off\n";
        let model = parse(source, Language::Yaml).unwrap();
        let pretty = model.pretty.unwrap();
        assert!(pretty.starts_with("%YAML 1.1\n---\nflag: on\ntext: ! plain\n"));
        assert!(pretty.contains("...\n%YAML 1.2\n---\nflag: off"));
        assert!(parse(&pretty, Language::Yaml).is_ok());
    }
}
