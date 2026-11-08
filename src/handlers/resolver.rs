use dashmap::mapref::one::Ref;
use tree_sitter::Node;

use crate::{
    doc_state::{DocState, LocalInstance},
    qml::{QMLComponent, QMLProperty},
    registry::ComponentRegistry,
    ts_kinds,
};

pub enum ExpressionMember<'a> {
    LocalProperty {
        property: &'a QMLProperty,
        owner_type_name: &'a str,
    },
    GlobalProperty {
        component: Ref<'a, String, QMLComponent>,
        prop_name: String,
    },
    Component(Ref<'a, String, QMLComponent>),
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
) -> Option<ExpressionMember<'a>> {
    let mut path = Vec::new();
    get_instance_path(&doc_state.instances, byte_offset, &mut path);

    for instance in path.into_iter().rev() {
        if let Some(prop) = instance.properties.iter().find(|p| p.name == prop_name) {
            return Some(ExpressionMember::LocalProperty {
                property: prop,
                owner_type_name: &instance.type_name,
            });
        }

        if let Some(component) = resolve_component(&instance.type_name, doc_state, workspace) {
            if component_has_property(prop_name, &component, doc_state, workspace) {
                return Some(ExpressionMember::GlobalProperty {
                    component,
                    prop_name: prop_name.into(),
                });
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
) -> Option<ExpressionMember<'a>> {
    let component = resolve_component(type_name, doc_state, workspace)?;

    if component_has_property(prop_name, &component, doc_state, workspace) {
        return Some(ExpressionMember::GlobalProperty {
            component,
            prop_name: prop_name.into(),
        });
    }
    None
}

pub fn resolve_identifier<'a>(
    byte_offset: usize,
    name: &str,
    doc_state: &'a DocState,
    workspace: &'a ComponentRegistry,
) -> Option<ExpressionMember<'a>> {
    if let Some(instance) = resolve_id(name, doc_state) {
        return Some(ExpressionMember::IdReference(instance));
    }

    if let Some(resolved) = resolve_property_in_scope(byte_offset, name, doc_state, workspace) {
        return Some(resolved);
    }

    if let Some(component) = resolve_component(name, doc_state, workspace) {
        return Some(ExpressionMember::Component(component));
    }

    None
}

pub fn resolve_expression<'a>(
    node: tree_sitter::Node<'_>,
    doc_state: &'a DocState,
    workspace: &'a ComponentRegistry,
) -> Option<ExpressionMember<'a>> {
    match node.kind() {
        ts_kinds::IDENTIFIER | ts_kinds::TYPE_IDENTIFIER => {
            let name = doc_state.node_text(&node);
            resolve_identifier(node.start_byte(), &name, doc_state, workspace)
        }
        ts_kinds::PROPERTY_IDENTIFIER => {
            if let Some(parent) = node.parent() {
                let object_node = parent.child_by_field_name("object")?;
                let prop_name = doc_state.node_text(&node);
                return resolve_member_access(object_node, &prop_name, doc_state, workspace);
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
) -> Option<ExpressionMember<'a>> {
    match resolve_expression(base_node, doc_state, workspace)? {
        ExpressionMember::LocalProperty { property, .. } => {
            let type_name = match &property.p_type {
                crate::qml::QMLPropertyType::BuiltIn(_) => return None,
                crate::qml::QMLPropertyType::Custom(custom_type) => custom_type,
            };
            resolve_property_on_type(type_name, prop_name, doc_state, workspace)
        }
        ExpressionMember::GlobalProperty {
            component,
            prop_name: base_prop_name,
        } => {
            let mut current_proto = Some(component.cpp_name.clone());
            while let Some(proto_name) = current_proto {
                let Some(parent_comp) = resolve_prototype(&proto_name, doc_state, workspace) else {
                    break;
                };

                if let Some(prop) = parent_comp
                    .properties
                    .iter()
                    .find(|p| p.name == base_prop_name)
                {
                    let type_name = match &prop.p_type {
                        crate::qml::QMLPropertyType::BuiltIn(_) => return None,
                        crate::qml::QMLPropertyType::Custom(custom_type) => custom_type,
                    };

                    let base_file_name = parent_comp
                        .name()
                        .split('.')
                        .next()
                        .unwrap_or(parent_comp.name());
                    let composite_name = format!("{}.{}", base_file_name, type_name);

                    if workspace.component(&composite_name).is_some() {
                        return resolve_property_on_type(
                            &composite_name,
                            prop_name,
                            doc_state,
                            workspace,
                        );
                    }

                    return resolve_property_on_type(type_name, prop_name, doc_state, workspace);
                }
                current_proto = parent_comp.prototype.clone();
            }
            None
        }
        ExpressionMember::Component(component) => {
            resolve_property_on_type(component.name(), prop_name, doc_state, workspace)
        }
        ExpressionMember::IdReference(instance) => {
            if let Some(prop) = instance.properties.iter().find(|p| p.name == prop_name) {
                return Some(ExpressionMember::LocalProperty {
                    property: prop,
                    owner_type_name: &instance.type_name,
                });
            }
            resolve_property_on_type(&instance.type_name, prop_name, doc_state, workspace)
        }
    }
}
