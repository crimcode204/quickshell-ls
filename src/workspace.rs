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
    pub(crate) name: String,
    pub(crate) type_name: String,
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
    pub cpp_name: String,
    pub qml_name: Option<String>,
    pub(crate) prototype: Option<String>,
    pub(crate) module: Option<String>,

    pub(crate) properties: Vec<QMLProperty>,
    signals: Vec<QMLSignal>,
    methods: Vec<QMLMethod>,
}

impl QMLComponent {
    pub fn name(&self) -> &str {
        match &self.qml_name {
            Some(name) => name,
            None => &self.cpp_name,
        }
    }
}

pub struct WorkspaceState {
    components: DashMap<String, QMLComponent>,
    cpp_to_qml: DashMap<String, String>,
}

impl WorkspaceState {
    /// Returns a new workspace state
    pub fn new() -> Self {
        Self {
            components: DashMap::new(),
            cpp_to_qml: DashMap::new(),
        }
    }

    /// Returns the component with a given name
    /// This uses the qml name, for getting a component by its cpp name use [`component_by_cpp_name()`]
    pub fn component(
        &self,
        name: &str,
    ) -> Option<dashmap::mapref::one::Ref<'_, String, QMLComponent>> {
        self.components.get(name)
    }

    pub fn remove_component(&self, name: &str) {
        self.components.remove(name);
    }

    /// Resolves a prototype name
    /// Prototype names have 2 forms: C++ (builtin types) or QML (local files)
    pub fn resolve_prototype(
        &self,
        proto_name: &str,
    ) -> Option<dashmap::mapref::one::Ref<'_, String, QMLComponent>> {
        if let Some(translated_qml_name) = self.cpp_to_qml.get(proto_name) {
            return self.components.get(translated_qml_name.value());
        }
        self.components.get(proto_name)
    }

    pub async fn index_directory(&self, root_path: impl AsRef<Path>) {
        let mut dirs_to_visit = vec![root_path.as_ref().to_path_buf()];

        while let Some(dir) = dirs_to_visit.pop() {
            let Ok(mut entries) = fs::read_dir(dir).await else {
                continue;
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if file_type.is_dir() {
                    dirs_to_visit.push(path);
                    continue;
                }

                let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
                    continue;
                };
                if extension == "qmltypes" {
                    if let Ok(content) = fs::read_to_string(&path).await {
                        self.parse_qmltypes(&content);
                    }
                } else if extension == "qml" {
                    if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str())
                        && file_stem.chars().next().map_or(false, |c| c.is_uppercase())
                        && let Ok(content) = tokio::fs::read_to_string(&path).await
                    {
                        self.parse_qml_file(&content, &path, root_path.as_ref());
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
        let text_bytes = source.as_bytes();

        let mut cursor = QueryCursor::new();
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
                        self.components.get_mut(component.name())
                    {
                        registered_component
                            .properties
                            .append(&mut component.properties);
                    } else {
                        self.cpp_to_qml
                            .insert(component.cpp_name.clone(), component.name().to_string());
                        self.components
                            .insert(component.name().to_string(), component);
                    }
                }
            });
    }

    pub fn parse_qml_file(&self, source: &str, file_path: &Path, root_path: &Path) {
        let file_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();

        let module_name = file_path
            .parent()
            .and_then(|p| p.strip_prefix(&root_path).ok())
            .and_then(|p| p.to_str())
            .map(|p| {
                if p.is_empty() {
                    "qs".to_string()
                } else {
                    format!("qs.{}", p).replace("/", ".")
                }
            })
            .unwrap_or_else(|| "Local Project".to_string());

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_qmljs::LANGUAGE.into())
            .unwrap();

        let tree = parser.parse(source, None).unwrap();
        let text_bytes = source.as_bytes();

        let root_node = tree.root_node();
        let mut cursor = root_node.walk();

        let Some(root_object) = root_node
            .children(&mut cursor)
            .find(|n| n.kind() == ts_kinds::UI_OBJECT_DEFINITION)
        else {
            return;
        };

        let prototype = root_object
            .child_by_field_name("type_name")
            .and_then(|n| n.utf8_text(text_bytes).ok())
            .map(String::from);

        let mut properties = Vec::new();

        if let Some(initializer) = root_object
            .children(&mut root_object.walk())
            .find(|n| n.kind() == ts_kinds::UI_OBJECT_INITIALIZER)
        {
            let mut init_cursor = initializer.walk();
            initializer.children(&mut init_cursor).for_each(|child| {
                if child.kind() != ts_kinds::UI_PROPERTY_DECLARATION {
                    return;
                }

                let Some(name) = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(text_bytes).ok())
                    .map(String::from)
                else {
                    return;
                };

                let type_name = child
                    .child_by_field_name("type")
                    .or_else(|| child.child_by_field_name("type_name"))
                    .and_then(|n| n.utf8_text(text_bytes).ok())
                    .unwrap_or("var")
                    .to_string();

                properties.push(QMLProperty {
                    name,
                    type_name,
                    description: None,
                });
            });
        }

        let component = QMLComponent {
            cpp_name: file_name.to_string(),
            qml_name: Some(file_name.to_string()),
            prototype,
            module: Some(module_name),
            properties,
            signals: vec![],
            methods: vec![],
        };
        self.components.insert(file_name.to_string(), component);
    }

    fn extract_component(node: Node, source: &[u8]) -> Option<QMLComponent> {
        let cpp_name = Self::find_binding_value(node, "name", source)?;
        let prototype = Self::find_binding_value(node, "prototype", source);
        let exports = Self::find_binding_value(node, "exports", source);
        let (qml_name, module) = Self::resolve_qml_name_and_module(exports);

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
            cpp_name,
            prototype,
            qml_name,
            module,

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
    fn resolve_qml_name_and_module(
        raw_exports: Option<String>,
    ) -> (Option<String>, Option<String>) {
        let Some(binding) = raw_exports else {
            return (None, None);
        };

        // Skip first empty match, jump over separating commas
        let exports = binding.split('"').skip(1).step_by(2);

        let Some(last_export) = exports.last() else {
            return (None, None);
        };
        let Some(name_and_module) = last_export.split(' ').next() else {
            return (None, None);
        };

        let mut parts = name_and_module.split('/');
        let first = parts.next();
        let second = parts.next();

        match (first, second) {
            (Some(module), Some(name)) => (Some(name.to_string()), Some(module.to_string())),
            (Some(name), None) => (Some(name.to_string()), None),
            _ => (None, None),
        }
    }
}

pub(crate) mod ts_kinds {
    pub const UI_PROPERTY_DECLARATION: &str = "ui_property_declaration";
    pub const UI_OBJECT_INITIALIZER: &str = "ui_object_initializer";
    pub const UI_OBJECT_DEFINITION: &str = "ui_object_definition";
    pub const UI_BINDING: &str = "ui_binding";
    pub const IDENTIFIER: &str = "identifier";
}
