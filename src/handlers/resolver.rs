use dashmap::mapref::one::Ref;

use crate::{
    doc_state::{DocState, LocalInstance},
    qml::{QMLComponent, QMLProperty},
    registry::ComponentRegistry,
};

pub enum ResolvedProperty<'a> {
    Local {
        property: &'a QMLProperty,
        owner_type_name: &'a str,
    },
    Global(Ref<'a, String, QMLComponent>),
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
