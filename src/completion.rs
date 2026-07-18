//! Context-aware completion items for `FreeStyleWiki` markup.

use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, InsertTextFormat, Position, Range,
    TextEdit,
};

use crate::syntax::{MAX_HEADING_LEVEL, MAX_LIST_LEVEL};

fn replacement_range(position: Position, prefix_length: usize) -> Option<Range> {
    let prefix_length = u32::try_from(prefix_length).ok()?;
    let start_character = position.character.checked_sub(prefix_length)?;
    Some(Range::new(
        Position::new(position.line, start_character),
        position,
    ))
}

fn snippet(
    label: &str,
    detail: &str,
    filter_text: &str,
    sort_text: &str,
    new_text: &str,
    range: Range,
) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(detail.to_string()),
        filter_text: Some(filter_text.to_string()),
        sort_text: Some(sort_text.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
            range,
            new_text.to_string(),
        ))),
        ..CompletionItem::default()
    }
}

fn heading_items(range: Range) -> Vec<CompletionItem> {
    vec![
        snippet(
            "!!!",
            "Heading 1",
            "!!! heading 1",
            "01",
            "!!! ${1:Heading}$0",
            range,
        ),
        snippet(
            "!!",
            "Heading 2",
            "!! heading 2",
            "02",
            "!! ${1:Heading}$0",
            range,
        ),
        snippet(
            "!",
            "Heading 3",
            "! heading 3",
            "03",
            "! ${1:Heading}$0",
            range,
        ),
    ]
}

fn list_items(marker: char, range: Range) -> Vec<CompletionItem> {
    let (kind, sort_start) = if marker == '*' {
        ("Unordered List", 4)
    } else {
        ("Ordered List", 7)
    };
    (1..=MAX_LIST_LEVEL)
        .map(|level| {
            let list_marker = marker.to_string().repeat(level);
            snippet(
                &list_marker,
                &format!("{kind} {level}"),
                &format!("{list_marker} {kind} {level}"),
                &format!("{:02}", sort_start + level - 1),
                &format!("{list_marker} ${{1:item}}$0"),
                range,
            )
        })
        .collect()
}

fn plugin_items(range: Range) -> Vec<CompletionItem> {
    vec![
        snippet(
            "pre",
            "Plugin",
            "{{pre}} pre",
            "11",
            "{{pre\n${1:content}\n}}$0",
            range,
        ),
        snippet(
            "outline",
            "Plugin",
            "{{outline}} outline",
            "12",
            "{{outline}}$0",
            range,
        ),
        snippet(
            "ref",
            "Plugin",
            "{{ref some.png}} ref",
            "13",
            "{{ref ${1:some.png}}}$0",
            range,
        ),
    ]
}

fn inline_items(range: Range) -> Vec<CompletionItem> {
    vec![
        snippet(
            "italic",
            "Inline markup",
            "''italic'' italic",
            "21",
            "''${1:italic}''$0",
            range,
        ),
        snippet(
            "bold",
            "Inline markup",
            "'''bold''' bold",
            "22",
            "'''${1:bold}'''$0",
            range,
        ),
        snippet(
            "line",
            "Inline markup",
            "==line== line-through",
            "23",
            "==${1:line}==$0",
            range,
        ),
        snippet(
            "under",
            "Inline markup",
            "__under__ underline",
            "24",
            "__${1:under}__$0",
            range,
        ),
    ]
}

fn heading_prefix_length(line_prefix: &str) -> Option<usize> {
    (!line_prefix.is_empty()
        && line_prefix.len() <= MAX_HEADING_LEVEL
        && line_prefix.bytes().all(|byte| byte == b'!'))
    .then_some(line_prefix.len())
}

fn list_prefix(line_prefix: &str) -> Option<(char, usize)> {
    let marker = line_prefix.chars().next()?;
    (line_prefix.len() <= MAX_LIST_LEVEL
        && matches!(marker, '*' | '+')
        && line_prefix.chars().all(|character| character == marker))
    .then_some((marker, line_prefix.len()))
}

fn plugin_prefix_length(line_prefix: &str) -> Option<usize> {
    let bytes = line_prefix.as_bytes();
    let mut start = bytes.len();
    while start > 0 && bytes[start - 1].is_ascii_alphabetic() {
        start -= 1;
    }

    let mut brace_start = start;
    let mut brace_count = 0;
    while brace_start > 0 && bytes[brace_start - 1] == b'{' && brace_count < 2 {
        brace_start -= 1;
        brace_count += 1;
    }
    if brace_count == 0 || (brace_start > 0 && bytes[brace_start - 1] == b'{') {
        None
    } else {
        Some(bytes.len() - brace_start)
    }
}

fn repeated_suffix_length(line_prefix: &str, marker: u8, maximum: usize) -> Option<usize> {
    let length = line_prefix
        .bytes()
        .rev()
        .take_while(|byte| *byte == marker)
        .count();
    (length > 0 && length <= maximum).then_some(length)
}

/// Build completions for the text on the current line before the cursor.
pub(crate) fn completion_items(line_prefix: &str, position: Position) -> Vec<CompletionItem> {
    if let Some(prefix_length) = heading_prefix_length(line_prefix) {
        return replacement_range(position, prefix_length).map_or_else(Vec::new, heading_items);
    }
    if let Some((marker, prefix_length)) = list_prefix(line_prefix) {
        let Some(range) = replacement_range(position, prefix_length) else {
            return Vec::new();
        };
        return list_items(marker, range);
    }
    if let Some(prefix_length) = plugin_prefix_length(line_prefix) {
        return replacement_range(position, prefix_length).map_or_else(Vec::new, plugin_items);
    }
    if let Some(prefix_length) = repeated_suffix_length(line_prefix, b'\'', 3) {
        let Some(range) = replacement_range(position, prefix_length) else {
            return Vec::new();
        };
        return inline_items(range)
            .into_iter()
            .filter(|item| {
                matches!(item.label.as_str(), "italic" | "bold")
                    && (prefix_length < 3 || item.label == "bold")
            })
            .collect();
    }
    if let Some(prefix_length) = repeated_suffix_length(line_prefix, b'=', 2) {
        let Some(range) = replacement_range(position, prefix_length) else {
            return Vec::new();
        };
        return inline_items(range)
            .into_iter()
            .filter(|item| item.label == "line")
            .collect();
    }
    if let Some(prefix_length) = repeated_suffix_length(line_prefix, b'_', 2) {
        let Some(range) = replacement_range(position, prefix_length) else {
            return Vec::new();
        };
        return inline_items(range)
            .into_iter()
            .filter(|item| item.label == "under")
            .collect();
    }

    let Some(range) = replacement_range(position, 0) else {
        return Vec::new();
    };
    if line_prefix.is_empty() {
        let mut items = heading_items(range);
        items.extend(list_items('*', range));
        items.extend(list_items('+', range));
        items.extend(plugin_items(range));
        items.extend(inline_items(range));
        items
    } else {
        inline_items(range)
    }
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::{CompletionItem, CompletionTextEdit, Position};

    use super::completion_items;

    fn labels(line_prefix: &str, position: Position) -> Vec<String> {
        completion_items(line_prefix, position)
            .into_iter()
            .map(|item| item.label)
            .collect()
    }

    fn new_text(item: &CompletionItem) -> &str {
        let Some(CompletionTextEdit::Edit(edit)) = item.text_edit.as_ref() else {
            panic!("completion must use a text edit");
        };
        &edit.new_text
    }

    #[test]
    fn completes_heading_markers_at_the_start_of_a_line() {
        let items = completion_items("!", Position::new(2, 1));

        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["!!!", "!!", "!"]
        );
        assert_eq!(new_text(&items[0]), "!!! ${1:Heading}$0");
        let Some(CompletionTextEdit::Edit(edit)) = items[0].text_edit.as_ref() else {
            panic!("heading completion edit");
        };
        assert_eq!(edit.range.start, Position::new(2, 0));
        assert_eq!(edit.range.end, Position::new(2, 1));
    }

    #[test]
    fn completes_plugins_after_one_or_two_opening_braces() {
        let items = completion_items("本文{{re", Position::new(0, 6));

        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["pre", "outline", "ref"]
        );
        assert_eq!(new_text(&items[0]), "{{pre\n${1:content}\n}}$0");
        assert_eq!(new_text(&items[1]), "{{outline}}$0");
        assert_eq!(new_text(&items[2]), "{{ref ${1:some.png}}}$0");
        let Some(CompletionTextEdit::Edit(edit)) = items[2].text_edit.as_ref() else {
            panic!("plugin completion edit");
        };
        assert_eq!(edit.range.start, Position::new(0, 2));
        assert_eq!(edit.range.end, Position::new(0, 6));
    }

    #[test]
    fn completes_unordered_and_ordered_list_levels() {
        assert_eq!(labels("*", Position::new(0, 1)), ["*", "**", "***"]);
        assert_eq!(labels("+", Position::new(0, 1)), ["+", "++", "+++"]);

        let items = completion_items("**", Position::new(1, 2));
        assert_eq!(new_text(&items[1]), "** ${1:item}$0");
        let Some(CompletionTextEdit::Edit(edit)) = items[1].text_edit.as_ref() else {
            panic!("list completion edit");
        };
        assert_eq!(edit.range.start, Position::new(1, 0));
        assert_eq!(edit.range.end, Position::new(1, 2));
    }

    #[test]
    fn completes_each_inline_markup() {
        assert_eq!(labels("text'", Position::new(0, 5)), ["italic", "bold"]);
        assert_eq!(labels("text'''", Position::new(0, 7)), ["bold"]);
        assert_eq!(labels("text=", Position::new(0, 5)), ["line"]);
        assert_eq!(labels("text_", Position::new(0, 5)), ["under"]);
    }

    #[test]
    fn offers_all_markup_at_an_empty_line() {
        assert_eq!(completion_items("", Position::new(0, 0)).len(), 16);
    }
}
