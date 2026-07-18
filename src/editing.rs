//! On-type edits for list continuation and hierarchy changes.

use tower_lsp_server::ls_types::{FormattingOptions, Position, Range, TextEdit};

use crate::{
    syntax::{closes_multiline_plugin, list_marker, opens_multiline_plugin},
    text::position_to_offset,
};

pub(crate) const LIST_INDENT_TRIGGER: &str = "\t";
pub(crate) const LIST_OUTDENT_TRIGGER: &str = "\u{000b}";

#[derive(Clone, Copy, Debug)]
pub(crate) enum ListHierarchyChange {
    Deeper,
    Shallower,
}

fn list_marker_text(line: &str) -> Option<&str> {
    let marker = list_marker(line)?;
    Some(&line[..marker.level])
}

fn inside_multiline_plugin(source: &str, line_start: usize) -> bool {
    let mut inside_plugin = false;
    for line in source[..line_start].split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if inside_plugin {
            if closes_multiline_plugin(line) {
                inside_plugin = false;
            }
        } else if opens_multiline_plugin(line) {
            inside_plugin = true;
        }
    }
    inside_plugin
}

pub(crate) fn list_continuation_edit(source: &str, position: Position) -> Option<TextEdit> {
    if position.line == 0 {
        return None;
    }
    let offset = position_to_offset(source, position)?;
    let current_line_start = source[..offset]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    if inside_multiline_plugin(source, current_line_start) {
        return None;
    }
    let current_prefix = &source[current_line_start..offset];
    if !current_prefix
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t'))
    {
        return None;
    }

    let previous_line_end = current_line_start.checked_sub(1)?;
    let previous_line_start = source[..previous_line_end]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let previous_line = source[previous_line_start..previous_line_end]
        .strip_suffix('\r')
        .unwrap_or(&source[previous_line_start..previous_line_end]);
    let marker = list_marker_text(previous_line)?;
    Some(TextEdit::new(
        Range::new(Position::new(position.line, 0), position),
        format!("{marker} "),
    ))
}

fn split_line_ending(line: &str) -> (&str, &str) {
    if let Some(body) = line.strip_suffix("\r\n") {
        (body, "\r\n")
    } else if let Some(body) = line.strip_suffix('\n') {
        (body, "\n")
    } else {
        (line, "")
    }
}

fn change_list_markers(source: &str, change: ListHierarchyChange) -> String {
    let mut changed = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let (body, line_ending) = split_line_ending(line);
        if let Some(marker) = list_marker_text(body) {
            match change {
                ListHierarchyChange::Deeper if marker.len() < 3 => {
                    changed.push_str(&marker[..1]);
                    changed.push_str(body);
                }
                ListHierarchyChange::Shallower if marker.len() > 1 => {
                    changed.push_str(&body[1..]);
                }
                ListHierarchyChange::Deeper | ListHierarchyChange::Shallower => {
                    changed.push_str(body);
                }
            }
        } else {
            changed.push_str(body);
        }
        changed.push_str(line_ending);
    }
    changed
}

fn replaced_selection<'a>(
    previous_source: &'a str,
    source: &str,
    trigger_start_offset: usize,
    trigger_end_offset: usize,
) -> Option<&'a str> {
    if !previous_source.starts_with(source.get(..trigger_start_offset)?) {
        return None;
    }
    let suffix = source.get(trigger_end_offset..)?;
    if !previous_source.ends_with(suffix) {
        return None;
    }
    let removed_end = previous_source.len().checked_sub(suffix.len())?;
    (removed_end >= trigger_start_offset)
        .then(|| &previous_source[trigger_start_offset..removed_end])
}

fn regular_tab_edits(
    line: &str,
    position: Position,
    trigger_start: Position,
    trigger_length: u32,
    trigger_range: Range,
    change: ListHierarchyChange,
    options: &FormattingOptions,
) -> Option<Vec<TextEdit>> {
    if matches!(change, ListHierarchyChange::Deeper) {
        let replacement = if options.insert_spaces {
            let tab_size = options.tab_size.max(1);
            " ".repeat(usize::try_from(tab_size - trigger_start.character % tab_size).ok()?)
        } else {
            "\t".to_string()
        };
        return Some(vec![TextEdit::new(trigger_range, replacement)]);
    }

    let leading_spaces = line.bytes().take_while(|byte| *byte == b' ').count();
    let remove_length = if line.starts_with('\t') {
        1
    } else if leading_spaces > 0 {
        let tab_size = usize::try_from(options.tab_size.max(1)).ok()?;
        let remainder = leading_spaces % tab_size;
        if remainder == 0 { tab_size } else { remainder }
    } else {
        0
    };
    let remove_length = u32::try_from(remove_length).ok()?;
    if trigger_start.character < remove_length {
        Some(vec![TextEdit::new(
            Range::new(
                Position::new(position.line, 0),
                Position::new(position.line, remove_length + trigger_length),
            ),
            String::new(),
        )])
    } else {
        let mut edits = vec![TextEdit::new(trigger_range, String::new())];
        if remove_length > 0 {
            edits.push(TextEdit::new(
                Range::new(
                    Position::new(position.line, 0),
                    Position::new(position.line, remove_length),
                ),
                String::new(),
            ));
        }
        Some(edits)
    }
}

pub(crate) fn list_hierarchy_edits(
    source: &str,
    previous_source: Option<&str>,
    position: Position,
    trigger: &str,
    change: ListHierarchyChange,
    options: &FormattingOptions,
) -> Option<Vec<TextEdit>> {
    let trigger_length = u32::try_from(trigger.encode_utf16().count()).ok()?;
    let trigger_start = Position::new(
        position.line,
        position.character.checked_sub(trigger_length)?,
    );
    let trigger_start_offset = position_to_offset(source, trigger_start)?;
    let trigger_end_offset = position_to_offset(source, position)?;
    if source.get(trigger_start_offset..trigger_end_offset)? != trigger {
        return None;
    }

    let line_start = source[..trigger_start_offset]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let line_end = source[trigger_end_offset..]
        .find('\n')
        .map_or(source.len(), |relative| trigger_end_offset + relative);
    let visible_line_end = if source[..line_end].ends_with('\r') {
        line_end - 1
    } else {
        line_end
    };
    let mut line_without_trigger = String::with_capacity(visible_line_end - line_start);
    line_without_trigger.push_str(&source[line_start..trigger_start_offset]);
    line_without_trigger.push_str(&source[trigger_end_offset..visible_line_end]);
    let trigger_range = Range::new(trigger_start, position);

    if let Some(selection) = previous_source.and_then(|previous_source| {
        replaced_selection(
            previous_source,
            source,
            trigger_start_offset,
            trigger_end_offset,
        )
    }) && !selection.is_empty()
    {
        return Some(vec![TextEdit::new(
            trigger_range,
            change_list_markers(selection, change),
        )]);
    }

    if !inside_multiline_plugin(source, line_start)
        && let Some(marker) = list_marker_text(&line_without_trigger)
    {
        let marker_length = u32::try_from(marker.len()).ok()?;
        let marker_start_character = u32::from(trigger_start.character == 0);
        let marker_start = Position::new(position.line, marker_start_character);
        let mut edits = vec![TextEdit::new(trigger_range, String::new())];
        match change {
            ListHierarchyChange::Deeper if marker_length < 3 => edits.push(TextEdit::new(
                Range::new(marker_start, marker_start),
                marker[..1].to_string(),
            )),
            ListHierarchyChange::Shallower if marker_length > 1 => edits.push(TextEdit::new(
                Range::new(
                    marker_start,
                    Position::new(position.line, marker_start_character + 1),
                ),
                String::new(),
            )),
            ListHierarchyChange::Deeper | ListHierarchyChange::Shallower => {}
        }
        return Some(edits);
    }

    regular_tab_edits(
        &line_without_trigger,
        position,
        trigger_start,
        trigger_length,
        trigger_range,
        change,
        options,
    )
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::{FormattingOptions, Position, Range, TextEdit};

    use super::{
        LIST_INDENT_TRIGGER, LIST_OUTDENT_TRIGGER, ListHierarchyChange, list_continuation_edit,
        list_hierarchy_edits,
    };
    use crate::text::position_to_offset;

    fn formatting_options() -> FormattingOptions {
        FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            ..FormattingOptions::default()
        }
    }

    fn apply_text_edits(source: &str, edits: Vec<TextEdit>) -> String {
        let mut replacements = edits
            .into_iter()
            .map(|edit| {
                (
                    position_to_offset(source, edit.range.start).expect("valid edit start"),
                    position_to_offset(source, edit.range.end).expect("valid edit end"),
                    edit.new_text,
                )
            })
            .collect::<Vec<_>>();
        replacements.sort_unstable_by_key(|(start, _, _)| std::cmp::Reverse(*start));

        let mut result = source.to_string();
        for (start, end, replacement) in replacements {
            result.replace_range(start..end, &replacement);
        }
        result
    }

    #[test]
    fn continues_the_same_list_level_after_a_newline() {
        let edit = list_continuation_edit("** item\n", Position::new(1, 0))
            .expect("unordered list continuation");
        assert_eq!(
            edit.range,
            Range::new(Position::new(1, 0), Position::new(1, 0))
        );
        assert_eq!(edit.new_text, "** ");

        let edit = list_continuation_edit("+++ 項目\r\n  next", Position::new(1, 2))
            .expect("ordered list continuation");
        assert_eq!(edit.new_text, "+++ ");
        assert_eq!(edit.range.start, Position::new(1, 0));
        assert_eq!(edit.range.end, Position::new(1, 2));
    }

    #[test]
    fn does_not_continue_non_list_lines() {
        assert!(list_continuation_edit("paragraph\n", Position::new(1, 0)).is_none());
        assert!(list_continuation_edit("* item", Position::new(0, 6)).is_none());
        assert!(
            list_continuation_edit("{{pre\n* raw\n", Position::new(2, 0)).is_none(),
            "plugin contents must remain literal"
        );
    }

    #[test]
    fn changes_list_hierarchy_with_tab_and_shift_tab() {
        let source = "** item\t";
        let edits = list_hierarchy_edits(
            source,
            None,
            Position::new(0, 8),
            LIST_INDENT_TRIGGER,
            ListHierarchyChange::Deeper,
            &formatting_options(),
        )
        .expect("indent edits");
        assert_eq!(apply_text_edits(source, edits), "*** item");

        let source = "*** item\u{000b}";
        let edits = list_hierarchy_edits(
            source,
            None,
            Position::new(0, 9),
            LIST_OUTDENT_TRIGGER,
            ListHierarchyChange::Shallower,
            &formatting_options(),
        )
        .expect("outdent edits");
        assert_eq!(apply_text_edits(source, edits), "** item");
    }

    #[test]
    fn changes_hierarchy_when_tab_is_typed_before_the_marker() {
        let source = "\t++ item";
        let edits = list_hierarchy_edits(
            source,
            None,
            Position::new(0, 1),
            LIST_INDENT_TRIGGER,
            ListHierarchyChange::Deeper,
            &formatting_options(),
        )
        .expect("indent edits");
        assert_eq!(apply_text_edits(source, edits), "+++ item");
    }

    #[test]
    fn changes_selected_list_lines_without_losing_the_selection_text() {
        let previous_source = "* first\r\n** second\r\nparagraph";
        let source = "\t";
        let edits = list_hierarchy_edits(
            source,
            Some(previous_source),
            Position::new(0, 1),
            LIST_INDENT_TRIGGER,
            ListHierarchyChange::Deeper,
            &formatting_options(),
        )
        .expect("selected list edits");
        assert_eq!(
            apply_text_edits(source, edits),
            "** first\r\n*** second\r\nparagraph"
        );
    }

    #[test]
    fn preserves_hierarchy_boundaries_and_regular_tab_behavior() {
        let source = "*** item\t";
        let edits = list_hierarchy_edits(
            source,
            None,
            Position::new(0, 9),
            LIST_INDENT_TRIGGER,
            ListHierarchyChange::Deeper,
            &formatting_options(),
        )
        .expect("maximum hierarchy edits");
        assert_eq!(apply_text_edits(source, edits), "*** item");

        let source = "text\t";
        let edits = list_hierarchy_edits(
            source,
            None,
            Position::new(0, 5),
            LIST_INDENT_TRIGGER,
            ListHierarchyChange::Deeper,
            &formatting_options(),
        )
        .expect("regular tab edits");
        assert_eq!(apply_text_edits(source, edits), "text    ");

        let source = "    text\u{000b}";
        let edits = list_hierarchy_edits(
            source,
            None,
            Position::new(0, 9),
            LIST_OUTDENT_TRIGGER,
            ListHierarchyChange::Shallower,
            &formatting_options(),
        )
        .expect("regular outdent edits");
        assert_eq!(apply_text_edits(source, edits), "text");
    }
}
