//! Outline and folding analysis for `FreeStyleWiki` documents.

pub use crate::text::document_end;
use crate::{
    syntax::{closes_multiline_plugin, heading_line, opens_multiline_plugin},
    text::{document_lines, line_start_offsets, utf16_len},
};

/// A heading and its nested children.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutlineSymbol {
    pub name: String,
    pub detail: String,
    pub line: u32,
    pub range_end_line: u32,
    pub range_end_character: u32,
    pub selection_start_character: u32,
    pub selection_end_character: u32,
    pub children: Vec<Self>,
}

/// A line-based folding range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

/// Copyable content for the heading section at a document position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadingSection {
    pub path: Vec<String>,
    pub content: String,
    pub content_with_ancestors_and_siblings: String,
}

#[derive(Clone, Debug)]
struct Heading {
    name: String,
    line: usize,
    level: u8,
    range_end_line: u32,
    range_end_character: u32,
    selection_start_character: u32,
    selection_end_character: u32,
}

#[derive(Debug)]
struct SymbolNode {
    heading: Heading,
    children: Vec<usize>,
}

fn heading(line: &str, line_number: usize) -> Option<Heading> {
    let parsed = heading_line(line)?;
    let name = parsed.body.trim();
    if name.is_empty() {
        return None;
    }

    let name_offset = parsed.body.find(name)?;
    let name_start = parsed.marker_length + name_offset;
    let name_end = name_start + name.len();
    Some(Heading {
        name: name.to_string(),
        line: line_number,
        level: u8::try_from(parsed.level).ok()?,
        range_end_line: 0,
        range_end_character: 0,
        selection_start_character: utf16_len(&line[..name_start]),
        selection_end_character: utf16_len(&line[..name_end]),
    })
}

fn headings_and_plugin_folds(source: &str) -> (Vec<Heading>, Vec<Fold>, Vec<&str>) {
    let document_lines = document_lines(source);
    let mut headings = Vec::new();
    let mut plugin_folds = Vec::new();
    let mut plugin_start = None;

    for (line_number, line) in document_lines.iter().copied().enumerate() {
        if let Some(start) = plugin_start {
            if closes_multiline_plugin(line) {
                if line_number > start {
                    plugin_folds.push(Fold {
                        start_line: u32::try_from(start).unwrap_or(u32::MAX),
                        end_line: u32::try_from(line_number).unwrap_or(u32::MAX),
                    });
                }
                plugin_start = None;
            }
            continue;
        }

        if opens_multiline_plugin(line) {
            plugin_start = Some(line_number);
            continue;
        }

        if let Some(heading) = heading(line, line_number) {
            headings.push(heading);
        }
    }

    (headings, plugin_folds, document_lines)
}

fn heading_path_indices(headings: &[Heading], target_line: u32) -> Vec<usize> {
    let mut path = Vec::<usize>::new();

    for (index, heading) in headings.iter().enumerate() {
        if u32::try_from(heading.line).unwrap_or(u32::MAX) > target_line {
            break;
        }
        while path
            .last()
            .is_some_and(|ancestor| headings[*ancestor].level >= heading.level)
        {
            path.pop();
        }
        path.push(index);
    }

    path
}

fn next_section_heading(headings: &[Heading], index: usize) -> Option<&Heading> {
    let level = headings[index].level;
    headings[index + 1..]
        .iter()
        .find(|candidate| candidate.level <= level)
}

fn make_outline_symbol(index: usize, nodes: &[SymbolNode]) -> OutlineSymbol {
    let node = &nodes[index];
    OutlineSymbol {
        name: node.heading.name.clone(),
        detail: format!("L{}", node.heading.line + 1),
        line: u32::try_from(node.heading.line).unwrap_or(u32::MAX),
        range_end_line: node.heading.range_end_line,
        range_end_character: node.heading.range_end_character,
        selection_start_character: node.heading.selection_start_character,
        selection_end_character: node.heading.selection_end_character,
        children: node
            .children
            .iter()
            .map(|child| make_outline_symbol(*child, nodes))
            .collect(),
    }
}

/// Build hierarchical document symbols from headings outside multiline plugins.
#[must_use]
pub fn outline(source: &str) -> Vec<OutlineSymbol> {
    let (mut headings, _, _) = headings_and_plugin_folds(source);
    let (document_end_line, document_end_character) = document_end(source);
    for index in 0..headings.len() {
        let section_end_line = next_section_heading(&headings, index)
            .map(|candidate| u32::try_from(candidate.line).unwrap_or(u32::MAX));
        headings[index].range_end_line = section_end_line.unwrap_or(document_end_line);
        headings[index].range_end_character =
            section_end_line.map_or(document_end_character, |_| 0);
    }

    let mut nodes = Vec::<SymbolNode>::new();
    let mut roots = Vec::new();
    let mut stack = Vec::<usize>::new();

    for heading in headings {
        while stack
            .last()
            .is_some_and(|index| nodes[*index].heading.level >= heading.level)
        {
            stack.pop();
        }

        let index = nodes.len();
        nodes.push(SymbolNode {
            heading,
            children: Vec::new(),
        });
        if let Some(parent) = stack.last().copied() {
            nodes[parent].children.push(index);
        } else {
            roots.push(index);
        }
        stack.push(index);
    }

    roots
        .into_iter()
        .map(|index| make_outline_symbol(index, &nodes))
        .collect()
}

/// Return the title of the heading whose section contains the target line.
#[must_use]
pub fn heading_title_at(source: &str, target_line: u32) -> Option<String> {
    headings_and_plugin_folds(source)
        .0
        .into_iter()
        .rev()
        .find(|heading| u32::try_from(heading.line).unwrap_or(u32::MAX) <= target_line)
        .map(|heading| heading.name)
}

/// Return the hierarchical heading path whose section contains the target line.
#[must_use]
pub fn heading_path_at(source: &str, target_line: u32) -> Option<Vec<String>> {
    let headings = headings_and_plugin_folds(source).0;
    let path = heading_path_indices(&headings, target_line);
    (!path.is_empty()).then(|| {
        path.into_iter()
            .map(|index| headings[index].name.clone())
            .collect()
    })
}

/// Return the current heading section and its top-level ancestor section.
#[must_use]
pub fn heading_section_at(source: &str, target_line: u32) -> Option<HeadingSection> {
    let headings = headings_and_plugin_folds(source).0;
    let path_indices = heading_path_indices(&headings, target_line);
    let current_index = *path_indices.last()?;
    let line_starts = line_start_offsets(source);
    let section_start = line_starts[headings[current_index].line];
    let section_end = next_section_heading(&headings, current_index)
        .map_or(source.len(), |candidate| line_starts[candidate.line]);
    let content = source[section_start..section_end].to_string();

    let ancestor_index = path_indices[0];
    let ancestor_start = line_starts[headings[ancestor_index].line];
    let ancestor_end = next_section_heading(&headings, ancestor_index)
        .map_or(source.len(), |candidate| line_starts[candidate.line]);
    let content_with_ancestors_and_siblings = source[ancestor_start..ancestor_end].to_string();

    Some(HeadingSection {
        path: path_indices
            .into_iter()
            .map(|index| headings[index].name.clone())
            .collect(),
        content,
        content_with_ancestors_and_siblings,
    })
}

/// Build folding ranges for heading sections and multiline plugins.
#[must_use]
pub fn folding_ranges(source: &str) -> Vec<Fold> {
    let (headings, mut folds, document_lines) = headings_and_plugin_folds(source);

    for (index, heading) in headings.iter().enumerate() {
        let next_line = next_section_heading(&headings, index).map(|candidate| candidate.line);
        let mut end = next_line.map_or(document_lines.len() - 1, |line| line.saturating_sub(1));
        if end > heading.line && document_lines[end].trim().is_empty() {
            end -= 1;
        }
        if end > heading.line {
            folds.push(Fold {
                start_line: u32::try_from(heading.line).unwrap_or(u32::MAX),
                end_line: u32::try_from(end).unwrap_or(u32::MAX),
            });
        }
    }

    folds.sort_unstable_by_key(|fold| (fold.start_line, std::cmp::Reverse(fold.end_line)));
    folds
}

#[cfg(test)]
mod tests {
    use super::{
        Fold, HeadingSection, document_end, folding_ranges, heading_path_at, heading_section_at,
        heading_title_at, outline,
    };

    #[test]
    fn builds_hierarchical_outline_and_skips_plugin_body() {
        let source =
            "!!! Root\n!! Child\n! Grandchild\n{{pre\n!!! hidden\n}}\n!! Sibling\n!!! Next\n";
        let symbols = outline(source);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Root");
        assert_eq!(symbols[0].children.len(), 2);
        assert_eq!(symbols[0].children[0].name, "Child");
        assert_eq!(symbols[0].children[0].children[0].name, "Grandchild");
        assert_eq!(symbols[1].name, "Next");
        assert_eq!(symbols[0].range_end_line, 7);
        assert_eq!(symbols[0].children[0].range_end_line, 6);
        assert_eq!(symbols[0].selection_start_character, 4);
        assert_eq!(symbols[0].selection_end_character, 8);
        assert_eq!(symbols[1].range_end_line, 8);
    }

    #[test]
    fn trims_heading_markers_and_whitespace_from_symbol_names() {
        let symbols = outline("!!!   日本語の見出し  \n");
        assert_eq!(symbols[0].name, "日本語の見出し");
        assert_eq!(symbols[0].selection_start_character, 6);
        assert_eq!(symbols[0].selection_end_character, 13);
    }

    #[test]
    fn finds_the_heading_for_a_line_and_ignores_plugin_contents() {
        let source = "before\n!!! Root\ntext\n!! Child\n{{pre\n! hidden\n}}\nafter\n";
        assert_eq!(heading_title_at(source, 0), None);
        assert_eq!(heading_title_at(source, 2).as_deref(), Some("Root"));
        assert_eq!(heading_title_at(source, 3).as_deref(), Some("Child"));
        assert_eq!(heading_title_at(source, 5).as_deref(), Some("Child"));
        assert_eq!(heading_title_at(source, 7).as_deref(), Some("Child"));
    }

    #[test]
    fn builds_the_heading_path_for_a_line_and_replaces_siblings() {
        let source = "before\n!!! A\n!! B\n! C\ntext\n{{pre\n!!! hidden\n}}\n!! D\ntext\n";
        assert_eq!(heading_path_at(source, 0), None);
        assert_eq!(
            heading_path_at(source, 4),
            Some(vec!["A".to_string(), "B".to_string(), "C".to_string()])
        );
        assert_eq!(
            heading_path_at(source, 7),
            Some(vec!["A".to_string(), "B".to_string(), "C".to_string()]),
            "headings inside multiline plugins must remain hidden"
        );
        assert_eq!(
            heading_path_at(source, 9),
            Some(vec!["A".to_string(), "D".to_string()])
        );
    }

    #[test]
    fn extracts_a_heading_section_and_its_ancestor_section() {
        let source = "!!! A\r\nparent text\r\n!! B\r\n! C\r\nbody\r\n{{pre\r\n!! hidden\r\n}}\r\n!! D\r\nsibling body\r\n!!! Next\r\n";
        assert_eq!(
            heading_section_at(source, 4),
            Some(HeadingSection {
                path: vec!["A".to_string(), "B".to_string(), "C".to_string()],
                content: "! C\r\nbody\r\n{{pre\r\n!! hidden\r\n}}\r\n".to_string(),
                content_with_ancestors_and_siblings: "!!! A\r\nparent text\r\n!! B\r\n! C\r\nbody\r\n{{pre\r\n!! hidden\r\n}}\r\n!! D\r\nsibling body\r\n".to_string(),
            })
        );
        assert_eq!(
            heading_section_at(source, 2)
                .expect("child section")
                .content,
            "!! B\r\n! C\r\nbody\r\n{{pre\r\n!! hidden\r\n}}\r\n"
        );
        assert_eq!(heading_section_at("before\n!!! A\n", 0), None);
    }

    #[test]
    fn folds_headings_and_plugins() {
        let source = "!!! Root\ntext\n!! Child\ntext\n{{pre\nraw\n}}\n!!! Next\n";
        assert_eq!(
            folding_ranges(source),
            vec![
                Fold {
                    start_line: 0,
                    end_line: 6,
                },
                Fold {
                    start_line: 2,
                    end_line: 6,
                },
                Fold {
                    start_line: 4,
                    end_line: 6,
                },
            ]
        );
    }

    #[test]
    fn computes_utf16_document_end() {
        assert_eq!(document_end("first\n😀"), (1, 2));
        assert_eq!(document_end("first\n"), (1, 0));
    }
}
