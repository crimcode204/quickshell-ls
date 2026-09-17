use dashmap::DashMap;
use tokio::sync::Mutex;
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};

use crate::doc_state::DocumentState;

pub struct QuickshellLanguageServer {
    client: Client,
    parser: Mutex<tree_sitter::Parser>,
    documents_map: DashMap<String, DocumentState>,
}

impl QuickshellLanguageServer {
    pub fn new(client: Client) -> Self {
        let mut parser = tree_sitter::Parser::new();
        let language = tree_sitter_qmljs::LANGUAGE;
        parser
            .set_language(&language.into())
            .expect("Error loading QML parser");
        Self {
            client,
            parser: Mutex::new(parser),
            documents_map: DashMap::new(),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for QuickshellLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            offset_encoding: None,
            server_info: Some(ServerInfo {
                name: "quickshell-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(false),
                        })),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::LOG, "server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        let text = params.text_document.text;
        let mut parser = self.parser.lock().await;
        let doc_state = DocumentState::new(text, &mut parser);

        self.client
            .log_message(
                MessageType::LOG,
                format!("{}\n{}\n", uri, doc_state.print_symbols()),
            )
            .await;

        self.documents_map.insert(uri, doc_state);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        if let Some(mut doc_state) = self.documents_map.get_mut(&uri) {
            let mut parser = self.parser.lock().await;
            doc_state.update(params.content_changes, &mut parser);
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.client
            .log_message(
                MessageType::LOG,
                format!("file '{}' saved!", params.text_document.uri),
            )
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        self.documents_map.remove(&uri);
    }
}
