use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, Documentation, MarkupContent,
    MarkupKind, Position,
};
use tree_sitter::{Node, Point};

use crate::{
    doc_state::DocState,
    handlers::{
        QT_DOCS,
        resolver::{
            collect_all_ids, collect_instance_properties_at, collect_scope_properties,
            resolve_member_properties,
        },
    },
    qml::{QMLProperty, QMLPropertyModifier},
    registry::ComponentRegistry,
    ts_kinds,
};

enum CompletionContext<'a> {
    MemberAccess {
        base_node: Node<'a>,
        is_lhs_binding: bool,
    },
    ComponentBody(usize),
    ValueExpression(usize),
    TopLevel,
}

pub fn completion(
    position: Position,
    doc_state: &DocState,
    registry: &ComponentRegistry,
) -> Option<CompletionResponse> {
    let byte_offset = doc_state.byte_offset_at(position);
    let context = detect_context(position, byte_offset, doc_state)?;

    let items = match context {
        CompletionContext::MemberAccess {
            base_node,
            is_lhs_binding,
        } => resolve_member_properties(base_node, doc_state, registry)
            .into_iter()
            .map(|(prop, owner)| format_property_item(&prop, &owner, is_lhs_binding))
            .collect(),
        CompletionContext::ComponentBody(offset) => {
            let prop_items = collect_instance_properties_at(offset, doc_state, registry)
                .into_iter()
                .map(|(prop, comp)| format_property_item(prop, comp, true));

            let comp_items = collect_component_items(doc_state, registry);

            prop_items.chain(comp_items).collect()
        }
        CompletionContext::ValueExpression(offset) => {
            let id_items = collect_all_ids(doc_state)
                .into_iter()
                .map(|(id, type_name)| CompletionItem {
                    label: id.to_string(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some(format!("id: {}", type_name)),
                    ..Default::default()
                })
                .chain(std::iter::once(CompletionItem {
                    label: "parent".to_string(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some("Parent instance".to_string()),
                    ..Default::default()
                }));

            let prop_items = collect_scope_properties(offset, doc_state, registry)
                .into_iter()
                .map(|(prop, comp)| format_property_item(prop, comp, false));

            let comp_items = collect_component_items(doc_state, registry);

            id_items.chain(prop_items).chain(comp_items).collect()
        }
        CompletionContext::TopLevel => collect_component_items(doc_state, registry).collect(),
    };

    Some(CompletionResponse::Array(items))
}

fn detect_context<'a>(
    position: Position,
    byte_offset: usize,
    doc_state: &'a DocState,
) -> Option<CompletionContext<'a>> {
    if let Some(base_node) = find_member_base_node(position, byte_offset, doc_state) {
        let is_lhs_binding = is_on_binding_lhs(base_node, byte_offset);
        return Some(CompletionContext::MemberAccess {
            base_node,
            is_lhs_binding,
        });
    }

    let probe_pos = Position {
        line: position.line,
        character: position.character.saturating_sub(1),
    };

    let Some(start_node) = doc_state
        .node_at(probe_pos)
        .or_else(|| doc_state.node_at(position))
    else {
        return Some(CompletionContext::TopLevel);
    };

    let mut current = Some(start_node);
    while let Some(node) = current {
        match node.kind() {
            ts_kinds::UI_BINDING => {
                if let Some(name_node) = node.child_by_field_name("name")
                    && byte_offset > name_node.end_byte()
                {
                    return Some(CompletionContext::ValueExpression(byte_offset));
                }
                return Some(CompletionContext::ComponentBody(byte_offset));
            }
            ts_kinds::UI_OBJECT_INITIALIZER => {
                return Some(CompletionContext::ComponentBody(byte_offset));
            }
            _ => current = node.parent(),
        }
    }

    if doc_state.instances.is_empty() {
        Some(CompletionContext::TopLevel)
    } else {
        Some(CompletionContext::ComponentBody(byte_offset))
    }
}

fn find_member_base_node<'a>(
    position: Position,
    byte_offset: usize,
    doc_state: &'a DocState,
) -> Option<Node<'a>> {
    let probe_point = Point::new(
        position.line as usize,
        position.character.saturating_sub(1) as usize,
    );
    let node = doc_state
        .tree
        .root_node()
        .named_descendant_for_point_range(probe_point, probe_point)?;

    if matches!(
        node.kind(),
        ts_kinds::PROPERTY_IDENTIFIER | ts_kinds::IDENTIFIER | ts_kinds::TYPE_IDENTIFIER
    ) && let Some(parent) = node.parent()
        && matches!(
            parent.kind(),
            ts_kinds::MEMBER_EXPRESSION | ts_kinds::NESTED_IDENTIFIER
        )
    {
        let object_node = parent
            .child_by_field_name("object")
            .or_else(|| parent.child(0))?;
        if object_node.id() != node.id() {
            return Some(object_node);
        }
    }

    if byte_offset >= 2 {
        let slice = doc_state.text.byte_slice(0..byte_offset).to_string();

        let before_dot = slice
            .trim_end_matches(|c: char| c.is_alphabetic() || c == '_')
            .strip_suffix('.')?
            .len();
        let target_byte = before_dot.saturating_sub(1);
        let line = doc_state.text.line_of_byte(target_byte);
        let col = target_byte - doc_state.text.byte_of_line(line);
        let point = Point::new(line, col);

        let base_candidate = doc_state
            .tree
            .root_node()
            .named_descendant_for_point_range(point, point)?;

        if let Some(parent) = base_candidate.parent()
            && parent.end_byte() <= before_dot
            && matches!(
                parent.kind(),
                ts_kinds::MEMBER_EXPRESSION | ts_kinds::NESTED_IDENTIFIER
            )
        {
            return Some(parent);
        }

        return Some(base_candidate);
    }

    None
}

fn format_property_item(
    property: &QMLProperty,
    comp_name: &str,
    append_colon: bool,
) -> CompletionItem {
    let documentation = QT_DOCS
        .get(comp_name)
        .and_then(|component_docs| component_docs.properties.get(&property.name))
        .map(|desc| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: desc.clone(),
            })
        });

    let should_append_colon = append_colon && property.modifier != QMLPropertyModifier::Readonly;
    CompletionItem {
        label: property.name.clone(),
        kind: Some(CompletionItemKind::PROPERTY),
        detail: Some(format!("{} ({})", property.p_type, comp_name)),
        insert_text: if should_append_colon {
            Some(format!("{}: ", property.name))
        } else {
            None
        },
        documentation,
        ..Default::default()
    }
}

fn collect_component_items<'a>(
    doc_state: &'a DocState,
    registry: &'a ComponentRegistry,
) -> impl Iterator<Item = CompletionItem> + 'a {
    let local_prefix = format!("{}.", doc_state.name);

    registry.components().filter_map(move |comp| {
        let name = comp.qml_name.as_deref()?;

        let display_name = if let Some(stripped) = name.strip_prefix(&local_prefix) {
            stripped
        } else if !name.contains('.') && name.chars().next().is_some_and(|c| c.is_uppercase()) {
            name
        } else {
            return None;
        };

        let documentation = QT_DOCS.get(display_name).map(|docs| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: docs.description.clone(),
            })
        });

        Some(CompletionItem {
            label: display_name.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            detail: comp.module.clone(),
            documentation,
            ..Default::default()
        })
    })
}

fn is_on_binding_lhs(base_node: Node, byte_offset: usize) -> bool {
    let mut current = Some(base_node);
    while let Some(node) = current {
        match node.kind() {
            ts_kinds::UI_BINDING => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    return byte_offset <= name_node.end_byte() + 1;
                }
                return false;
            }
            ts_kinds::UI_PROPERTY => return false,
            ts_kinds::UI_OBJECT_INITIALIZER => return true,
            _ => current = node.parent(),
        }
    }
    false
}
