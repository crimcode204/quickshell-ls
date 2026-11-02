use dashmap::mapref::one::Ref;
use tree_sitter::Node;

use crate::{
    doc_state::{DocState, LocalInstance},
    qml::{QMLComponent, QMLProperty},
    registry::ComponentRegistry,
    ts_kinds,
};

pub enum ResolvedProperty<'a> {
    Local {
        property: &'a QMLProperty,
        owner_type_name: &'a str,
    },
    Global(Ref<'a, String, QMLComponent>),
    IdReference(&'a LocalInstance),
}

pub fn resolve_component<'a>(
    name: &str,
    doc_state: &DocState,
    workspace: &'a ComponentRegistry,
) -> Option<Ref<'a, String, QMLComponent>> {
    let local_name = format!("{}.{}", doc_state.name, name);
    match workspace.component(&local_name) {
        Some(comp) => Some(comp),
        None => workspace.component(name),
    }
}

pub fn resolve_prototype<'a>(
    proto_name: &str,
    doc_state: &DocState,
    workspace: &'a ComponentRegistry,
) -> Option<Ref<'a, String, QMLComponent>> {
    let local_name = format!("{}.{}", doc_state.name, proto_name);
    match workspace.component(&local_name) {
        Some(comp) => Some(comp),
        None => workspace.resolve_prototype(proto_name),
    }
}

pub fn component_has_property(
    prop_name: &str,
    component: &QMLComponent,
    doc_state: &DocState,
    workspace: &ComponentRegistry,
) -> bool {
    let mut current_proto = Some(component.cpp_name.clone());

    while let Some(proto_name) = current_proto {
        let Some(parent_comp) = resolve_prototype(&proto_name, doc_state, workspace) else {
            break;
        };
        if parent_comp.properties.iter().any(|p| p.name == prop_name) {
            return true;
        }
        current_proto = parent_comp.prototype.clone();
    }
    false
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
    workspace: &'a ComponentRegistry,
) -> Option<ResolvedProperty<'a>> {
    let mut path = Vec::new();
    get_instance_path(&doc_state.instances, byte_offset, &mut path);

    for instance in path.into_iter().rev() {
        if let Some(prop) = instance.properties.iter().find(|p| p.name == prop_name) {
            return Some(ResolvedProperty::Local {
                property: prop,
                owner_type_name: &instance.type_name,
            });
        }

        if let Some(component) = resolve_component(&instance.type_name, doc_state, workspace) {
            if component_has_property(prop_name, &component, doc_state, workspace) {
                return Some(ResolvedProperty::Global(component));
            }
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
    workspace: &'a ComponentRegistry,
) -> Option<ResolvedProperty<'a>> {
    let component = resolve_component(type_name, doc_state, workspace)?;

    if component_has_property(prop_name, &component, doc_state, workspace) {
        return Some(ResolvedProperty::Global(component));
    }
    None
}

pub fn resolve_identifier<'a>(
    byte_offset: usize,
    name: &str,
    doc_state: &'a DocState,
    workspace: &'a ComponentRegistry,
) -> Option<ResolvedProperty<'a>> {
    if let Some(instance) = resolve_id(name, doc_state) {
        return Some(ResolvedProperty::IdReference(instance));
    }

    if let Some(resolved) = resolve_property_in_scope(byte_offset, name, doc_state, workspace) {
        return Some(resolved);
    }
    None
}

pub fn resolve_expression<'a>(
    node: tree_sitter::Node<'_>,
    doc_state: &'a DocState,
    workspace: &'a ComponentRegistry,
) -> Option<ResolvedProperty<'a>> {
    match node.kind() {
        ts_kinds::IDENTIFIER => {
            let name = doc_state.node_text(&node);
            resolve_identifier(node.start_byte(), &name, doc_state, workspace)
        }
        ts_kinds::MEMBER_EXPRESSION | ts_kinds::NESTED_IDENTIFIER => {
            let object_node = node
                .child_by_field_name("object")
                .or_else(|| node.child(0))?;
            let property_node = node
                .child_by_field_name("property")
                .or_else(|| node.child(2))?;

            let property_name = doc_state.node_text(&property_node);

            resolve_member_access(object_node, &property_name, doc_state, workspace)
        }
        _ => None,
    }
}

pub fn resolve_member_access<'a>(
    base_node: Node,
    prop_name: &str,
    doc_state: &'a DocState,
    workspace: &'a ComponentRegistry,
) -> Option<ResolvedProperty<'a>> {
    match resolve_expression(base_node, doc_state, workspace)? {
        ResolvedProperty::Local { property, .. } => {
            let type_name = match &property.p_type {
                crate::qml::QMLPropertyType::BuiltIn(_) => return None,
                crate::qml::QMLPropertyType::Custom(custom_type) => custom_type,
            };
            resolve_property_on_type(type_name, prop_name, doc_state, workspace)
        }
        ResolvedProperty::Global(component) => {
            resolve_property_on_type(component.name(), prop_name, doc_state, workspace)
        }
        ResolvedProperty::IdReference(instance) => {
            if let Some(prop) = instance.properties.iter().find(|p| p.name == prop_name) {
                return Some(ResolvedProperty::Local {
                    property: prop,
                    owner_type_name: &instance.type_name,
                });
            }

            resolve_property_on_type(&instance.type_name, prop_name, doc_state, workspace)
        }
    }
}
