use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Mutex, OnceLock, RwLock},
};

use dashmap::DashMap;
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};

use crate::{doc_state::DocState, handlers, registry::ComponentRegistry};

pub struct QuickshellLanguageServer {
    _client: Client,
    parser: Mutex<tree_sitter::Parser>,
    documents: DashMap<Url, DocState>,
    registry: RwLock<ComponentRegistry>,
    root_path: OnceLock<PathBuf>,
}

impl QuickshellLanguageServer {
    pub fn new(_client: Client) -> Self {
        let mut parser = tree_sitter::Parser::new();
        let language = tree_sitter_qmljs::LANGUAGE;
        parser
            .set_language(&language.into())
            .expect("Error loading QML parser");
        Self {
            _client,
            parser: Mutex::new(parser),
            documents: DashMap::new(),
            registry: RwLock::new(ComponentRegistry::new()),
            root_path: OnceLock::new(),
        }
    }

    fn refresh_document_components(&self, uri: &Url, doc_state: &DocState) {
        if let Ok(file_path) = uri.to_file_path()
            && file_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.chars().next().is_some_and(|c| c.is_uppercase()))
            && let Some(root_path) = self.root_path.get()
        {
            let text = doc_state.text.to_string();
            let components = ComponentRegistry::extract_qml_file_components(
                doc_state.tree.root_node(),
                text.as_bytes(),
                &file_path,
                root_path,
            );
            // Can optimize: check which components actually change under a read lock
            // Only update those ones under a write lock
            if let Ok(mut registry) = self.registry.write() {
                registry.remove_component(&doc_state.name);
                components
                    .into_iter()
                    .for_each(|comp| registry.add_component(comp));
            }
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
            self.root_path
                .set(path)
                .expect("initialize should only be set once");
        }

        Ok(InitializeResult {
            offset_encoding: None,
            server_info: Some(ServerInfo {
                name: "quickshell-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            capabilities: ServerCapabilities {
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    all_commit_characters: None,
                    work_done_progress_options: Default::default(),
                    completion_item: None,
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                position_encoding: Some(PositionEncodingKind::UTF8),
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
                workspace: Some(WorkspaceServerCapabilities {
                    file_operations: Some(WorkspaceFileOperationsServerCapabilities {
                        did_create: Some(FileOperationRegistrationOptions {
                            filters: vec![FileOperationFilter {
                                scheme: Some(String::from("file")),
                                pattern: FileOperationPattern {
                                    glob: String::from("**/*.qml"),
                                    matches: Some(FileOperationPatternKind::File),
                                    options: None,
                                },
                            }],
                        }),
                        will_rename: Some(FileOperationRegistrationOptions {
                            filters: vec![FileOperationFilter {
                                scheme: Some(String::from("file")),
                                pattern: FileOperationPattern {
                                    glob: String::from("**/*.qml"),
                                    matches: Some(FileOperationPatternKind::File),
                                    options: None,
                                },
                            }],
                        }),
                        did_rename: Some(FileOperationRegistrationOptions {
                            filters: vec![FileOperationFilter {
                                scheme: Some(String::from("file")),
                                pattern: FileOperationPattern {
                                    glob: String::from("**/*.qml"),
                                    matches: Some(FileOperationPatternKind::File),
                                    options: None,
                                },
                            }],
                        }),
                        did_delete: Some(FileOperationRegistrationOptions {
                            filters: vec![FileOperationFilter {
                                scheme: Some(String::from("file")),
                                pattern: FileOperationPattern {
                                    glob: String::from("**/*.qml"),
                                    matches: Some(FileOperationPatternKind::File),
                                    options: None,
                                },
                            }],
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let mut paths = Vec::new();

        if let Some(root) = self.root_path.get() {
            paths.push(root.clone());

            let ini_path = root.join(".qmlls.ini");
            if let Ok(ini_content) = tokio::fs::read_to_string(&ini_path).await {
                for line in ini_content.lines() {
                    if let Some(import_paths) = line
                        .strip_prefix("importPaths=")
                        .map(|paths| paths.trim_matches('"'))
                    {
                        paths.extend(import_paths.split(':').map(PathBuf::from));
                    };
                }
            }
        }

        if let Ok(qml_paths) = std::env::var("QML_IMPORT_PATH") {
            paths.extend(std::env::split_paths(&qml_paths));
        }

        paths.extend(
            ["qtpaths6", "qtpaths", "qmake6", "qmake"]
                .into_iter()
                .filter_map(|tool| {
                    let flag = if tool.starts_with("qmake") {
                        "-query"
                    } else {
                        "--query"
                    };
                    std::process::Command::new(tool)
                        .args([flag, "QT_INSTALL_QML"])
                        .output()
                        .ok()
                })
                .filter(|output| output.status.success())
                .filter_map(|output| String::from_utf8(output.stdout).ok())
                .map(|s| PathBuf::from(s.trim()))
                .collect::<HashSet<PathBuf>>(),
        );

        for path in paths {
            ComponentRegistry::index_directory(&self.registry, path).await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;

        let text = params.text_document.text;
        let file_stem = uri
            .to_file_path()
            .ok()
            .and_then(|path| path.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();

        let Ok(doc_state) = self
            .parser
            .lock()
            .map(|mut parser| DocState::new(file_stem, text, &mut parser))
        else {
            return;
        };

        self.refresh_document_components(&uri, &doc_state);
        self.documents.insert(uri, doc_state);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        if let Some(mut doc_state) = self.documents.get_mut(&uri) {
            if let Ok(mut parser) = self.parser.lock() {
                doc_state.update(params.content_changes, &mut parser);
            }
            self.refresh_document_components(&uri, &doc_state);
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;

        if let Some(doc_state) = self.documents.get_mut(&uri) {
            self.refresh_document_components(&uri, &doc_state);
        }

        // Add qmllint diagnostics here later
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);
    }

    async fn did_create_files(&self, params: CreateFilesParams) {
        for file in params.files {
            if let Ok(uri) = Url::parse(&file.uri)
                && let Ok(path) = uri.to_file_path()
                && path.extension().and_then(|ext| ext.to_str()) == Some("qml")
                && path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map_or(false, |stem| {
                        stem.chars().next().map_or(false, |c| c.is_uppercase())
                    })
                && let Some(root_path) = self.root_path.get()
                && let Ok(content) = tokio::fs::read_to_string(&path).await
            {
                let components = ComponentRegistry::parse_qml_file(&content, &path, root_path);
                if let Ok(mut registry) = self.registry.write() {
                    components
                        .into_iter()
                        .for_each(|comp| registry.add_component(comp));
                }
            }
        }
    }

    async fn will_rename_files(&self, params: RenameFilesParams) -> Result<Option<WorkspaceEdit>> {
        let renames: Vec<(String, String)> = params
            .files
            .iter()
            .filter_map(|file| {
                Some((
                    qml_component_stem(&file.old_uri)?,
                    qml_component_stem(&file.new_uri)?,
                ))
            })
            .collect();

        if let Ok(registry) = self.registry.read() {
            return Ok(handlers::will_rename_files(
                &renames,
                &self.documents,
                &registry,
            ));
        }
        Ok(None)
    }

    async fn did_rename_files(&self, params: RenameFilesParams) {
        for file in params.files {
            if let Ok(old_uri) = Url::parse(&file.old_uri)
                && let Ok(path) = old_uri.to_file_path()
                && let Some(file_stem) = path.file_stem().and_then(|s| s.to_str())
                && let Ok(mut registry) = self.registry.write()
            {
                registry.remove_component(file_stem);

                if let Some((_, mut doc_state)) = self.documents.remove(&old_uri)
                    && let Ok(new_uri) = Url::parse(&file.new_uri)
                    && let Ok(new_path) = new_uri.to_file_path()
                    && let Some(new_stem) = new_path.file_stem().and_then(|s| s.to_str())
                {
                    doc_state.name = new_stem.to_string();
                    self.documents.insert(new_uri, doc_state);
                }
            }

            if let Ok(new_uri) = Url::parse(&file.new_uri)
                && let Ok(new_path) = new_uri.to_file_path()
                && new_path.extension().and_then(|ext| ext.to_str()) == Some("qml")
                // First char is upper case
                && new_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map_or(false, |stem| {
                        stem.chars().next().map_or(false, |c| c.is_uppercase())
                    })
                && let Some(root_path) = self.root_path.get()
                && let Ok(content) = tokio::fs::read_to_string(&new_path).await
            {
                let components = ComponentRegistry::parse_qml_file(&content, &new_path, root_path);
                if let Ok(mut registry) = self.registry.write() {
                    components
                        .into_iter()
                        .for_each(|comp| registry.add_component(comp));
                }
            }
        }
    }

    async fn did_delete_files(&self, params: DeleteFilesParams) {
        params.files.into_iter().for_each(|file| {
            if let Ok(uri) = Url::parse(&file.uri)
                && let Ok(path) = uri.to_file_path()
                && let Some(file_stem) = path.file_stem().and_then(|s| s.to_str())
                && let Ok(mut registry) = self.registry.write()
            {
                registry.remove_component(file_stem);
            }
        });
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc_state) = self.documents.get(&uri)
            && let Ok(registry) = self.registry.read()
        {
            return Ok(handlers::hover(position, &doc_state, &registry));
        }

        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        if let Some(doc_state) = self.documents.get(&uri)
            && let Ok(registry) = self.registry.read()
        {
            return Ok(handlers::completion(position, &doc_state, &registry));
        }

        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;

        if let Some(doc_state) = self.documents.get(&uri) {
            return Ok(handlers::document_symbol(&doc_state));
        } else {
            Ok(None)
        }
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        if let Ok(registry) = self.registry.read() {
            return Ok(handlers::workspace_symbol(&params.query, &registry));
        }

        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc_state) = self.documents.get(&uri)
            && let Ok(registry) = self.registry.read()
        {
            return Ok(handlers::goto_definition(
                &uri, position, &doc_state, &registry,
            ));
        }

        Ok(None)
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc_state) = self.documents.get(&uri)
            && let Ok(registry) = self.registry.read()
        {
            return Ok(handlers::document_highlight(
                position, &doc_state, &registry,
            ));
        }

        Ok(None)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri;
        let position = params.position;

        if let Some(doc_state) = self.documents.get(&uri)
            && let Ok(registry) = self.registry.read()
        {
            return Ok(handlers::prepare_rename(position, &doc_state, &registry));
        }

        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        let result = {
            let Some(doc_state) = self.documents.get(&uri) else {
                return Ok(None);
            };
            let Ok(registry) = self.registry.read() else {
                return Ok(None);
            };
            handlers::rename(
                &uri,
                position,
                new_name,
                &doc_state,
                &self.documents,
                &registry,
            )
        };

        let Some((edit, sync)) = result else {
            return Ok(None);
        };

        if let Some(sync) = sync
            && let Ok(mut registry) = self.registry.write()
        {
            match sync {
                handlers::RegistrySync::Property {
                    comp_name,
                    old_name,
                    new_name,
                } => registry.rename_property(&comp_name, &old_name, &new_name),
                handlers::RegistrySync::InlineComponent { old_name, new_name } => {
                    registry.rename_inline_component(&old_name, &new_name)
                }
            }
        }

        Ok(Some(edit))
    }
}

fn qml_component_stem(uri_str: &str) -> Option<String> {
    let path = Url::parse(uri_str).ok()?.to_file_path().ok()?;
    if path.extension().and_then(|e| e.to_str()) != Some("qml") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    stem.chars()
        .next()
        .is_some_and(|c| c.is_uppercase())
        .then(|| stem.to_string())
}
