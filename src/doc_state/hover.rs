use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::{doc_state::DocState, workspace::WorkspaceState};

impl DocState {
    pub fn hover_info(
        &self,
        position: tower_lsp::lsp_types::Position,
        workspace: &WorkspaceState,
    ) -> Option<Hover> {
        let node = self.node_at(position)?;
        let name = self.node_text(&node);

        if let Some(global_component) = workspace.components.get(&name) {
            let markdown = format!("**Component:** `{}`", global_component.name);

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
