use std::{path::Path, sync::LazyLock};

use dashmap::DashMap;
use tokio::fs;
use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

static QMLTYPES_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let language = tree_sitter_qmljs::LANGUAGE.into();
    let query_string = r#"
        (ui_object_definition
            type_name: (identifier) @obj_type
        ) @object"#;

    Query::new(&language, query_string).expect("Failed to compile qml types query")
});

pub struct QMLProperty {
    name: String,
    type_name: String,
    description: Option<String>,
}

pub struct QMLSignal {
    name: String,
    parameters: Vec<QMLParameter>,
}
pub struct QMLMethod {
    name: String,
    parameters: Vec<QMLParameter>,
}
pub struct QMLParameter {
    name: String,
    // temporary string, this has to be a primitive type (int, bool, etc.) or a component
    p_type: String,
}

pub struct QMLComponent {
    pub name: String,
    prototype: Option<String>,
    pub qml_name: Option<String>,

    properties: Vec<QMLProperty>,
    signals: Vec<QMLSignal>,
    methods: Vec<QMLMethod>,
}

impl QMLComponent {
    pub fn qml_name(&self) -> &str {
        match &self.qml_name {
            Some(name) => name,
            None => &self.name,
        }
    }
}

pub struct WorkspaceState {
    pub components: DashMap<String, QMLComponent>,
}

impl WorkspaceState {
    /// Returns a new workspace state
    pub fn new() -> Self {
        Self {
            components: DashMap::new(),
        }
    }

    pub async fn index_directory(&self, root_path: impl AsRef<Path>) {
        let mut dirs_to_visit = vec![root_path.as_ref().to_path_buf()];

        while let Some(dir) = dirs_to_visit.pop() {
            let Ok(mut entries) = fs::read_dir(dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Ok(file_type) = entry.file_type().await {
                    if file_type.is_dir() {
                        dirs_to_visit.push(path);
                    } else if path.extension().and_then(|ext| ext.to_str()) == Some("qmltypes") {
                        if let Ok(content) = fs::read_to_string(&path).await {
                            self.parse_qmltypes(&content);
                        }
                    }
                }
            }
        }
    }

    pub fn parse_qmltypes(&self, source: &str) {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_qmljs::LANGUAGE.into())
            .unwrap();

        let tree = parser.parse(source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let text_bytes = source.as_bytes();

        cursor
            .captures(&QMLTYPES_QUERY, tree.root_node(), text_bytes)
            .for_each(|(capture_match, idx)| {
                let capture = capture_match.captures().get(*idx).unwrap();

                if QMLTYPES_QUERY.capture_names()[capture.index as usize] == "obj_type" {
                    let type_name = capture.node.utf8_text(text_bytes).unwrap_or("");
                    if type_name != "Component" {
                        return;
                    }

                    let component_node = capture.node.parent().unwrap();
                    let Some(mut component) = Self::extract_component(component_node, text_bytes)
                    else {
                        return;
                    };

                    if let Some(mut registered_component) =
                        self.components.get_mut(component.qml_name())
                    {
                        registered_component
                            .properties
                            .append(&mut component.properties);
                    } else {
                        self.components
                            .insert(component.qml_name().to_string(), component);
                    }
                }
            });
    }

    fn extract_component(node: Node, source: &[u8]) -> Option<QMLComponent> {
        let name = Self::find_binding_value(node, "name", source)?;
        let prototype = Self::find_binding_value(node, "prototype", source);
        let qml_name = Self::resolve_qml_name(Self::find_binding_value(node, "exports", source));

        let mut properties = Vec::new();

        let mut cursor = node.walk();
        node.children(&mut cursor).for_each(|child| {
            if child.kind() != ts_kinds::UI_OBJECT_INITIALIZER {
                return;
            }
            let mut init_cursor = child.walk();
            child.children(&mut init_cursor).for_each(|inner_child| {
                let Some(type_node) = inner_child.child_by_field_name("type_name") else {
                    return;
                };
                match type_node.utf8_text(source).unwrap_or("") {
                    "Property" => {
                        if let Some(prop) = Self::extract_property(inner_child, source) {
                            properties.push(prop);
                        }
                    }
                    "Signal" => return,
                    "Method" => return,
                    _ => return,
                }
            });
        });

        Some(QMLComponent {
            name,
            prototype,
            qml_name,
            properties,
            signals: vec![],
            methods: vec![],
        })
    }

    fn extract_property(node: Node, source: &[u8]) -> Option<QMLProperty> {
        let name = Self::find_binding_value(node, "name", source)?;
        let type_name =
            Self::find_binding_value(node, "type", source).unwrap_or_else(|| "var".to_string());

        Some(QMLProperty {
            name,
            type_name,
            description: None,
        })
    }

    fn find_binding_value(node: Node, key: &str, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        node.children(&mut cursor).find_map(|child| {
            if child.kind() != ts_kinds::UI_OBJECT_INITIALIZER {
                return None;
            }
            let mut init_cursor = child.walk();
            child.children(&mut init_cursor).find_map(|inner_child| {
                if inner_child.kind() != ts_kinds::UI_BINDING {
                    return None;
                }
                let name_node = inner_child.child_by_field_name("name")?;
                if name_node.utf8_text(source).unwrap_or("") != key {
                    return None;
                }
                let value_node = inner_child.child_by_field_name("value")?;
                let raw_string = value_node.utf8_text(source).unwrap_or("");
                return Some(raw_string.trim_matches('"').to_string());
            })
        })
    }

    /// Extracts the latest QML name from a list of exports
    fn resolve_qml_name(raw_exports: Option<String>) -> Option<String> {
        let binding = raw_exports?;

        // Skip first empty match, jump over separating commas
        let exports = binding.split('"').skip(1).step_by(2);

        let last_export = exports.last()?;
        last_export
            .split(' ')
            .next()?
            .split('/')
            .last()
            .map(String::from)
    }
}

mod ts_kinds {
    pub const UI_OBJECT_DEFINITION: &str = "ui_object_definition";
    pub const UI_OBJECT_INITIALIZER: &str = "ui_object_initializer";
    pub const UI_BINDING: &str = "ui_binding";
    pub const IDENTIFIER: &str = "identifier";
}
