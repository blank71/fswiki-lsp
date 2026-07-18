//! Syntax and structural diagnostics for `FreeStyleWiki` documents.

use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::{
    syntax::{MAX_HEADING_LEVEL, MAX_LIST_LEVEL, heading_line, list_marker},
    text::{document_lines, utf16_len},
};

const SOURCE: &str = "fswiki-lsp";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Rule {
    HeadingMarkerTooLong,
    EmptyHeading,
    HeadingLevelJump,
    ListMarkerTooLong,
    ListLevelJump,
    UnclosedTableCellQuote,
    UnexpectedTableCellText,
    InvalidPluginName,
    MissingRefTarget,
    UnexpectedOutlineArguments,
    InvalidPreformattedPlugin,
    UnclosedPlugin,
    OrphanPluginClose,
    UnclosedInternalLink,
    UnclosedInlineMarkup,
    EmptyInlineMarkup,
    TableColumnCount,
    UnclosedPreformattedBlock,
}

impl Rule {
    const fn code(self) -> &'static str {
        match self {
            Self::HeadingMarkerTooLong => "fswiki/heading-marker-too-long",
            Self::EmptyHeading => "fswiki/empty-heading",
            Self::HeadingLevelJump => "fswiki/heading-level-jump",
            Self::ListMarkerTooLong => "fswiki/list-marker-too-long",
            Self::ListLevelJump => "fswiki/list-level-jump",
            Self::UnclosedTableCellQuote => "fswiki/unclosed-table-cell-quote",
            Self::UnexpectedTableCellText => "fswiki/unexpected-table-cell-text",
            Self::InvalidPluginName => "fswiki/invalid-plugin-name",
            Self::MissingRefTarget => "fswiki/missing-ref-target",
            Self::UnexpectedOutlineArguments => "fswiki/unexpected-outline-arguments",
            Self::InvalidPreformattedPlugin => "fswiki/invalid-preformatted-plugin",
            Self::UnclosedPlugin => "fswiki/unclosed-plugin",
            Self::OrphanPluginClose => "fswiki/orphan-plugin-close",
            Self::UnclosedInternalLink => "fswiki/unclosed-internal-link",
            Self::UnclosedInlineMarkup => "fswiki/unclosed-inline-markup",
            Self::EmptyInlineMarkup => "fswiki/empty-inline-markup",
            Self::TableColumnCount => "fswiki/table-column-count",
            Self::UnclosedPreformattedBlock => "fswiki/unclosed-preformatted-block",
        }
    }

    const fn severity(self) -> DiagnosticSeverity {
        match self {
            Self::HeadingMarkerTooLong
            | Self::EmptyHeading
            | Self::HeadingLevelJump
            | Self::ListMarkerTooLong
            | Self::ListLevelJump
            | Self::MissingRefTarget
            | Self::UnexpectedOutlineArguments
            | Self::OrphanPluginClose
            | Self::TableColumnCount => DiagnosticSeverity::WARNING,
            Self::UnclosedTableCellQuote
            | Self::UnexpectedTableCellText
            | Self::InvalidPluginName
            | Self::InvalidPreformattedPlugin
            | Self::UnclosedPlugin
            | Self::UnclosedInternalLink
            | Self::UnclosedInlineMarkup
            | Self::EmptyInlineMarkup
            | Self::UnclosedPreformattedBlock => DiagnosticSeverity::ERROR,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PreformattedStart {
    line: u32,
    marker_end: usize,
}

#[derive(Clone, Copy, Debug)]
struct ListState {
    marker: u8,
    level: usize,
}

#[derive(Debug, Default)]
struct StructureState {
    heading_levels: [bool; MAX_HEADING_LEVEL + 1],
    list: Option<ListState>,
    table_columns: Option<usize>,
}

fn diagnostic(
    line_number: u32,
    line: &str,
    start: usize,
    end: usize,
    rule: Rule,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic::new(
        Range::new(
            Position::new(line_number, utf16_len(&line[..start])),
            Position::new(line_number, utf16_len(&line[..end])),
        ),
        Some(rule.severity()),
        Some(NumberOrString::String(rule.code().to_string())),
        Some(SOURCE.to_string()),
        message.into(),
        None,
        None,
    )
}

fn validate_heading(
    line_number: u32,
    line: &str,
    state: &mut StructureState,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(heading) = heading_line(line) else {
        return false;
    };
    let marker_length = heading.marker_run;

    if marker_length > MAX_HEADING_LEVEL {
        diagnostics.push(diagnostic(
            line_number,
            line,
            0,
            marker_length,
            Rule::HeadingMarkerTooLong,
            "Heading markers may contain at most three '!'.",
        ));
    }

    if line[marker_length..].trim().is_empty() {
        diagnostics.push(diagnostic(
            line_number,
            line,
            0,
            marker_length,
            Rule::EmptyHeading,
            "Heading title is empty.",
        ));
    }

    let level = heading.level;
    if level > 1 && !state.heading_levels[level - 1] {
        diagnostics.push(diagnostic(
            line_number,
            line,
            0,
            marker_length,
            Rule::HeadingLevelJump,
            format!("Heading level {level} has no level {} parent.", level - 1),
        ));
    }
    for present in &mut state.heading_levels[level..] {
        *present = false;
    }
    state.heading_levels[level] = true;
    true
}

fn validate_list(
    line_number: u32,
    line: &str,
    state: &mut StructureState,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(parsed) = list_marker(line) else {
        return false;
    };
    let marker = parsed.marker;
    let marker_length = parsed.marker_run;
    if marker_length > MAX_LIST_LEVEL {
        diagnostics.push(diagnostic(
            line_number,
            line,
            0,
            marker_length,
            Rule::ListMarkerTooLong,
            format!(
                "List markers may contain at most three '{}'.",
                char::from(marker)
            ),
        ));
    }

    let level = parsed.level;
    let preceding_level = state
        .list
        .filter(|previous| previous.marker == marker)
        .map_or(0, |previous| previous.level);
    if level > preceding_level + 1 {
        diagnostics.push(diagnostic(
            line_number,
            line,
            0,
            marker_length,
            Rule::ListLevelJump,
            format!(
                "List level {level} has no preceding level {} item of the same type.",
                level - 1
            ),
        ));
    }
    state.list = Some(ListState { marker, level });
    true
}

fn validate_table(
    line_number: u32,
    line: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut cursor = 0;
    let mut columns = 0;

    while cursor < bytes.len() && bytes[cursor] == b',' {
        columns += 1;
        cursor += 1;
        if cursor < bytes.len() && bytes[cursor] == b'"' {
            let quote_start = cursor;
            cursor += 1;
            let mut closed = false;
            while cursor < bytes.len() {
                if bytes[cursor] == b'"' {
                    if bytes.get(cursor + 1) == Some(&b'"') {
                        cursor += 2;
                    } else {
                        cursor += 1;
                        closed = true;
                        break;
                    }
                } else {
                    cursor += 1;
                }
            }
            if !closed {
                diagnostics.push(diagnostic(
                    line_number,
                    line,
                    quote_start,
                    line.len(),
                    Rule::UnclosedTableCellQuote,
                    "Unclosed table cell quote.",
                ));
                return None;
            }

            let suffix_start = cursor;
            while cursor < bytes.len() && bytes[cursor] != b',' {
                cursor += 1;
            }
            if let Some(relative) =
                line[suffix_start..cursor].find(|character: char| !matches!(character, ' ' | '\t'))
            {
                let unexpected = suffix_start + relative;
                let unexpected_end =
                    unexpected + line[unexpected..].chars().next().map_or(0, char::len_utf8);
                diagnostics.push(diagnostic(
                    line_number,
                    line,
                    unexpected,
                    unexpected_end,
                    Rule::UnexpectedTableCellText,
                    "Unexpected text after a closing table cell quote.",
                ));
            }
        } else {
            while cursor < bytes.len() && bytes[cursor] != b',' {
                cursor += 1;
            }
        }
    }

    Some(columns)
}

fn validate_plugin(
    line_number: u32,
    line: &str,
    start: usize,
    close: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let inner_start = start + 2;
    let inner = &line[inner_start..close];
    let trimmed = inner.trim_start_matches([' ', '\t']);
    let name_start = inner_start + inner.len() - trimmed.len();
    let name_length = trimmed
        .find(|character: char| character.is_whitespace())
        .unwrap_or(trimmed.len());
    let name = &trimmed[..name_length];
    let valid_name = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');

    if !valid_name {
        let end = (name_start + name.len()).max(start + 2);
        diagnostics.push(diagnostic(
            line_number,
            line,
            start,
            end,
            Rule::InvalidPluginName,
            "Plugin names may contain only ASCII letters, digits, and underscores.",
        ));
        return;
    }

    let arguments = trimmed[name_length..].trim();
    match name {
        "ref" if arguments.is_empty() => diagnostics.push(diagnostic(
            line_number,
            line,
            start,
            close + 2,
            Rule::MissingRefTarget,
            "The ref plugin requires a target.",
        )),
        "outline" if !arguments.is_empty() => diagnostics.push(diagnostic(
            line_number,
            line,
            start,
            close + 2,
            Rule::UnexpectedOutlineArguments,
            "The outline plugin does not accept arguments.",
        )),
        "pre" => diagnostics.push(diagnostic(
            line_number,
            line,
            start,
            close + 2,
            Rule::InvalidPreformattedPlugin,
            "The pre plugin must use the multiline form with a closing '}}' line.",
        )),
        _ => {}
    }
}

fn validate_inline(line_number: u32, line: &str, diagnostics: &mut Vec<Diagnostic>) {
    let mut cursor = 0;
    while cursor < line.len() {
        let rest = &line[cursor..];
        if rest.starts_with("http://") || rest.starts_with("https://") {
            cursor += rest.find(char::is_whitespace).unwrap_or(rest.len());
            continue;
        }

        if rest.starts_with("{{") {
            let Some(relative_close) = line[cursor + 2..].find("}}") else {
                diagnostics.push(diagnostic(
                    line_number,
                    line,
                    cursor,
                    line.len(),
                    Rule::UnclosedPlugin,
                    "Unclosed inline plugin. Expected '}}' before the end of the line.",
                ));
                break;
            };
            let close = cursor + 2 + relative_close;
            validate_plugin(line_number, line, cursor, close, diagnostics);
            cursor = close + 2;
            continue;
        }

        if rest.starts_with("}}") {
            diagnostics.push(diagnostic(
                line_number,
                line,
                cursor,
                cursor + 2,
                Rule::OrphanPluginClose,
                "Closing '}}' has no matching plugin opener.",
            ));
            cursor += 2;
            continue;
        }

        if rest.starts_with("[[") {
            let Some(relative_close) = line[cursor + 2..].find("]]") else {
                diagnostics.push(diagnostic(
                    line_number,
                    line,
                    cursor,
                    line.len(),
                    Rule::UnclosedInternalLink,
                    "Unclosed internal link. Expected ']]' before the end of the line.",
                ));
                break;
            };
            cursor += 2 + relative_close + 2;
            continue;
        }

        let marker = if rest.starts_with("'''") {
            Some("'''")
        } else if rest.starts_with("''") {
            Some("''")
        } else if rest.starts_with("==") {
            Some("==")
        } else if rest.starts_with("__") {
            Some("__")
        } else {
            None
        };
        if let Some(marker) = marker {
            let body_start = cursor + marker.len();
            let Some(relative_close) = line[body_start..].find(marker) else {
                diagnostics.push(diagnostic(
                    line_number,
                    line,
                    cursor,
                    line.len(),
                    Rule::UnclosedInlineMarkup,
                    format!(
                        "Unclosed inline markup. Expected '{marker}' before the end of the line."
                    ),
                ));
                break;
            };
            let close = body_start + relative_close;
            if close == body_start {
                diagnostics.push(diagnostic(
                    line_number,
                    line,
                    cursor,
                    close + marker.len(),
                    Rule::EmptyInlineMarkup,
                    "Inline markup content is empty.",
                ));
            }
            cursor = close + marker.len();
            continue;
        }

        cursor += rest.chars().next().map_or(1, char::len_utf8);
    }
}

/// Validate a complete `FreeStyleWiki` document.
#[must_use]
pub fn validate(source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut state = StructureState::default();
    let mut preformatted_start = None;
    let lines = document_lines(source);

    for (line_index, line) in lines.iter().copied().enumerate() {
        let line_number = u32::try_from(line_index).unwrap_or(u32::MAX);

        if preformatted_start.is_some() {
            if line == "}}" {
                preformatted_start = None;
            }
            continue;
        }

        if line
            .strip_prefix("{{pre")
            .is_some_and(|suffix| suffix.trim().is_empty())
        {
            preformatted_start = Some(PreformattedStart {
                line: line_number,
                marker_end: "{{pre".len(),
            });
            state.list = None;
            state.table_columns = None;
            continue;
        }

        let is_comment = line.starts_with("//");
        if is_comment {
            continue;
        }

        if line.starts_with([' ', '\t']) {
            state.list = None;
            state.table_columns = None;
            continue;
        }

        let is_heading = validate_heading(line_number, line, &mut state, &mut diagnostics);
        let is_list = !is_heading && validate_list(line_number, line, &mut state, &mut diagnostics);
        let is_table = !is_heading && !is_list && line.starts_with(',');

        if is_table {
            state.list = None;
            if let Some(columns) = validate_table(line_number, line, &mut diagnostics) {
                if let Some(expected) = state.table_columns {
                    if columns != expected {
                        diagnostics.push(diagnostic(
                            line_number,
                            line,
                            0,
                            line.len(),
                            Rule::TableColumnCount,
                            format!("Table row has {columns} cells; expected {expected}."),
                        ));
                    }
                } else {
                    state.table_columns = Some(columns);
                }
            }
        } else {
            state.table_columns = None;
        }

        if !is_list {
            state.list = None;
        }
        validate_inline(line_number, line, &mut diagnostics);
    }

    if let Some(start) = preformatted_start {
        let line = lines
            .get(usize::try_from(start.line).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or("{{pre");
        diagnostics.push(diagnostic(
            start.line,
            line,
            0,
            start.marker_end,
            Rule::UnclosedPreformattedBlock,
            "Unclosed preformatted block. Expected a closing '}}' line before the end of the document.",
        ));
    }

    diagnostics.sort_by(|left, right| {
        left.range
            .start
            .line
            .cmp(&right.range.start.line)
            .then(left.range.start.character.cmp(&right.range.start.character))
            .then(left.message.cmp(&right.message))
    });
    diagnostics
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

    use super::validate;

    fn code(diagnostic: &Diagnostic) -> &str {
        let Some(NumberOrString::String(code)) = diagnostic.code.as_ref() else {
            panic!("string diagnostic code");
        };
        code
    }

    fn diagnostic<'a>(diagnostics: &'a [Diagnostic], expected_code: &str) -> &'a Diagnostic {
        diagnostics
            .iter()
            .find(|diagnostic| code(diagnostic) == expected_code)
            .unwrap_or_else(|| panic!("missing diagnostic {expected_code}"))
    }

    #[test]
    fn reports_each_syntax_error_and_orphan_close_warning() {
        let source = "{{ref image.png\n[[FrontPage\n__underline\n,\"unclosed\n{{invalid-name}}\n}}\n{{pre argument}}\n{{pre\nraw\n";
        let diagnostics = validate(source);

        for expected in [
            "fswiki/unclosed-plugin",
            "fswiki/unclosed-internal-link",
            "fswiki/unclosed-inline-markup",
            "fswiki/unclosed-table-cell-quote",
            "fswiki/invalid-plugin-name",
            "fswiki/invalid-preformatted-plugin",
            "fswiki/unclosed-preformatted-block",
        ] {
            assert_eq!(
                diagnostic(&diagnostics, expected).severity,
                Some(DiagnosticSeverity::ERROR),
                "{expected}"
            );
        }
        assert_eq!(
            diagnostic(&diagnostics, "fswiki/orphan-plugin-close").severity,
            Some(DiagnosticSeverity::WARNING)
        );
    }

    #[test]
    fn reports_each_structural_warning() {
        let source = "!! Child\n!\n!!!! Too deep\n** nested\n**** too deep\n,one,two\n,one\n{{ref}}\n{{outline unexpected}}\n";
        let diagnostics = validate(source);

        for expected in [
            "fswiki/heading-level-jump",
            "fswiki/empty-heading",
            "fswiki/heading-marker-too-long",
            "fswiki/list-level-jump",
            "fswiki/list-marker-too-long",
            "fswiki/table-column-count",
            "fswiki/missing-ref-target",
            "fswiki/unexpected-outline-arguments",
        ] {
            assert_eq!(
                diagnostic(&diagnostics, expected).severity,
                Some(DiagnosticSeverity::WARNING),
                "{expected}"
            );
        }
    }

    #[test]
    fn accepts_valid_markup_and_ignores_literal_contents() {
        let source = "!!! Root\n!! Child\n! Leaf\n* first\n** second\n,one,two\n,three,four\n{{ref image.png}}\n{{outline}}\n''italic'' '''bold''' ==strike== __under__\n[[FrontPage]]\n http://example.test/a==b\n// {{invalid-name}} __open\n{{pre\n{{invalid-name}}\n__open\n}}\n";
        assert_eq!(validate(source), Vec::new());
    }

    #[test]
    fn reports_utf16_ranges() {
        let diagnostics = validate("日本語 __open\n");
        let diagnostic = diagnostic(&diagnostics, "fswiki/unclosed-inline-markup");
        assert_eq!(diagnostic.range.start.character, 4);
        assert_eq!(diagnostic.range.end.character, 10);
    }
}
