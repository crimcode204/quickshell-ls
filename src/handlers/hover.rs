use std::{collections::HashMap, sync::LazyLock};

use serde::Deserialize;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::{
    doc_state::DocState,
    handlers::resolver::{ExpressionMember, resolve_component, resolve_expression},
    qml::{QMLComponent, QMLProperty},
    registry::ComponentRegistry,
};

#[derive(Deserialize)]
struct QtComponentDocs {
    description: String,
    properties: HashMap<String, String>,
}

static QT_DOCS: LazyLock<HashMap<String, QtComponentDocs>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/qt_docs.json")).unwrap_or_default()
});

pub fn hover(
    position: tower_lsp::lsp_types::Position,
    doc_state: &DocState,
    workspace: &ComponentRegistry,
) -> Option<Hover> {
    let node = doc_state.node_at(position)?;

    match resolve_expression(node, doc_state, workspace)? {
        ExpressionMember::LocalProperty {
            property,
            owner_type_name,
        } => {
            return Some(property_hover_info(&property, owner_type_name));
        }
        ExpressionMember::GlobalProperty {
            component,
            prop_name,
        } => {
            let mut current_proto = Some(component.cpp_name.clone());

            while let Some(proto_name) = current_proto {
                let Some(parent_comp) = resolve_component(&proto_name, doc_state, workspace) else {
                    break;
                };
                if let Some(prop) = parent_comp.properties.iter().find(|p| p.name == prop_name) {
                    return Some(property_hover_info(&prop, component.name()));
                }
                current_proto = parent_comp.prototype.clone();
            }
            return None;
        }
        ExpressionMember::Component(component) => return Some(component_hover_info(&component)),
        ExpressionMember::IdReference(instance) => {
            let signature = format!("id {}: {}", doc_state.node_text(&node), instance.type_name);
            let mut builder = HoverBuilder::new().signature(signature);

            if let Some(docs) = QT_DOCS.get(&instance.type_name) {
                builder = builder.docs(&docs.description);
            }

            return Some(builder.build());
        }
    }
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
        builder = builder.docs(&docs.description);
    }

    builder.build()
}

fn property_hover_info(property: &QMLProperty, parent_comp_name: &str) -> Hover {
    let mut builder = HoverBuilder::new()
        .signature(format!(
            "{}property {}: {}",
            property.modifier, property.name, property.p_type
        ))
        .prop_owner(parent_comp_name);

    if let Some(parent_docs) = QT_DOCS.get(parent_comp_name) {
        if let Some(prop_desc) = parent_docs.properties.get(&property.name) {
            builder = builder.docs(prop_desc)
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
