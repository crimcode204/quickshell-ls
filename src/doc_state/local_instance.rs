use std::ops::Range;
use tree_sitter::Node;

use crate::{qml::QMLProperty, ts_kinds};

pub struct LocalInstance {
    pub byte_range: Range<usize>,
    pub type_name: String,
    pub id: Option<String>,
    pub properties: Vec<QMLProperty>,
    pub children: Vec<LocalInstance>,
}

impl LocalInstance {
    pub fn build_tree(node: Node, source: &[u8]) -> Vec<LocalInstance> {
        let mut instances = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == ts_kinds::UI_OBJECT_DEFINITION {
                instances.push(Self::parse_object(child, source));
            } else {
                instances.append(&mut Self::build_tree(child, source));
            }
        }

        instances
    }

    fn parse_object(node: Node, source: &[u8]) -> LocalInstance {
        let type_name = node
            .child_by_field_name("type_name")
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("Item")
            .to_string();

        let byte_range = node.byte_range();
        let mut properties = Vec::new();
        let mut children = Vec::new();
        let mut id = None;

        let mut cursor = node.walk();
        if let Some(initializer) = node
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
                        children.push(Self::parse_object(item, source));
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
                    }
                    _ => {}
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

        Some(QMLProperty {
            name,
            p_type: type_name.into(),
            description: None,
        })
    }
}
