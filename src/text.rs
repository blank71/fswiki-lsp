//! Shared document text and LSP position utilities.

use tower_lsp_server::ls_types::{Position, TextDocumentContentChangeEvent};

pub(crate) fn without_carriage_return(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

pub(crate) fn document_lines(source: &str) -> Vec<&str> {
    source.split('\n').map(without_carriage_return).collect()
}

pub(crate) fn utf16_len(source: &str) -> u32 {
    u32::try_from(source.encode_utf16().count()).unwrap_or(u32::MAX)
}

pub(crate) fn line_start_offsets(source: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(source.match_indices('\n').map(|(newline, _)| newline + 1))
        .collect()
}

/// Return the UTF-16 LSP position at the end of a document.
#[must_use]
pub fn document_end(source: &str) -> (u32, u32) {
    let lines = document_lines(source);
    let last_line = lines.len() - 1;
    (
        u32::try_from(last_line).unwrap_or(u32::MAX),
        utf16_len(lines[last_line]),
    )
}

pub(crate) fn position_to_offset(source: &str, position: Position) -> Option<usize> {
    let target_line = usize::try_from(position.line).ok()?;
    let mut line_start = 0;

    for _ in 0..target_line {
        line_start += source[line_start..].find('\n')? + 1;
    }

    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |relative| line_start + relative);
    let visible_end = if source[..line_end].ends_with('\r') {
        line_end - 1
    } else {
        line_end
    };
    let line = &source[line_start..visible_end];
    let target_character = usize::try_from(position.character).ok()?;
    let mut utf16_offset = 0;

    for (byte_offset, character) in line.char_indices() {
        if utf16_offset == target_character {
            return Some(line_start + byte_offset);
        }
        utf16_offset += character.len_utf16();
        if utf16_offset > target_character {
            return None;
        }
    }
    (utf16_offset == target_character).then_some(visible_end)
}

pub(crate) fn apply_changes(
    source: &mut String,
    changes: impl IntoIterator<Item = TextDocumentContentChangeEvent>,
) {
    for change in changes {
        if let Some(range) = change.range {
            let Some(start) = position_to_offset(source, range.start) else {
                continue;
            };
            let Some(end) = position_to_offset(source, range.end) else {
                continue;
            };
            if start <= end {
                source.replace_range(start..end, &change.text);
            }
        } else {
            *source = change.text;
        }
    }
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::{Position, Range, TextDocumentContentChangeEvent};

    use super::{apply_changes, document_end, position_to_offset};

    #[test]
    fn converts_utf16_positions_and_applies_incremental_changes() {
        let mut source = "a😀b\n次".to_string();
        assert_eq!(position_to_offset(&source, Position::new(0, 1)), Some(1));
        assert_eq!(position_to_offset(&source, Position::new(0, 3)), Some(5));
        apply_changes(
            &mut source,
            [TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 1), Position::new(0, 3))),
                range_length: None,
                text: "x".to_string(),
            }],
        );
        assert_eq!(source, "axb\n次");
    }

    #[test]
    fn computes_document_end_for_lf_and_crlf() {
        assert_eq!(document_end("first\n😀"), (1, 2));
        assert_eq!(document_end("first\r\n"), (1, 0));
    }
}
