use std::{collections::HashMap, sync::LazyLock};

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::{doc_state::DocState, workspace::WorkspaceState};

static JSON_QT_DOCS: &str = include_str!("../../data/qt_docs.json");
static QT_DOCS: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| serde_json::from_str(JSON_QT_DOCS).unwrap_or_default());

impl DocState {
    pub fn hover_info(
        &self,
        position: tower_lsp::lsp_types::Position,
        workspace: &WorkspaceState,
    ) -> Option<Hover> {
        let node = self.node_at(position)?;
        let name = self.node_text(&node);

        if let Some(global_component) = workspace.components.get(&name) {
            let mut markdown = format!("**Component:** `{}`\n", global_component.qml_name());

            if let Some(desc) = QT_DOCS.get(global_component.qml_name()) {
                markdown.push_str(&format!("--\n{}\n\n", desc));
            }

            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: markdown,
                }),
                range: None,
            });
        }

        None
    }
}
