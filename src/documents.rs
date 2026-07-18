//! Thread-safe in-memory document snapshots.

use std::{
    collections::HashMap,
    sync::{PoisonError, RwLock},
};

use tower_lsp_server::ls_types::{TextDocumentContentChangeEvent, Uri};

use crate::text::apply_changes;

#[derive(Clone, Debug, Default)]
struct DocumentState {
    current: String,
    previous: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct DocumentStore {
    documents: RwLock<HashMap<Uri, DocumentState>>,
}

impl DocumentStore {
    pub(crate) fn open(&self, uri: Uri, text: String) {
        self.documents
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                uri,
                DocumentState {
                    current: text,
                    previous: None,
                },
            );
    }

    pub(crate) fn change(
        &self,
        uri: Uri,
        changes: impl IntoIterator<Item = TextDocumentContentChangeEvent>,
    ) -> String {
        let mut documents = self
            .documents
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let document = documents.entry(uri).or_default();
        document.previous = Some(document.current.clone());
        apply_changes(&mut document.current, changes);
        document.current.clone()
    }

    pub(crate) fn close(&self, uri: &Uri) {
        self.documents
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(uri);
    }

    pub(crate) fn current(&self, uri: &Uri) -> Option<String> {
        self.documents
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(uri)
            .map(|document| document.current.clone())
    }

    pub(crate) fn previous(&self, uri: &Uri) -> Option<String> {
        self.documents
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(uri)
            .and_then(|document| document.previous.clone())
    }
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::{TextDocumentContentChangeEvent, Uri};

    use super::DocumentStore;

    #[test]
    fn keeps_the_current_and_previous_snapshots_together() {
        let store = DocumentStore::default();
        let uri = "file:///test.fsw".parse::<Uri>().expect("URI");
        store.open(uri.clone(), "before".to_string());
        assert_eq!(store.current(&uri).as_deref(), Some("before"));
        assert_eq!(store.previous(&uri), None);

        let current = store.change(
            uri.clone(),
            [TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "after".to_string(),
            }],
        );
        assert_eq!(current, "after");
        assert_eq!(store.current(&uri).as_deref(), Some("after"));
        assert_eq!(store.previous(&uri).as_deref(), Some("before"));

        store.close(&uri);
        assert_eq!(store.current(&uri), None);
        assert_eq!(store.previous(&uri), None);
    }
}
