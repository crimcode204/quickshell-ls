use std::{collections::HashMap, sync::LazyLock};

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::{
    doc_state::DocState,
    workspace::{QMLComponent, WorkspaceState, ts_kinds},
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

        if node.parent()?.kind() == ts_kinds::UI_BINDING {
            let mut current_block = node.parent()?;
            while current_block.kind() != ts_kinds::UI_OBJECT_DEFINITION {
                current_block = current_block.parent()?;
            }

            let type_node = current_block.child_by_field_name("type_name")?;
            let parent_component_name = self.node_text(&type_node);

            if let Some(component) = workspace.component(&parent_component_name) {
                return Self::property_hover_info(&name, &component, workspace);
            }
        }

        if let Some(component) = workspace.component(&name) {
            return Some(Self::component_hover_info(&component, workspace));
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
            Some(proto_name) => match workspace.resolve_prototype(proto_name) {
                Some(p) => format!(" : {}", p.name()),
                None => "".to_string(),
            },
            None => "".to_string(),
        };
        let docs_md = match QT_DOCS.get(component.name()) {
            Some(docs) => docs,
            None => "",
        };

        let markdown = format!(
            "```qml\n{}{}{}\n```\n{}",
            module_md, component_md, proto_md, docs_md,
        );

        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: None,
        }
    }

    fn property_hover_info(
        prop_name: &str,
        component: &QMLComponent,
        workspace: &WorkspaceState,
    ) -> Option<Hover> {
        let mut current_proto = Some(component.cpp_name.clone());

        while let Some(proto_name) = current_proto {
            let Some(parent_comp) = workspace.resolve_prototype(&proto_name) else {
                break;
            };
            if let Some(prop) = parent_comp.properties.iter().find(|p| p.name == prop_name) {
                let markdown = format!(
                    "```qml\n(property) {}: {}\n```\n*Defined in `{}`*",
                    prop.name,
                    prop.type_name,
                    parent_comp.name(),
                );

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: None,
                });
            }
            current_proto = parent_comp.prototype.clone();
        }
        None
    }
}
