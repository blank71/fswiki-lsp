//! `FreeStyleWiki` document formatting.

use std::fmt::Write as _;

use unicode_width::UnicodeWidthStr;

use crate::parser::{NodeKind, parse};

/// Horizontal alignment used while padding table cells.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TableAlign {
    #[default]
    Left,
    Right,
}

/// Formatting options compatible with `go-fswiki::FormatOption`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FormatOptions {
    pub table_align: TableAlign,
    /// `None` preserves whether each input table uses a suffix space.
    pub table_cell_suffix_space: Option<bool>,
}

fn pad_left(source: &str, width: usize) -> String {
    format!(
        "{}{}",
        " ".repeat(width.saturating_sub(source.width())),
        source
    )
}

fn pad_right(source: &str, width: usize) -> String {
    format!(
        "{}{}",
        source,
        " ".repeat(width.saturating_sub(source.width()))
    )
}

#[derive(Debug)]
struct TableBuffer {
    rows: Vec<Vec<String>>,
    suffix_spaces: Vec<Vec<bool>>,
    column_widths: Vec<usize>,
    comments: Vec<String>,
    completed_rows: usize,
    current_column: usize,
}

impl TableBuffer {
    fn new() -> Self {
        Self {
            rows: Vec::new(),
            suffix_spaces: Vec::new(),
            column_widths: vec![0],
            comments: Vec::new(),
            completed_rows: 0,
            current_column: 0,
        }
    }

    fn start_row(&mut self) {
        self.current_column = 0;
        self.rows
            .push(vec![String::new(); self.column_widths.len()]);
        self.suffix_spaces
            .push(vec![false; self.column_widths.len()]);
        self.comments.push(String::new());
    }

    fn finish_row(&mut self) {
        self.completed_rows += 1;
    }

    fn start_cell(&mut self) {
        if self.column_widths.len() <= self.current_column {
            for row in &mut self.rows {
                row.push(String::new());
            }
            for row in &mut self.suffix_spaces {
                row.push(false);
            }
            self.column_widths.push(0);
        }
    }

    fn finish_cell(&mut self) {
        self.current_column += 1;
    }

    fn set_cell(&mut self, mut content: String, suffix_space: bool) {
        self.suffix_spaces[self.completed_rows][self.current_column] = suffix_space;
        if content.contains([',', '"']) {
            content = format!("\"{}\"", content.replace('"', "\"\""));
        }
        self.column_widths[self.current_column] =
            self.column_widths[self.current_column].max(content.as_str().width());
        self.rows[self.completed_rows][self.current_column] = content;
    }

    fn push_comment(&mut self, content: &str) {
        if self.completed_rows > 0 {
            let _ = writeln!(self.comments[self.completed_rows - 1], "//{content}");
        }
    }

    fn write_to(self, output: &mut String, options: FormatOptions) {
        let detected_suffix_space = self.rows.iter().enumerate().any(|(row_index, row)| {
            row.iter().enumerate().any(|(column_index, cell)| {
                column_index < row.len() - 1
                    && cell.as_str().width() == self.column_widths[column_index]
                    && self.suffix_spaces[row_index][column_index]
            })
        });
        let insert_suffix_space = options
            .table_cell_suffix_space
            .unwrap_or(detected_suffix_space);

        for (row_index, row) in self.rows.iter().enumerate() {
            for (column_index, source_cell) in row.iter().enumerate() {
                let is_last = column_index == row.len() - 1;
                let cell = match options.table_align {
                    TableAlign::Left if !is_last => {
                        pad_right(source_cell, self.column_widths[column_index])
                    }
                    TableAlign::Left => source_cell.clone(),
                    TableAlign::Right => pad_left(source_cell, self.column_widths[column_index]),
                };
                output.push(',');
                output.push_str(&cell);
                if insert_suffix_space && !is_last {
                    output.push(' ');
                }
            }
            output.push('\n');
            output.push_str(&self.comments[row_index]);
        }
    }
}

fn inline_content(node: &crate::parser::Node) -> String {
    let mut inline = String::new();
    for child in &node.children {
        match child.kind {
            NodeKind::Text => inline.push_str(&child.content),
            NodeKind::SoftBreak => inline.push('\n'),
            NodeKind::StrongOpen | NodeKind::StrongClose => inline.push_str("'''"),
            NodeKind::EmphasisOpen | NodeKind::EmphasisClose => inline.push_str("''"),
            _ => {}
        }
    }
    inline
        .split('\n')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format an entire `FreeStyleWiki` document.
#[must_use]
#[allow(clippy::too_many_lines)] // Keeping the event-to-output mapping in one match is clearer.
pub fn format_document(source: &str, options: FormatOptions) -> String {
    let nodes = parse(source);
    let mut output = String::new();
    let mut list_header = '*';
    let mut list_depth = 0_usize;

    let mut table = None;

    for (index, node) in nodes.iter().enumerate() {
        match node.kind {
            NodeKind::HeadingOpen => {
                let _ = write!(output, "{} ", "!".repeat(usize::from(5 - node.level)));
            }
            NodeKind::HeadingClose | NodeKind::ListItemClose | NodeKind::ParagraphClose => {
                output.push('\n');
            }
            NodeKind::UnorderedListOpen => {
                list_header = '*';
                list_depth += 1;
            }
            NodeKind::OrderedListOpen => {
                list_header = '+';
                list_depth += 1;
            }
            NodeKind::UnorderedListClose | NodeKind::OrderedListClose => {
                list_depth = list_depth.saturating_sub(1);
            }
            NodeKind::ListItemOpen => {
                let _ = write!(output, "{} ", list_header.to_string().repeat(list_depth));
            }
            NodeKind::Preformatted => {
                for line in node.content.lines() {
                    let _ = writeln!(output, " {line}");
                }
            }
            NodeKind::TableOpen => {
                table = Some(TableBuffer::new());
            }
            NodeKind::TableClose => {
                if let Some(table) = table.take() {
                    table.write_to(&mut output, options);
                }
            }
            NodeKind::TableRowOpen => {
                if let Some(table) = table.as_mut() {
                    table.start_row();
                }
            }
            NodeKind::Comment => {
                if let Some(table) = table.as_mut() {
                    table.push_comment(&node.content);
                } else {
                    let _ = writeln!(output, "//{}", node.content);
                }
            }
            NodeKind::TableRowClose => {
                if let Some(table) = table.as_mut() {
                    table.finish_row();
                }
            }
            NodeKind::TableHeaderOpen | NodeKind::TableDataOpen => {
                if let Some(table) = table.as_mut() {
                    table.start_cell();
                }
            }
            NodeKind::TableHeaderClose | NodeKind::TableDataClose => {
                if let Some(table) = table.as_mut() {
                    table.finish_cell();
                }
            }
            NodeKind::Plugin => {
                if node.content.is_empty() {
                    let _ = write!(output, "{{{{{}}}}}", node.tag);
                } else if node.content.starts_with('\n') {
                    let _ = write!(output, "{{{{{}{}}}}}", node.tag, node.content);
                } else {
                    let _ = write!(output, "{{{{{} {}}}}}", node.tag, node.content);
                }
                output.push('\n');
            }
            NodeKind::Inline => {
                let inline = inline_content(node);
                if let Some(table) = table.as_mut() {
                    let suffix_space = node
                        .children
                        .last()
                        .is_some_and(|child| child.content.ends_with(' '));
                    table.set_cell(inline, suffix_space);
                } else {
                    output.push_str(&inline);
                }
            }
            NodeKind::ParagraphOpen
            | NodeKind::StrongOpen
            | NodeKind::StrongClose
            | NodeKind::EmphasisOpen
            | NodeKind::EmphasisClose
            | NodeKind::Text
            | NodeKind::SoftBreak => {}
            NodeKind::BlankLine => {
                if !output.ends_with("\n\n") {
                    output.push('\n');
                }
            }
        }

        if index < nodes.len() - 1 {
            match node.kind {
                NodeKind::HeadingClose
                | NodeKind::ParagraphClose
                | NodeKind::Preformatted
                | NodeKind::Plugin
                | NodeKind::TableClose => output.push('\n'),
                NodeKind::OrderedListClose | NodeKind::UnorderedListClose if list_depth == 0 => {
                    output.push('\n');
                }
                _ => {}
            }
        }
    }

    if source.contains("\r\n") {
        output.replace('\n', "\r\n")
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::{FormatOptions, TableAlign, format_document};

    const TABLE_INPUT: &str = ", aaa, bbb,ccc\n,あいう,かきくけこ, さしす\n,\"あい,\",かき,さし\n";

    #[test]
    fn formats_table_left_without_suffix_space_by_default() {
        let expected =
            ",aaa    ,bbb       ,ccc\n,あいう ,かきくけこ,さしす\n,\"あい,\",かき      ,さし\n";
        assert_eq!(
            format_document(TABLE_INPUT, FormatOptions::default()),
            expected
        );
    }

    #[test]
    fn formats_table_left_with_suffix_space() {
        let expected = ",aaa     ,bbb        ,ccc\n,あいう  ,かきくけこ ,さしす\n,\"あい,\" ,かき       ,さし\n";
        assert_eq!(
            format_document(
                TABLE_INPUT,
                FormatOptions {
                    table_align: TableAlign::Left,
                    table_cell_suffix_space: Some(true),
                }
            ),
            expected
        );
    }

    #[test]
    fn formats_table_right_with_suffix_space() {
        let expected = ",    aaa ,       bbb ,   ccc\n, あいう ,かきくけこ ,さしす\n,\"あい,\" ,      かき ,  さし\n";
        assert_eq!(
            format_document(
                TABLE_INPUT,
                FormatOptions {
                    table_align: TableAlign::Right,
                    table_cell_suffix_space: Some(true),
                }
            ),
            expected
        );
    }

    #[test]
    fn formats_table_right_without_suffix_space() {
        let expected = ",    aaa,       bbb,   ccc\n, あいう,かきくけこ,さしす\n,\"あい,\",      かき,  さし\n";
        assert_eq!(
            format_document(
                TABLE_INPUT,
                FormatOptions {
                    table_align: TableAlign::Right,
                    table_cell_suffix_space: Some(false),
                }
            ),
            expected
        );
    }

    #[test]
    fn formats_document_blocks() {
        let source =
            "!heading\nparagraph line  \ncontinued\n\n***item\n** child\n\n{{pre\nraw\n}}\n";
        let expected =
            "! heading\n\nparagraph line\ncontinued\n\n*** item\n** child\n\n{{pre\nraw\n}}\n";
        assert_eq!(format_document(source, FormatOptions::default()), expected);
    }
}
