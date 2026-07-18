//! Language server settings and compatibility aliases.

use serde_json::Value;

use crate::formatter::{FormatOptions, TableAlign};

/// Mutable server configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ServerSettings {
    pub formatting: FormatOptions,
}

impl ServerSettings {
    /// Read settings from Zed initialization options or workspace configuration.
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        let mut settings = Self::default();
        let root = value
            .get("fswiki-lsp")
            .or_else(|| value.get("fswiki"))
            .unwrap_or(value);
        let formatting = root.get("formatting").unwrap_or(root);

        let alignment = formatting
            .get("tableAlign")
            .or_else(|| formatting.get("formatTableAlignOption"))
            .and_then(Value::as_str);
        if let Some(alignment) = alignment {
            settings.formatting.table_align = if alignment.eq_ignore_ascii_case("left") {
                TableAlign::Left
            } else {
                TableAlign::Right
            };
        }

        if let Some(suffix_space) = formatting
            .get("tableCellSuffixSpace")
            .or_else(|| formatting.get("formatTableCellSuffixSpace"))
            .and_then(Value::as_bool)
        {
            settings.formatting.table_cell_suffix_space = Some(suffix_space);
        }
        settings
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ServerSettings;
    use crate::formatter::TableAlign;

    #[test]
    fn reads_zed_and_legacy_format_settings() {
        assert_eq!(
            ServerSettings::default().formatting.table_align,
            TableAlign::Left
        );

        let settings = ServerSettings::from_json(&json!({
            "fswiki-lsp": {
                "formatting": {
                    "tableAlign": "left",
                    "tableCellSuffixSpace": true
                }
            }
        }));
        assert_eq!(settings.formatting.table_align, TableAlign::Left);
        assert_eq!(settings.formatting.table_cell_suffix_space, Some(true));

        let settings = ServerSettings::from_json(&json!({
            "formatTableAlignOption": "right",
            "formatTableCellSuffixSpace": false
        }));
        assert_eq!(settings.formatting.table_align, TableAlign::Right);
        assert_eq!(settings.formatting.table_cell_suffix_space, Some(false));
    }
}
