use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use dashmap::DashMap;
use tokio::sync::Mutex;
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};

use crate::{doc_state::DocState, workspace::WorkspaceState};

pub struct QuickshellLanguageServer {
    client: Client,
    parser: Mutex<tree_sitter::Parser>,
    documents_map: DashMap<String, DocState>,
    workspace: Arc<WorkspaceState>,
    root_path: OnceLock<PathBuf>,
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
            workspace: Arc::new(WorkspaceState::new()),
            root_path: OnceLock::new(),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for QuickshellLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root_url = params
            .workspace_folders
            .and_then(|mut folders| folders.pop().map(|f| f.uri))
            .or(params.root_uri);

        if let Some(url) = root_url
            && let Ok(path) = url.to_file_path()
        {
            let _ = self.root_path.set(path);
        }

        Ok(InitializeResult {
            offset_encoding: None,
            server_info: Some(ServerInfo {
                name: "quickshell-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            capabilities: ServerCapabilities {
                document_symbol_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
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
        let workspace = self.workspace.clone();
        let root_path = self.root_path.get().cloned();

        tokio::spawn(async move {
            if let Some(root) = root_path {
                workspace.index_directory(&root).await;

                let ini_path = root.join(".qmlls.ini");
                if let Ok(ini_content) = tokio::fs::read_to_string(&ini_path).await {
                    for line in ini_content.lines() {
                        let Some(import_paths) = line
                            .strip_prefix("importPaths=")
                            .and_then(|paths| Some(paths.trim_matches('"')))
                        else {
                            continue;
                        };

                        for path in import_paths.split(':') {
                            workspace.index_directory(path).await;
                        }
                    }
                }
            }

            if let Ok(qml_paths) = std::env::var("QML_IMPORT_PATH") {
                for path in std::env::split_paths(&qml_paths) {
                    workspace.index_directory(path).await;
                }
            }

            let qt_paths_cmd = std::process::Command::new("qtpaths")
                .args(["--query", "QT_INSTALL_QML"])
                .output()
                .or_else(|_| {
                    std::process::Command::new("qmake")
                        .args(["-query", "QT_INSTALL_QML"])
                        .output()
                });

            if let Ok(output) = qt_paths_cmd
                && output.status.success()
                && let Ok(path_str) = String::from_utf8(output.stdout)
            {
                workspace.index_directory(path_str.trim()).await;
            }
        });
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        let text = params.text_document.text;
        let mut parser = self.parser.lock().await;
        let doc_state = DocState::new(text, &mut parser);

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

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri.to_string();

        if let Some(doc_state) = self.documents_map.get(&uri) {
            let lsp_symbols = doc_state.symbols.iter().map(DocumentSymbol::from).collect();

            Ok(Some(DocumentSymbolResponse::Nested(lsp_symbols)))
        } else {
            Ok(None)
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        if let Some(doc_state) = self.documents_map.get(&uri) {
            return Ok(doc_state.hover_info(position, &self.workspace));
        }

        Ok(None)
    }
}
