//! Parser compatible with the subset implemented by `go-fswiki`.

use crate::syntax::MAX_LIST_LEVEL;

/// The kind of a parsed `FreeStyleWiki` node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    HeadingOpen,
    HeadingClose,
    UnorderedListOpen,
    UnorderedListClose,
    OrderedListOpen,
    OrderedListClose,
    ListItemOpen,
    ListItemClose,
    ParagraphOpen,
    ParagraphClose,
    Preformatted,
    TableOpen,
    TableClose,
    TableHeaderOpen,
    TableHeaderClose,
    TableRowOpen,
    TableRowClose,
    TableDataOpen,
    TableDataClose,
    StrongOpen,
    StrongClose,
    EmphasisOpen,
    EmphasisClose,
    Inline,
    Text,
    SoftBreak,
    BlankLine,
    Comment,
    Plugin,
}

/// A node emitted by the line-oriented parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub tag: String,
    pub content: String,
    pub level: u8,
    pub children: Vec<Self>,
}

impl Node {
    fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            tag: String::new(),
            content: String::new(),
            level: 0,
            children: Vec::new(),
        }
    }

    fn with_content(kind: NodeKind, content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            ..Self::new(kind)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ListElementType {
    #[default]
    Unknown,
    Unordered,
    Ordered,
}

#[derive(Clone, Copy, Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // Mirrors go-fswiki's independent parser states.
struct ParserState {
    is_paragraph: bool,
    list_depth: usize,
    list_type: ListElementType,
    is_preformatted: bool,
    is_table: bool,
    is_plugin: bool,
}

#[derive(Debug, Default)]
struct Parser {
    current: ParserState,
    previous: ParserState,
    list_types: [ListElementType; MAX_LIST_LEVEL],
}

impl Parser {
    fn parse_delimited(
        &self,
        source: &str,
        marker: &str,
        open: NodeKind,
        close: NodeKind,
    ) -> Option<(Vec<Node>, usize)> {
        let body = source.strip_prefix(marker)?;
        let end = body.find(marker)?;
        let content = &body[..end];
        let nodes = std::iter::once(Node::new(open))
            .chain(self.parse_inline_children(content))
            .chain(std::iter::once(Node::new(close)))
            .collect();
        Some((nodes, marker.len() + end + marker.len()))
    }

    fn parse_inline_children(&self, source: &str) -> Vec<Node> {
        let mut nodes = Vec::new();
        let mut cursor = 0;
        let mut text_start = 0;

        while cursor < source.len() {
            let rest = &source[cursor..];
            let parsed = if rest.starts_with("'''") {
                self.parse_delimited(rest, "'''", NodeKind::StrongOpen, NodeKind::StrongClose)
            } else if rest.starts_with("''") {
                self.parse_delimited(rest, "''", NodeKind::EmphasisOpen, NodeKind::EmphasisClose)
            } else {
                None
            };

            if let Some((parsed_nodes, consumed)) = parsed {
                nodes.push(Node::with_content(
                    NodeKind::Text,
                    &source[text_start..cursor],
                ));
                nodes.extend(parsed_nodes);
                cursor += consumed;
                text_start = cursor;
            } else {
                cursor += rest.chars().next().map_or(1, char::len_utf8);
            }
        }

        if text_start < source.len() {
            nodes.push(Node::with_content(NodeKind::Text, &source[text_start..]));
        }
        nodes
    }

    fn parse_inline(&self, nodes: &mut Vec<Node>, source: &str) {
        if nodes
            .last()
            .is_none_or(|node| node.kind != NodeKind::Inline)
        {
            nodes.push(Node::new(NodeKind::Inline));
        } else if let Some(node) = nodes.last_mut() {
            node.children.push(Node::new(NodeKind::SoftBreak));
        }

        if let Some(node) = nodes.last_mut() {
            node.children.extend(self.parse_inline_children(source));
        }
    }

    fn parse_multiline_markup(&mut self, nodes: &mut Vec<Node>) {
        if !self.previous.is_paragraph && self.current.is_paragraph {
            nodes.push(Node::new(NodeKind::ParagraphOpen));
        }
        if self.previous.is_paragraph && !self.current.is_paragraph {
            nodes.push(Node::new(NodeKind::ParagraphClose));
        }

        if self.previous.list_type != self.current.list_type {
            let close_kind = match self.previous.list_type {
                ListElementType::Ordered => Some(NodeKind::OrderedListClose),
                ListElementType::Unordered => Some(NodeKind::UnorderedListClose),
                ListElementType::Unknown => None,
            };
            if let Some(kind) = close_kind {
                nodes.extend((0..self.previous.list_depth).map(|_| Node::new(kind)));
            }
            self.previous.list_depth = 0;
        }

        if self.previous.list_depth < self.current.list_depth {
            let open_kind = match self.current.list_type {
                ListElementType::Ordered => Some(NodeKind::OrderedListOpen),
                ListElementType::Unordered => Some(NodeKind::UnorderedListOpen),
                ListElementType::Unknown => None,
            };
            if let Some(kind) = open_kind {
                for depth in self.previous.list_depth..self.current.list_depth {
                    self.list_types[depth] = self.current.list_type;
                    nodes.push(Node::new(kind));
                }
            }
        }

        if self.previous.list_depth > self.current.list_depth {
            for depth in (self.current.list_depth..self.previous.list_depth).rev() {
                let close_kind = match self.list_types[depth] {
                    ListElementType::Ordered => Some(NodeKind::OrderedListClose),
                    ListElementType::Unordered => Some(NodeKind::UnorderedListClose),
                    ListElementType::Unknown => None,
                };
                if let Some(kind) = close_kind {
                    nodes.push(Node::new(kind));
                }
            }
        }

        if !self.previous.is_preformatted && self.current.is_preformatted {
            nodes.push(Node::new(NodeKind::Preformatted));
        }
        if !self.previous.is_table && self.current.is_table {
            nodes.push(Node::new(NodeKind::TableOpen));
        }
        if self.previous.is_table && !self.current.is_table {
            nodes.push(Node::new(NodeKind::TableClose));
        }
    }

    fn parse_heading(&mut self, nodes: &mut Vec<Node>, marker_length: u8, source: &str) {
        self.parse_multiline_markup(nodes);
        let level = 5 - marker_length;
        let mut open = Node::new(NodeKind::HeadingOpen);
        open.level = level;
        nodes.push(open);
        self.parse_inline(nodes, source);
        let mut close = Node::new(NodeKind::HeadingClose);
        close.level = level;
        nodes.push(close);
    }

    fn parse_list(
        &mut self,
        nodes: &mut Vec<Node>,
        depth: usize,
        list_type: ListElementType,
        source: &str,
    ) {
        self.current.list_depth = depth;
        self.current.list_type = list_type;
        self.parse_multiline_markup(nodes);
        nodes.push(Node::new(NodeKind::ListItemOpen));
        self.parse_inline(nodes, source);
        nodes.push(Node::new(NodeKind::ListItemClose));
    }

    fn parse_preformatted(&mut self, nodes: &mut Vec<Node>, source: &str) {
        self.current.is_preformatted = true;
        self.parse_multiline_markup(nodes);
        if let Some(node) = nodes
            .last_mut()
            .filter(|node| node.kind == NodeKind::Preformatted)
        {
            if node.content.is_empty() {
                node.content.push_str(source);
            } else {
                node.content.push('\n');
                node.content.push_str(source);
            }
        }
    }

    fn parse_table(&mut self, nodes: &mut Vec<Node>, source: &str) {
        self.current.is_table = true;
        self.parse_multiline_markup(nodes);
        let (open_kind, close_kind) = if self.previous.is_table {
            (NodeKind::TableDataOpen, NodeKind::TableDataClose)
        } else {
            (NodeKind::TableHeaderOpen, NodeKind::TableHeaderClose)
        };

        let parse_cell = |nodes: &mut Vec<Node>, content: &str| {
            nodes.push(Node::new(open_kind));
            self.parse_inline(nodes, content);
            nodes.push(Node::new(close_kind));
        };

        nodes.push(Node::new(NodeKind::TableRowOpen));
        let mut cursor = 0;
        while let Some(relative_delimiter) = source[cursor..].find(',') {
            let delimiter = cursor + relative_delimiter;
            let start = delimiter + 1;

            if source[start..].starts_with('"') {
                let mut content = String::new();
                let mut position = start + 1;
                let mut closed = false;
                while position < source.len() {
                    let rest = &source[position..];
                    let character = rest.chars().next().expect("position is in bounds");
                    if character == '"' {
                        if rest[character.len_utf8()..].starts_with('"') {
                            content.push('"');
                            position += character.len_utf8() * 2;
                        } else {
                            position += character.len_utf8();
                            closed = true;
                            break;
                        }
                    } else {
                        content.push(character);
                        position += character.len_utf8();
                    }
                }

                if closed {
                    let end = source[position..]
                        .find(',')
                        .map_or(source.len(), |relative| position + relative);
                    content.push_str(&source[position..end]);
                    parse_cell(nodes, &content);
                    cursor = end;
                    continue;
                }
            }

            let end = source[start..]
                .find(',')
                .map_or(source.len(), |relative| start + relative);
            parse_cell(nodes, &source[start..end]);
            cursor = end;
        }
        nodes.push(Node::new(NodeKind::TableRowClose));
    }

    fn parse_comment(&mut self, nodes: &mut Vec<Node>, source: &str) {
        nodes.push(Node::with_content(NodeKind::Comment, source));
        self.current = self.previous;
    }

    fn parse_plugin(&mut self, nodes: &mut Vec<Node>, source: &str) {
        self.current.is_plugin = true;
        self.parse_multiline_markup(nodes);

        let source = if let Some(stripped) = source.strip_suffix("}}") {
            self.current.is_plugin = false;
            stripped
        } else {
            source
        };

        let (tag, content) = source.split_once(' ').map_or_else(
            || {
                let content = if self.current.is_plugin { "\n" } else { "" };
                (source, content)
            },
            |(tag, content)| (tag, content.trim()),
        );
        let mut node = Node::new(NodeKind::Plugin);
        node.tag.push_str(tag);
        node.content.push_str(content);
        nodes.push(node);
    }

    fn parse_paragraph(&mut self, nodes: &mut Vec<Node>, source: &str) {
        self.current.is_paragraph = true;
        self.parse_multiline_markup(nodes);
        self.parse_inline(nodes, source);
    }

    fn parse_line(&mut self, nodes: &mut Vec<Node>, line: &str) {
        if self.previous.is_plugin {
            if line == "}}" {
                self.previous.is_plugin = false;
            } else if let Some(node) = nodes.last_mut() {
                node.content.push_str(line);
                node.content.push('\n');
            }
            return;
        }

        if line.is_empty() {
            self.parse_multiline_markup(nodes);
            nodes.push(Node::new(NodeKind::BlankLine));
        } else if let Some(source) = line.strip_prefix("!!!") {
            self.parse_heading(nodes, 3, trim_one_space(source));
        } else if let Some(source) = line.strip_prefix("!!") {
            self.parse_heading(nodes, 2, trim_one_space(source));
        } else if let Some(source) = line.strip_prefix('!') {
            self.parse_heading(nodes, 1, trim_one_space(source));
        } else if let Some(source) = line.strip_prefix("***") {
            self.parse_list(nodes, 3, ListElementType::Unordered, trim_one_space(source));
        } else if let Some(source) = line.strip_prefix("**") {
            self.parse_list(nodes, 2, ListElementType::Unordered, trim_one_space(source));
        } else if let Some(source) = line.strip_prefix('*') {
            self.parse_list(nodes, 1, ListElementType::Unordered, trim_one_space(source));
        } else if let Some(source) = line.strip_prefix("+++") {
            self.parse_list(nodes, 3, ListElementType::Ordered, trim_one_space(source));
        } else if let Some(source) = line.strip_prefix("++") {
            self.parse_list(nodes, 2, ListElementType::Ordered, trim_one_space(source));
        } else if let Some(source) = line.strip_prefix('+') {
            self.parse_list(nodes, 1, ListElementType::Ordered, trim_one_space(source));
        } else if let Some(source) = line.strip_prefix(' ') {
            self.parse_preformatted(nodes, source);
        } else if line.starts_with(',') {
            self.parse_table(nodes, line);
        } else if let Some(source) = line.strip_prefix("//") {
            self.parse_comment(nodes, source);
        } else if let Some(source) = line.strip_prefix("{{") {
            self.parse_plugin(nodes, trim_one_space(source));
        } else {
            self.parse_paragraph(nodes, line);
        }

        self.previous = self.current;
        self.current = ParserState::default();
    }
}

fn trim_one_space(source: &str) -> &str {
    source.strip_prefix(' ').unwrap_or(source)
}

/// Parse a `FreeStyleWiki` document into the same event-like node stream as `go-fswiki`.
#[must_use]
pub fn parse(source: &str) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut parser = Parser::default();

    // `bufio.Scanner` in go-fswiki does not emit a final empty token for a trailing newline.
    for line in source.lines() {
        parser.parse_line(&mut nodes, line.strip_suffix('\r').unwrap_or(line));
    }
    parser.parse_multiline_markup(&mut nodes);
    nodes
}

#[cfg(test)]
mod tests {
    use super::{NodeKind, parse};

    #[test]
    fn parses_inline_markup() {
        let nodes = parse("! a '''bold''' and ''italic''\n");
        let inline = nodes
            .iter()
            .find(|node| node.kind == NodeKind::Inline)
            .expect("inline node");
        assert!(
            inline
                .children
                .iter()
                .any(|node| node.kind == NodeKind::StrongOpen)
        );
        assert!(
            inline
                .children
                .iter()
                .any(|node| node.kind == NodeKind::EmphasisOpen)
        );
    }

    #[test]
    fn keeps_multiline_plugin_content() {
        let nodes = parse("{{pre\n! not a heading\n}}\n");
        let plugin = nodes
            .iter()
            .find(|node| node.kind == NodeKind::Plugin)
            .expect("plugin node");
        assert_eq!(plugin.tag, "pre");
        assert_eq!(plugin.content, "\n! not a heading\n");
    }

    #[test]
    fn parses_escaped_quotes_in_table_cells() {
        let nodes = parse(",\"セルの\"\"引用\"\"\"\n");
        let inline = nodes
            .iter()
            .find(|node| node.kind == NodeKind::Inline)
            .expect("inline table cell");
        assert_eq!(inline.children[0].content, "セルの\"引用\"");
    }
}
