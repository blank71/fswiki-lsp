use std::{fs, path::PathBuf};

use fswiki_lsp::formatter::{FormatOptions, format_document};

#[test]
fn tree_sitter_samples_are_canonical() {
    let sample_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tree-sitter-fswiki")
        .join("sample");
    if !sample_directory.is_dir() {
        return;
    }

    let mut paths = fs::read_dir(&sample_directory)
        .expect("read tree-sitter sample directory")
        .map(|entry| entry.expect("read sample entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "fsw"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut differences = Vec::new();
    for path in paths {
        let source = fs::read_to_string(&path).expect("read sample file");
        let formatted = format_document(&source, FormatOptions::default());
        if formatted != source {
            differences.push(format!(
                "{}\n--- expected ---\n{source:?}\n--- formatted ---\n{formatted:?}",
                path.display()
            ));
        }
    }

    assert!(
        differences.is_empty(),
        "sample formatting differs:\n\n{}",
        differences.join("\n\n")
    );
}
