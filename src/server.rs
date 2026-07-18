//! Language Server Protocol backend.

use std::{
    fs,
    sync::{Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use arboard::Clipboard;
use serde_json::Value;
use tower_lsp_server::{
    Client, LanguageServer,
    jsonrpc::Result,
    ls_types::{
        CodeActionParams, CodeActionProviderCapability, CodeActionResponse, CompletionOptions,
        CompletionParams, CompletionResponse, DidChangeConfigurationParams,
        DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
        DocumentFormattingParams, DocumentOnTypeFormattingOptions, DocumentOnTypeFormattingParams,
        DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, ExecuteCommandOptions,
        ExecuteCommandParams, FoldingRange, FoldingRangeParams, FoldingRangeProviderCapability,
        InitializeParams, InitializeResult, InitializedParams, MessageType, OneOf, Position,
        PositionEncodingKind, Range, ServerCapabilities, ServerInfo, SymbolKind,
        TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
    },
};

use crate::{
    actions::{CopyCommand, copy_heading_actions},
    analysis::{OutlineSymbol, document_end, folding_ranges, outline},
    completion::completion_items,
    diagnostics::validate,
    documents::DocumentStore,
    editing::{
        LIST_INDENT_TRIGGER, LIST_OUTDENT_TRIGGER, ListHierarchyChange, list_continuation_edit,
        list_hierarchy_edits,
    },
    formatter::format_document,
    text::position_to_offset,
};

pub use crate::settings::ServerSettings;

/// In-memory `FreeStyleWiki` language server state.
pub struct Backend {
    client: Client,
    clipboard: Mutex<Option<Clipboard>>,
    documents: DocumentStore,
    settings: RwLock<ServerSettings>,
}

impl Backend {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            clipboard: Mutex::new(None),
            documents: DocumentStore::default(),
            settings: RwLock::new(ServerSettings::default()),
        }
    }

    fn read_settings(&self) -> RwLockReadGuard<'_, ServerSettings> {
        self.settings.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_settings(&self) -> RwLockWriteGuard<'_, ServerSettings> {
        self.settings
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn document(&self, uri: &Uri) -> Option<String> {
        self.documents.current(uri).or_else(|| {
            uri.to_file_path()
                .and_then(|path| fs::read_to_string(path.as_ref()).ok())
        })
    }

    fn previous_document(&self, uri: &Uri) -> Option<String> {
        self.documents.previous(uri)
    }

    fn set_settings(&self, value: &Value) {
        *self.write_settings() = ServerSettings::from_json(value);
    }

    fn copy_to_clipboard(&self, text: &str) -> std::result::Result<(), arboard::Error> {
        let mut clipboard = self
            .clipboard
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if clipboard.is_none() {
            *clipboard = Some(Clipboard::new()?);
        }
        clipboard
            .as_mut()
            .expect("clipboard was initialized")
            .set_text(text.to_string())
    }
}

fn outline_to_document_symbol(symbol: OutlineSymbol) -> DocumentSymbol {
    let range = Range::new(
        Position::new(symbol.line, 0),
        Position::new(symbol.range_end_line, symbol.range_end_character),
    );
    let selection_range = Range::new(
        Position::new(symbol.line, symbol.selection_start_character),
        Position::new(symbol.line, symbol.selection_end_character),
    );
    let children = (!symbol.children.is_empty()).then(|| {
        symbol
            .children
            .into_iter()
            .map(outline_to_document_symbol)
            .collect()
    });

    #[allow(deprecated)]
    DocumentSymbol {
        name: symbol.name,
        detail: Some(symbol.detail),
        kind: SymbolKind::STRING,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(options) = params.initialization_options.as_ref() {
            self.set_settings(options);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: CopyCommand::ids(),
                    ..ExecuteCommandOptions::default()
                }),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(
                        ["!", "{", "'", "=", "_", "*", "+"]
                            .map(str::to_string)
                            .into_iter()
                            .collect(),
                    ),
                    ..CompletionOptions::default()
                }),
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: "\n".to_string(),
                    more_trigger_character: Some(vec![
                        LIST_INDENT_TRIGGER.to_string(),
                        LIST_OUTDENT_TRIGGER.to_string(),
                    ]),
                }),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "fswiki-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            offset_encoding: None,
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "fswiki-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        let diagnostics = validate(&document.text);
        self.documents.open(document.uri.clone(), document.text);
        self.client
            .publish_diagnostics(document.uri, diagnostics, Some(document.version))
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let source = self.documents.change(uri.clone(), params.content_changes);
        self.client
            .publish_diagnostics(uri, validate(&source), Some(version))
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.close(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        self.set_settings(&params.settings);
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let Some(source) = self.document(&params.text_document.uri) else {
            return Ok(None);
        };
        let formatted = format_document(&source, self.read_settings().formatting);
        let (line, character) = document_end(&source);
        Ok(Some(vec![TextEdit::new(
            Range::new(Position::new(0, 0), Position::new(line, character)),
            formatted,
        )]))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some(source) = self.document(uri) else {
            return Ok(None);
        };
        let Some(offset) = position_to_offset(&source, position) else {
            return Ok(Some(Vec::new().into()));
        };
        let line_start = source[..offset]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        let items = completion_items(&source[line_start..offset], position);
        Ok(Some(items.into()))
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some(source) = self.document(uri) else {
            return Ok(None);
        };
        let previous_source = self.previous_document(uri);
        match params.ch.as_str() {
            "\n" => Ok(list_continuation_edit(&source, position).map(|edit| vec![edit])),
            LIST_INDENT_TRIGGER => Ok(list_hierarchy_edits(
                &source,
                previous_source.as_deref(),
                position,
                LIST_INDENT_TRIGGER,
                ListHierarchyChange::Deeper,
                &params.options,
            )),
            LIST_OUTDENT_TRIGGER => Ok(list_hierarchy_edits(
                &source,
                previous_source.as_deref(),
                position,
                LIST_OUTDENT_TRIGGER,
                ListHierarchyChange::Shallower,
                &params.options,
            )),
            _ => Ok(None),
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let Some(source) = self.document(&params.text_document.uri) else {
            return Ok(None);
        };
        let symbols = outline(&source)
            .into_iter()
            .map(outline_to_document_symbol)
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let Some(source) = self.document(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(Some(copy_heading_actions(&source, params.range.start.line)))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        let Some(command) = CopyCommand::from_id(&params.command) else {
            return Ok(None);
        };
        let item_name = command.item_name();
        let item_name_lowercase = item_name.to_ascii_lowercase();
        let Some(text) = params.arguments.first().and_then(Value::as_str) else {
            self.client
                .show_message(
                    MessageType::ERROR,
                    format!("No {item_name_lowercase} was provided to copy."),
                )
                .await;
            return Ok(None);
        };

        match self.copy_to_clipboard(text) {
            Ok(()) => {
                self.client
                    .show_message(MessageType::INFO, format!("{item_name} copied: {text}"))
                    .await;
            }
            Err(error) => {
                self.client
                    .show_message(
                        MessageType::ERROR,
                        format!("Failed to copy {item_name_lowercase}: {error}"),
                    )
                    .await;
            }
        }
        Ok(None)
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let Some(source) = self.document(&params.text_document.uri) else {
            return Ok(None);
        };
        let ranges = folding_ranges(&source)
            .into_iter()
            .map(|fold| FoldingRange {
                start_line: fold.start_line,
                end_line: fold.end_line,
                ..FoldingRange::default()
            })
            .collect();
        Ok(Some(ranges))
    }
}
