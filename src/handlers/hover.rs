use std::{collections::HashMap, sync::LazyLock};

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::{
    doc_state::DocState,
    handlers::resolver::{
        ResolvedProperty, resolve_component, resolve_property_in_scope, resolve_prototype,
    },
    qml::{QMLComponent, QMLProperty, QMLPropertyType},
    registry::ComponentRegistry,
};

static QT_DOCS: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/qt_docs.json")).unwrap_or_default()
});

pub fn hover(
    position: tower_lsp::lsp_types::Position,
    doc_state: &DocState,
    workspace: &ComponentRegistry,
) -> Option<Hover> {
    let node = doc_state.node_at(position)?;
    let name = doc_state.node_text(&node);
    let byte_offset = node.start_byte();

    if let Some(resolved) = resolve_property_in_scope(byte_offset, &name, doc_state, workspace) {
        match resolved {
            ResolvedProperty::Local {
                property,
                owner_type_name,
            } => {
                return Some(
                    HoverBuilder::new()
                        .signature(format!("property {}: {}", property.name, property.p_type))
                        .prop_owner(owner_type_name)
                        .build(),
                );
            }
            ResolvedProperty::Global(component) => {
                let mut current_proto = Some(component.cpp_name.clone());

                while let Some(proto_name) = current_proto {
                    let Some(parent_comp) = resolve_prototype(&proto_name, doc_state, workspace)
                    else {
                        break;
                    };
                    if let Some(prop) = parent_comp.properties.iter().find(|p| p.name == name) {
                        return Some(global_property_hover_info(
                            &prop, &component, doc_state, workspace,
                        ));
                    }
                    current_proto = parent_comp.prototype.clone();
                }
                return None;
            }
        }
    }

    if let Some(component) = resolve_component(&name, doc_state, workspace) {
        return Some(component_hover_info(&component));
    }

    None
}

fn component_hover_info(component: &QMLComponent) -> Hover {
    let module_str = component.module.as_deref().unwrap_or("");
    let proto_str = component.prototype.as_deref().unwrap_or("");

    let signature = format!(
        "{}component {}{}",
        if module_str.is_empty() {
            "".to_string()
        } else {
            format!("import {}\n\n", module_str)
        },
        component.name(),
        if proto_str.is_empty() {
            "".to_string()
        } else {
            format!(" : {}", proto_str)
        }
    );

    let mut builder = HoverBuilder::new().signature(signature);

    if let Some(docs) = QT_DOCS.get(component.name()) {
        builder = builder.docs(docs);
    }

    builder.build()
}

fn global_property_hover_info(
    property: &QMLProperty,
    parent_comp: &QMLComponent,
    doc_state: &DocState,
    workspace: &ComponentRegistry,
) -> Hover {
    let mut builder = HoverBuilder::new()
        .signature(format!("property {}: {}", property.name, property.p_type))
        .prop_owner(parent_comp.name());

    if let QMLPropertyType::Custom(comp_name) = &property.p_type {
        if let Some(type_component) = resolve_component(comp_name, doc_state, workspace) {
            if let Some(docs) = QT_DOCS.get(type_component.name()) {
                builder = builder.docs(format!("**{}**\n{}", type_component.name(), docs));
            }
        }
    }

    builder.build()
}

#[derive(Default)]
struct HoverBuilder {
    signature: String,
    prop_owner: Option<String>,
    docs: Option<String>,
}

impl HoverBuilder {
    fn new() -> Self {
        Self::default()
    }

    fn signature(mut self, sig: impl Into<String>) -> Self {
        self.signature = sig.into();
        self
    }

    fn prop_owner(mut self, owner: impl Into<String>) -> Self {
        self.prop_owner = Some(owner.into());
        self
    }

    fn docs(mut self, docs: impl Into<String>) -> Self {
        self.docs = Some(docs.into());
        self
    }

    fn build(self) -> Hover {
        let mut markdown = String::with_capacity(32);

        if let Some(owner) = self.prop_owner {
            markdown.push_str(&format!("**{}**\n\n", owner));
        }

        markdown.push_str(&format!("```qml\n{}\n```", self.signature));

        if let Some(docs) = self.docs {
            markdown.push_str(&format!("\n\n{}", docs));
        }

        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: None,
        }
    }
}
