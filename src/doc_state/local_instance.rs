use std::{ops::Range, str::FromStr};
use tree_sitter::Node;

use crate::{
    qml::{QMLProperty, QMLPropertyModifier},
    ts_kinds,
};

pub struct LocalInstance {
    pub(crate) byte_range: Range<usize>,
    pub(crate) type_name: String,
    pub(crate) id: Option<String>,
    pub(crate) properties: Vec<QMLProperty>,
    pub(crate) children: Vec<LocalInstance>,
}

impl LocalInstance {
    pub fn build_tree(node: Node, source: &[u8], doc_name: Option<&str>) -> Vec<LocalInstance> {
        let mut instances = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == ts_kinds::UI_OBJECT_DEFINITION
                || child.kind() == ts_kinds::UI_INLINE_COMPONENT
            {
                instances.push(Self::parse_object(child, source, doc_name));
            } else {
                instances.append(&mut Self::build_tree(child, source, None));
            }
        }

        instances
    }

    fn parse_object(node: Node, source: &[u8], doc_name: Option<&str>) -> LocalInstance {
        let type_name = if node.kind() == ts_kinds::UI_INLINE_COMPONENT {
            node.child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .unwrap_or("Item")
                .to_string()
        } else if let Some(name) = doc_name {
            name.to_string()
        } else {
            node.child_by_field_name("type_name")
                .and_then(|n| n.utf8_text(source).ok())
                .unwrap_or("Item")
                .to_string()
        };

        let target_node = if node.kind() == ts_kinds::UI_INLINE_COMPONENT {
            node.child_by_field_name("component").unwrap_or(node)
        } else {
            node
        };

        let byte_range = node.byte_range();
        let mut properties = Vec::new();
        let mut children = Vec::new();
        let mut id = None;

        let mut cursor = target_node.walk();
        if let Some(initializer) = target_node
            .children(&mut cursor)
            .find(|n| n.kind() == ts_kinds::UI_OBJECT_INITIALIZER)
        {
            let mut init_cursor = initializer.walk();

            for item in initializer.children(&mut init_cursor) {
                match item.kind() {
                    ts_kinds::UI_PROPERTY => {
                        if let Some(prop) = Self::extract_property(item, source) {
                            properties.push(prop);
                        }
                    }
                    ts_kinds::UI_OBJECT_DEFINITION => {
                        children.push(Self::parse_object(item, source, None));
                    }
                    ts_kinds::UI_BINDING => {
                        if let Some(name_node) = item.child_by_field_name("name") {
                            if name_node.utf8_text(source).unwrap_or("") == "id" {
                                if let Some(value_node) = item.child_by_field_name("value") {
                                    id = value_node
                                        .utf8_text(source)
                                        .ok()
                                        .map(|s| s.trim().to_string());
                                }
                            }
                        }

                        children.append(&mut Self::build_tree(item, source, None));
                    }
                    _ => {
                        children.append(&mut Self::build_tree(item, source, None));
                    }
                }
            }
        }

        LocalInstance {
            byte_range,
            type_name,
            id,
            properties,
            children,
        }
    }

    fn extract_property(node: Node, source: &[u8]) -> Option<QMLProperty> {
        let name = node
            .child_by_field_name("name")?
            .utf8_text(source)
            .ok()?
            .to_string();

        let type_name = node
            .child_by_field_name("type")
            .or_else(|| node.child_by_field_name("type_name"))
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("var")
            .to_string();

        let modifier = node
            .children(&mut node.walk())
            .find(|inner_child| inner_child.kind() == ts_kinds::UI_PROPERTY_MODIFIER)
            .and_then(|inner_child| inner_child.utf8_text(source).ok())
            .and_then(|text| QMLPropertyModifier::from_str(text).ok())
            .unwrap_or_default();

        Some(QMLProperty {
            name,
            p_type: type_name.into(),
            modifier,
        })
    }
}
