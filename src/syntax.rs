//! Shared recognition of line-oriented block markers.

pub(crate) const MAX_HEADING_LEVEL: usize = 3;
pub(crate) const MAX_LIST_LEVEL: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeadingLine<'a> {
    pub(crate) level: usize,
    pub(crate) marker_length: usize,
    pub(crate) marker_run: usize,
    pub(crate) body: &'a str,
}

pub(crate) fn heading_line(line: &str) -> Option<HeadingLine<'_>> {
    let marker_run = line.bytes().take_while(|byte| *byte == b'!').count();
    if marker_run == 0 {
        return None;
    }
    let marker_length = marker_run.min(MAX_HEADING_LEVEL);
    Some(HeadingLine {
        level: MAX_HEADING_LEVEL + 1 - marker_length,
        marker_length,
        marker_run,
        body: &line[marker_length..],
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ListMarker {
    pub(crate) marker: u8,
    pub(crate) level: usize,
    pub(crate) marker_run: usize,
}

pub(crate) fn list_marker(line: &str) -> Option<ListMarker> {
    let marker = *line.as_bytes().first()?;
    if !matches!(marker, b'*' | b'+') {
        return None;
    }
    let marker_run = line.bytes().take_while(|byte| *byte == marker).count();
    Some(ListMarker {
        marker,
        level: marker_run.min(MAX_LIST_LEVEL),
        marker_run,
    })
}

pub(crate) fn opens_multiline_plugin(line: &str) -> bool {
    line.strip_prefix("{{")
        .is_some_and(|body| !body.is_empty() && !body.contains('}'))
}

pub(crate) fn closes_multiline_plugin(line: &str) -> bool {
    line == "}}"
}

#[cfg(test)]
mod tests {
    use super::{HeadingLine, ListMarker, heading_line, list_marker};

    #[test]
    fn recognizes_heading_levels_and_preserves_overlong_content() {
        assert_eq!(
            heading_line("!!! Root"),
            Some(HeadingLine {
                level: 1,
                marker_length: 3,
                marker_run: 3,
                body: " Root",
            })
        );
        assert_eq!(heading_line("text"), None);
        assert_eq!(
            heading_line("!!!! title").map(|heading| heading.body),
            Some("! title")
        );
    }

    #[test]
    fn recognizes_list_kind_depth_and_overlong_runs() {
        assert_eq!(
            list_marker("++++ item"),
            Some(ListMarker {
                marker: b'+',
                level: 3,
                marker_run: 4,
            })
        );
        assert_eq!(list_marker("text"), None);
    }
}
