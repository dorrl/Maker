use std::{fmt::Write, io};

use roxmltree::{Document, Node as XmlNode, ParsingOptions};
use scraper::{ElementRef, Html, Node};

use super::model::{Entry, MAX_NODES, Model, Output, check_budget, indent, preview};
use crate::text::{Color, Language};

pub(super) fn parse(code: &str, language: Language) -> io::Result<Model> {
    let mut entries = Vec::new();
    let mut roots = Vec::new();
    let mut output = Output::default();
    let pretty = if language == Language::Html {
        let document = Html::parse_document(code);
        let root = document.root_element();
        html_tree(root, &mut entries, 0, None)?;
        roots.push(0);
        let result = (|| {
            for child in document.tree.root().children() {
                match child.value() {
                    Node::Doctype(doctype) => writeln!(output, "<!DOCTYPE {}>", doctype.name())?,
                    Node::Comment(comment) => writeln!(output, "<!--{}-->", &**comment)?,
                    _ if child.id() == root.id() => html_print(root, &mut output, 0)?,
                    _ => {}
                }
            }
            Ok::<_, std::fmt::Error>(())
        })();
        result.is_ok().then_some(output.0)
    } else {
        let document = Document::parse_with_options(
            code,
            ParsingOptions {
                allow_dtd: true,
                nodes_limit: MAX_NODES as u32,
                ..ParsingOptions::default()
            },
        )
        .map_err(io::Error::other)?;
        let root = document.root_element();
        xml_tree(root, code, &mut entries, 0, None)?;
        roots.push(0);
        let result = (|| {
            output.write_str(code[..root.range().start].trim_start())?;
            xml_print(root, code, &mut output, 0)?;
            output.write_str(code[root.range().end..].trim_end())?;
            Ok::<_, std::fmt::Error>(())
        })();
        result.is_ok().then_some(output.0)
    };
    Ok(Model {
        pretty,
        entries,
        roots,
    })
}

fn row(
    entries: &mut Vec<Entry>,
    key: String,
    value: String,
    color: Color,
    depth: usize,
    parent: Option<usize>,
) -> io::Result<usize> {
    let mut length = entries.len();
    check_budget(&mut length, depth)?;
    let index = entries.len();
    entries.push(Entry {
        key,
        value: preview(&value, 512),
        color,
        depth,
        parent,
        end: index + 1,
    });
    Ok(index)
}

fn html_tree(
    element: ElementRef<'_>,
    entries: &mut Vec<Entry>,
    depth: usize,
    parent: Option<usize>,
) -> io::Result<()> {
    let attributes = element.value().attrs().count();
    let id = row(
        entries,
        format!("<{}>", element.value().name()),
        if attributes > 0 {
            format!("[{attributes}]")
        } else {
            String::new()
        },
        Color::Markup,
        depth,
        parent,
    )?;
    for (name, value) in element.value().attrs() {
        row(
            entries,
            format!("@{name}"),
            value.to_owned(),
            Color::String,
            depth + 1,
            Some(id),
        )?;
    }
    for child in element.children() {
        match child.value() {
            Node::Text(text) if !text.trim().is_empty() => {
                row(
                    entries,
                    String::new(),
                    text.trim().to_owned(),
                    Color::String,
                    depth + 1,
                    Some(id),
                )?;
            }
            Node::Comment(comment) => {
                row(
                    entries,
                    String::new(),
                    format!("<!--{}-->", &**comment),
                    Color::Comment,
                    depth + 1,
                    Some(id),
                )?;
            }
            _ => {
                if let Some(element) = ElementRef::wrap(child) {
                    html_tree(element, entries, depth + 1, Some(id))?;
                }
            }
        }
    }
    entries[id].end = entries.len();
    Ok(())
}

fn html_print(element: ElementRef<'_>, out: &mut Output, depth: usize) -> std::fmt::Result {
    let name = element.value().name();
    // Mixed text, inline markup and raw-text elements keep their text boundaries.
    let container = matches!(
        name,
        "html"
            | "head"
            | "body"
            | "main"
            | "header"
            | "footer"
            | "div"
            | "section"
            | "article"
            | "nav"
            | "aside"
            | "ul"
            | "ol"
            | "table"
            | "tbody"
            | "thead"
            | "tfoot"
            | "tr"
            | "form"
            | "fieldset"
    );
    let blocks = element.children().all(|child| match child.value() {
        Node::Text(text) => text.trim().is_empty(),
        Node::Element(element) => matches!(
            element.name(),
            "head"
                | "body"
                | "meta"
                | "link"
                | "title"
                | "base"
                | "style"
                | "script"
                | "p"
                | "div"
                | "section"
                | "article"
                | "header"
                | "footer"
                | "main"
                | "nav"
                | "aside"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "ul"
                | "ol"
                | "li"
                | "table"
                | "tbody"
                | "thead"
                | "tfoot"
                | "tr"
                | "td"
                | "th"
                | "form"
                | "fieldset"
                | "pre"
                | "blockquote"
                | "hr"
                | "svg"
        ),
        Node::Comment(_) => true,
        _ => false,
    });
    indent(out, depth)?;
    if !container
        || !blocks
        || !element
            .children()
            .any(|child| !matches!(child.value(), Node::Text(_)))
    {
        return writeln!(out, "{}", element.html());
    }
    write!(out, "<{name}")?;
    for (key, value) in element.value().attrs() {
        write!(out, " {key}=\"")?;
        for ch in value.chars() {
            match ch {
                '&' => out.write_str("&amp;")?,
                '"' => out.write_str("&quot;")?,
                _ => out.write_char(ch)?,
            }
        }
        out.write_char('"')?;
    }
    out.write_str(">\n")?;
    for child in element.children() {
        if let Some(child) = ElementRef::wrap(child) {
            html_print(child, out, depth + 1)?;
        } else if let Node::Comment(comment) = child.value() {
            indent(out, depth + 1)?;
            writeln!(out, "<!--{}-->", &**comment)?;
        }
    }
    indent(out, depth)?;
    writeln!(out, "</{name}>")
}

fn xml_tree(
    node: XmlNode<'_, '_>,
    code: &str,
    entries: &mut Vec<Entry>,
    depth: usize,
    parent: Option<usize>,
) -> io::Result<()> {
    let id = if node.is_element() {
        let name = node.tag_name();
        let prefix = name.namespace().and_then(|ns| node.lookup_prefix(ns));
        let name = prefix.map_or_else(
            || name.name().to_owned(),
            |prefix| format!("{prefix}:{}", name.name()),
        );
        row(
            entries,
            format!("<{name}>"),
            String::new(),
            Color::Markup,
            depth,
            parent,
        )?
    } else {
        let color = if node.is_comment() {
            Color::Comment
        } else {
            Color::String
        };
        return row(
            entries,
            String::new(),
            code[node.range()].to_owned(),
            color,
            depth,
            parent,
        )
        .map(|_| ());
    };
    for attr in node.attributes() {
        let prefix = attr.namespace().and_then(|ns| node.lookup_prefix(ns));
        let name = prefix.map_or_else(
            || format!("@{}", attr.name()),
            |prefix| format!("@{prefix}:{}", attr.name()),
        );
        row(
            entries,
            name,
            attr.value().to_owned(),
            Color::String,
            depth + 1,
            Some(id),
        )?;
    }
    for child in node.children() {
        if !child.is_text() || child.text().is_some_and(|text| !text.trim().is_empty()) {
            xml_tree(child, code, entries, depth + 1, Some(id))?;
        }
    }
    entries[id].end = entries.len();
    Ok(())
}

fn xml_print(
    node: XmlNode<'_, '_>,
    code: &str,
    out: &mut Output,
    depth: usize,
) -> std::fmt::Result {
    let source = &code[node.range()];
    indent(out, depth)?;
    let content = node.is_element() && node.children().any(|child| !child.is_text());
    let preserve = node
        .ancestors()
        .find_map(|ancestor| ancestor.attribute(("http://www.w3.org/XML/1998/namespace", "space")))
        == Some("preserve")
        || node.children().any(|child| {
            child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty())
        })
        || matches!(
            node.tag_name().name(),
            "text" | "tspan" | "textPath" | "script" | "style" | "foreignObject"
        );
    if !content || preserve {
        return writeln!(out, "{source}");
    }
    let mut quoted = None;
    let Some(opening) = source.char_indices().find_map(|(at, ch)| {
        match quoted {
            Some(quote) if quote == ch => quoted = None,
            None if matches!(ch, '\'' | '"') => quoted = Some(ch),
            None if ch == '>' => return Some(at + 1),
            _ => {}
        }
        None
    }) else {
        return Err(std::fmt::Error);
    };
    let Some(closing) = source.rfind("</") else {
        return Err(std::fmt::Error);
    };
    writeln!(out, "{}", &source[..opening])?;
    for child in node.children().filter(|child| !child.is_text()) {
        xml_print(child, code, out, depth + 1)?;
    }
    indent(out, depth)?;
    writeln!(out, "{}", &source[closing..])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html_keeps_inline_text_and_handles_implied_tags() {
        let result = parse("<!doctype html><main><p>Hi <strong>there</strong><p>Again<script>if (a < b) a++;</script></main>", Language::Html).unwrap();
        let pretty = result.pretty.unwrap();
        assert!(pretty.contains("Hi <strong>there</strong>") && pretty.contains("if (a < b) a++;"));
        assert_eq!(
            result.entries.iter().filter(|row| row.key == "<p>").count(),
            2
        );
    }

    #[test]
    fn svg_keeps_namespaces_and_mixed_content() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"a>b\"><text>A <tspan>B</tspan></text><path d=\"M 1 2\"/></g></svg>";
        let result = parse(input, Language::Xml).unwrap();
        let pretty = result.pretty.unwrap();
        assert!(
            pretty.contains("\n  <g id=\"a>b\">\n")
                && pretty.contains("<text>A <tspan>B</tspan></text>")
        );
        assert!(Document::parse(&pretty).is_ok());
        assert!(result.entries.iter().any(|x| x.key == "@d"));
    }
}
