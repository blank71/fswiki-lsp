//! Code Actions and commands for copying heading-related content.

use serde_json::Value;
use tower_lsp_server::ls_types::{CodeActionOrCommand, Command};

use crate::analysis::heading_section_at;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CopyCommand {
    Title,
    Path,
    Section,
    SectionWithAncestorsAndSiblings,
}

impl CopyCommand {
    const ALL: [Self; 4] = [
        Self::Title,
        Self::Path,
        Self::Section,
        Self::SectionWithAncestorsAndSiblings,
    ];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Title => "fswiki.copyHeadingTitle",
            Self::Path => "fswiki.copyHeadingPath",
            Self::Section => "fswiki.copyHeadingSection",
            Self::SectionWithAncestorsAndSiblings => {
                "fswiki.copyHeadingSectionWithAncestorsAndSiblings"
            }
        }
    }

    pub(crate) const fn item_name(self) -> &'static str {
        match self {
            Self::Title => "Heading title",
            Self::Path => "Heading path",
            Self::Section => "Heading section",
            Self::SectionWithAncestorsAndSiblings => "Heading section with ancestors and siblings",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    pub(crate) fn ids() -> Vec<String> {
        Self::ALL
            .into_iter()
            .map(|command| command.id().to_string())
            .collect()
    }

    fn action(self, detail: &str, text: String) -> CodeActionOrCommand {
        CodeActionOrCommand::Command(Command::new(
            format!("Copy {}: {detail}", self.item_name().to_ascii_lowercase()),
            self.id().to_string(),
            Some(vec![Value::String(text)]),
        ))
    }
}

pub(crate) fn copy_heading_actions(source: &str, line: u32) -> Vec<CodeActionOrCommand> {
    let Some(section) = heading_section_at(source, line) else {
        return Vec::new();
    };
    let heading_title = section
        .path
        .last()
        .expect("heading path is not empty")
        .clone();
    let heading_path = section
        .path
        .iter()
        .map(|heading| format!("[{heading}]"))
        .collect::<Vec<_>>()
        .join(" > ");

    vec![
        CopyCommand::Title.action(&heading_title, heading_title.clone()),
        CopyCommand::Path.action(&heading_path, heading_path.clone()),
        CopyCommand::Section.action(&heading_title, section.content),
        CopyCommand::SectionWithAncestorsAndSiblings
            .action(&heading_title, section.content_with_ancestors_and_siblings),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tower_lsp_server::ls_types::CodeActionOrCommand;

    use super::{CopyCommand, copy_heading_actions};

    fn command(action: &CodeActionOrCommand) -> &tower_lsp_server::ls_types::Command {
        let CodeActionOrCommand::Command(command) = action else {
            panic!("expected command");
        };
        command
    }

    #[test]
    fn offers_copy_actions_for_the_current_heading_section() {
        let source =
            "!!! Root\nroot text\n!! Child\n! Leaf\ntext\n!! Sibling\nsibling text\n!!! Next\n";
        let actions = copy_heading_actions(source, 4);
        assert_eq!(actions.len(), 4);

        let title = command(&actions[0]);
        assert_eq!(title.title, "Copy heading title: Leaf");
        assert_eq!(title.command, CopyCommand::Title.id());
        assert_eq!(title.arguments, Some(vec![json!("Leaf")]));

        let path = command(&actions[1]);
        assert_eq!(path.title, "Copy heading path: [Root] > [Child] > [Leaf]");
        assert_eq!(path.command, CopyCommand::Path.id());
        assert_eq!(
            path.arguments,
            Some(vec![json!("[Root] > [Child] > [Leaf]")])
        );

        let section = command(&actions[2]);
        assert_eq!(section.title, "Copy heading section: Leaf");
        assert_eq!(section.command, CopyCommand::Section.id());
        assert_eq!(section.arguments, Some(vec![json!("! Leaf\ntext\n")]));

        let ancestors = command(&actions[3]);
        assert_eq!(
            ancestors.title,
            "Copy heading section with ancestors and siblings: Leaf"
        );
        assert_eq!(
            ancestors.command,
            CopyCommand::SectionWithAncestorsAndSiblings.id()
        );
        assert_eq!(
            ancestors.arguments,
            Some(vec![json!(
                "!!! Root\nroot text\n!! Child\n! Leaf\ntext\n!! Sibling\nsibling text\n"
            )])
        );
        assert!(copy_heading_actions("text\n!!! Root\n", 0).is_empty());
    }

    #[test]
    fn maps_every_advertised_command_id() {
        for id in CopyCommand::ids() {
            assert_eq!(
                CopyCommand::from_id(&id).map(CopyCommand::id),
                Some(id.as_str())
            );
        }
        assert_eq!(CopyCommand::from_id("fswiki.unknown"), None);
    }
}
