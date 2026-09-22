use std::{collections::HashMap, sync::LazyLock};

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::{
    doc_state::DocState,
    workspace::{QMLComponent, WorkspaceState},
};

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

        if let Some(global_component) = workspace.component(&name) {
            return Some(Self::component_hover_info(&global_component, workspace));
        }

        None
    }

    fn component_hover_info(component: &QMLComponent, workspace: &WorkspaceState) -> Hover {
        let module_md = match &component.module {
            Some(module) => format!("import {}\n", module),
            None => "".to_string(),
        };
        let component_md = format!("component {}", component.name());
        let proto_md = match &component.prototype {
            Some(proto) => match workspace.component_by_cpp_name(proto) {
                Some(p) => format!(" : {}", p.name()),
                None => "".to_string(),
            },
            None => "".to_string(),
        };

        let markdown = format!(
            "```qml\n{}{}{}\n```\n{}",
            module_md,
            component_md,
            proto_md,
            QT_DOCS.get(component.name()).unwrap_or(&"".to_string()),
        );

        return Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: None,
        };
    }
}
