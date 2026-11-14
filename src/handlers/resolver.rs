use std::collections::HashSet;

use tree_sitter::Node;

use crate::{
    doc_state::{DocState, LocalInstance},
    qml::{QMLComponent, QMLProperty, QMLPropertyType},
    registry::ComponentRegistry,
    ts_kinds,
};

pub enum ExpressionMember<'a> {
    LocalProperty {
        property: &'a QMLProperty,
        instance: &'a LocalInstance,
    },
    GlobalProperty {
        property: &'a QMLProperty,
        component: &'a QMLComponent,
    },
    Component(&'a QMLComponent),
    IdReference(&'a LocalInstance),
}

pub fn resolve_component<'a>(
    name: &str,
    doc_state: &DocState,
    registry: &'a ComponentRegistry,
) -> Option<&'a QMLComponent> {
    let local_name = format!("{}.{}", doc_state.name, name);
    if let Some(comp) = registry.component(&local_name) {
        return Some(comp);
    }

    if let Some(comp) = registry.component(name) {
        return Some(comp);
    }

    registry.resolve_by_cpp_name(name)
}

fn get_instance_path<'a>(
    instances: &'a [LocalInstance],
    byte_offset: usize,
    path: &mut Vec<&'a LocalInstance>,
) {
    for instance in instances {
        if instance.byte_range.contains(&byte_offset) {
            path.push(instance);
            get_instance_path(&instance.children, byte_offset, path);
            return;
        }
    }
}

pub fn resolve_property_in_scope<'a>(
    byte_offset: usize,
    prop_name: &str,
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> Option<ExpressionMember<'a>> {
    let mut path = Vec::new();
    get_instance_path(&doc_state.instances, byte_offset, &mut path);

    for instance in path.into_iter().rev() {
        if let Some(prop) = instance.properties.iter().find(|p| p.name == prop_name) {
            return Some(ExpressionMember::LocalProperty {
                property: prop,
                instance,
            });
        }

        if let Some(global_prop) =
            resolve_property_on_type(&instance.type_name, prop_name, doc_state, registry)
        {
            return Some(global_prop);
        }
    }

    None
}

fn find_instance_by_id<'a>(
    instances: &'a [LocalInstance],
    target_id: &str,
) -> Option<&'a LocalInstance> {
    for instance in instances {
        if let Some(id) = &instance.id {
            if id == target_id {
                return Some(instance);
            }
        }

        if let Some(found) = find_instance_by_id(&instance.children, target_id) {
            return Some(found);
        }
    }
    None
}

pub fn resolve_id<'a>(id_name: &str, doc_state: &'a DocState) -> Option<&'a LocalInstance> {
    find_instance_by_id(&doc_state.instances, id_name)
}

pub fn resolve_property_on_type<'a>(
    type_name: &str,
    prop_name: &str,
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> Option<ExpressionMember<'a>> {
    let mut current_proto = Some(type_name);

    while let Some(proto_name) = current_proto {
        let comp = resolve_component(proto_name, doc_state, registry)?;
        if let Some(property) = comp.properties.iter().find(|p| p.name == prop_name) {
            return Some(ExpressionMember::GlobalProperty {
                property,
                component: comp,
            });
        }
        current_proto = comp.prototype.as_deref();
    }

    None
}

pub fn resolve_identifier<'a>(
    byte_offset: usize,
    name: &str,
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> Option<ExpressionMember<'a>> {
    if name == "parent" {
        let mut path = Vec::new();
        get_instance_path(&doc_state.instances, byte_offset, &mut path);

        if path.len() >= 2 {
            let parent_instance = path[path.len() - 2];

            return Some(ExpressionMember::IdReference(parent_instance));
        }
    }

    if let Some(instance) = resolve_id(name, doc_state) {
        return Some(ExpressionMember::IdReference(instance));
    }

    if let Some(resolved) = resolve_property_in_scope(byte_offset, name, doc_state, registry) {
        return Some(resolved);
    }

    if let Some(component) = resolve_component(name, doc_state, registry) {
        return Some(ExpressionMember::Component(component));
    }

    None
}

pub fn resolve_expression<'a>(
    node: tree_sitter::Node<'_>,
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> Option<ExpressionMember<'a>> {
    match node.kind() {
        ts_kinds::IDENTIFIER | ts_kinds::TYPE_IDENTIFIER => {
            let name = doc_state.node_text(&node);

            if let Some(parent) = node.parent() {
                if parent.kind() == ts_kinds::NESTED_IDENTIFIER
                    && let Some(right_node) = parent.child(2)
                    && right_node.id() == node.id()
                    && let Some(object_node) = parent.child(0)
                {
                    return resolve_member_access(object_node, &name, doc_state, registry);
                }

                let binding_node = if parent.kind() == ts_kinds::NESTED_IDENTIFIER {
                    parent.parent()
                } else {
                    Some(parent)
                };

                if let Some(binding) = binding_node
                    && matches!(binding.kind(), ts_kinds::UI_BINDING | ts_kinds::UI_PROPERTY)
                    && let Some(name_node) = parent.child_by_field_name("name")
                    && (name_node.id() == node.id()
                        || (parent.kind() == ts_kinds::NESTED_IDENTIFIER
                            && name_node.id() == parent.id()))
                {
                    return resolve_property_in_scope(
                        node.start_byte(),
                        &name,
                        doc_state,
                        registry,
                    );
                }
            }

            resolve_identifier(node.start_byte(), &name, doc_state, registry)
        }
        ts_kinds::PROPERTY_IDENTIFIER => {
            if let Some(parent) = node.parent() {
                let object_node = parent.child_by_field_name("object")?;
                let prop_name = doc_state.node_text(&node);
                return resolve_member_access(object_node, &prop_name, doc_state, registry);
            }
            None
        }
        ts_kinds::MEMBER_EXPRESSION | ts_kinds::NESTED_IDENTIFIER => {
            let object_node = node
                .child_by_field_name("object")
                .or_else(|| node.child(0))?;
            let property_node = node
                .child_by_field_name("property")
                .or_else(|| node.child(2))?;

            let property_name = doc_state.node_text(&property_node);

            resolve_member_access(object_node, &property_name, doc_state, registry)
        }
        _ => None,
    }
}

pub fn resolve_member_access<'a>(
    base_node: Node,
    prop_name: &str,
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> Option<ExpressionMember<'a>> {
    match resolve_expression(base_node, doc_state, registry)? {
        ExpressionMember::LocalProperty { property, .. } => {
            let type_name = match &property.p_type {
                crate::qml::QMLPropertyType::BuiltIn(_) => return None,
                crate::qml::QMLPropertyType::Custom(custom_type) => custom_type,
            };
            resolve_property_on_type(type_name, prop_name, doc_state, registry)
        }
        ExpressionMember::GlobalProperty {
            property,
            component: owner,
        } => {
            let QMLPropertyType::Custom(type_name) = &property.p_type else {
                return None;
            };

            let base_file_name = owner.name().split('.').next().unwrap_or(owner.name());
            let composite_name = format!("{}.{}", base_file_name, type_name);

            if registry.component(&composite_name).is_some() {
                return resolve_property_on_type(&composite_name, prop_name, doc_state, registry);
            }

            resolve_property_on_type(type_name, prop_name, doc_state, registry)
        }
        ExpressionMember::Component(component) => {
            resolve_property_on_type(component.name(), prop_name, doc_state, registry)
        }
        ExpressionMember::IdReference(instance) => {
            if let Some(prop) = instance.properties.iter().find(|p| p.name == prop_name) {
                return Some(ExpressionMember::LocalProperty {
                    property: prop,
                    instance,
                });
            }
            resolve_property_on_type(&instance.type_name, prop_name, doc_state, registry)
        }
    }
}

/// Walks the prototype chain of a `type_name` and collects all properties.
pub fn collect_type_properties<'a>(
    type_name: &str,
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> Vec<(&'a QMLProperty, &'a str)> {
    let mut properties = Vec::new();
    let mut seen = HashSet::new();
    let mut current_proto = Some(type_name);

    while let Some(proto_name) = current_proto {
        let Some(comp) = resolve_component(&proto_name, doc_state, registry) else {
            break;
        };
        let owner_name = comp.name();

        comp.properties.iter().for_each(|prop| {
            if seen.insert(prop.name.as_str()) {
                properties.push((prop, owner_name));
            }
        });

        current_proto = comp.prototype.as_deref();
    }

    properties
}

/// Resolves all properties available on a node(member) of an expression.
pub fn resolve_member_properties<'a>(
    base_node: Node,
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> Vec<(&'a QMLProperty, &'a str)> {
    let Some(member) = resolve_expression(base_node, doc_state, registry) else {
        return Vec::new();
    };

    match member {
        ExpressionMember::LocalProperty { property, .. } => match &property.p_type {
            crate::qml::QMLPropertyType::BuiltIn(_) => Vec::new(),
            crate::qml::QMLPropertyType::Custom(type_name) => {
                collect_type_properties(type_name, doc_state, registry)
            }
        },
        ExpressionMember::GlobalProperty {
            property,
            component,
        } => {
            let QMLPropertyType::Custom(type_name) = &property.p_type else {
                return Vec::new();
            };

            let base_file = component
                .name()
                .split('.')
                .next()
                .unwrap_or(component.name());
            let composite_name = format!("{}.{}", base_file, type_name);

            if registry.component(&composite_name).is_some() {
                return collect_type_properties(&composite_name, doc_state, registry);
            }
            collect_type_properties(type_name, doc_state, registry)
        }
        ExpressionMember::Component(component) => {
            collect_type_properties(component.name(), doc_state, registry)
        }
        ExpressionMember::IdReference(instance) => {
            let mut properties = Vec::new();
            let mut seen = HashSet::new();
            let component_name = instance.type_name.as_str();

            instance.properties.iter().for_each(|prop| {
                if seen.insert(prop.name.as_str()) {
                    properties.push((prop, component_name));
                }
            });
            collect_type_properties(&instance.type_name, doc_state, registry)
                .iter()
                .for_each(|(prop, comp)| {
                    if seen.insert(prop.name.as_str()) {
                        properties.push((prop, comp));
                    }
                });

            properties
        }
    }
}

/// Collects properties belonging to the innermost enclosing `LocalInstance` at `byte_offset`.
pub fn collect_instance_properties_at<'a>(
    byte_offset: usize,
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> Vec<(&'a QMLProperty, &'a str)> {
    let mut path = Vec::new();
    get_instance_path(&doc_state.instances, byte_offset, &mut path);

    let Some(&instance) = path.last() else {
        return Vec::new();
    };

    let mut properties = Vec::new();
    let mut seen = HashSet::new();
    let component_name = instance.type_name.as_str();

    instance.properties.iter().for_each(|prop| {
        if seen.insert(prop.name.as_str()) {
            properties.push((prop, component_name));
        }
    });
    collect_type_properties(&instance.type_name, doc_state, registry)
        .iter()
        .for_each(|(prop, comp)| {
            if seen.insert(prop.name.as_str()) {
                properties.push((prop, comp));
            }
        });

    properties
}

/// Collects all unique properties accessible in the lexical scope chain at `byte_offset`.
pub fn collect_scope_properties<'a>(
    byte_offset: usize,
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> Vec<(&'a QMLProperty, &'a str)> {
    let mut path = Vec::new();
    get_instance_path(&doc_state.instances, byte_offset, &mut path);

    let mut properties = Vec::new();
    let mut seen = HashSet::new();

    path.into_iter().rev().for_each(|instance| {
        let component_name = instance.type_name.as_str();

        instance.properties.iter().for_each(|prop| {
            if seen.insert(prop.name.as_str()) {
                properties.push((prop, component_name));
            }
        });
        collect_type_properties(&instance.type_name, doc_state, registry)
            .iter()
            .for_each(|(prop, comp)| {
                if seen.insert(prop.name.as_str()) {
                    properties.push((prop, comp));
                }
            });
    });

    properties
}

/// Collects all `(id, type_name)` pairs defined in the current document.
pub fn collect_all_ids<'a>(doc_state: &'a DocState) -> Vec<(&'a str, &'a str)> {
    fn walk<'a>(instances: &'a [LocalInstance], ids: &mut Vec<(&'a str, &'a str)>) {
        instances.iter().for_each(|instance| {
            if let Some(id) = instance.id.as_deref() {
                ids.push((id, &instance.type_name));
            }
            walk(&instance.children, ids);
        });
    }

    let mut ids = Vec::new();
    walk(&doc_state.instances, &mut ids);
    ids
}
