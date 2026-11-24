use std::collections::{HashMap, HashSet};

use dashmap::DashMap;
use tower_lsp::lsp_types::{Position, PrepareRenameResponse, TextEdit, Url, WorkspaceEdit};
use tree_sitter::Node;

use crate::{
    doc_state::DocState,
    handlers::resolver::{
        ExpressionMember, find_all_references_in_doc, find_member_references_in_doc,
        resolve_expression,
    },
    qml::node_to_lsp_range,
    registry::ComponentRegistry,
};

pub enum RegistrySync {
    Property {
        comp_name: String,
        old_name: String,
        new_name: String,
    },
    InlineComponent {
        old_name: String,
        new_name: String,
    },
}

pub fn prepare_rename(
    position: Position,
    doc_state: &DocState,
    registry: &ComponentRegistry,
) -> Option<PrepareRenameResponse> {
    let node = doc_state.node_at(position)?;
    let member = resolve_expression(node, doc_state, registry)?;
    let text = doc_state.node_text(&node);

    if !is_renameable(&text, &member) {
        return None;
    }

    Some(PrepareRenameResponse::RangeWithPlaceholder {
        range: node_to_lsp_range(node),
        placeholder: text,
    })
}

pub fn rename(
    uri: &Url,
    position: Position,
    new_name: String,
    doc_state: &DocState,
    documents: &DashMap<Url, DocState>,
    registry: &ComponentRegistry,
) -> Option<(WorkspaceEdit, Option<RegistrySync>)> {
    let node = doc_state.node_at(position)?;
    let old_name = doc_state.node_text(&node);
    let (member, current_doc_refs) = find_all_references_in_doc(node, doc_state, &registry)?;

    if !is_renameable(&old_name, &member) {
        return None;
    }

    if is_local_member(&member) {
        let mut changes = HashMap::new();
        changes.insert(uri.clone(), node_to_text_edits(current_doc_refs, &new_name));

        let edit = WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        };

        return Some((edit, None));
    }

    let sync =
        match &member {
            ExpressionMember::LocalProperty { instance, .. } => instance
                .registered_name
                .as_ref()
                .map(|comp_name| RegistrySync::Property {
                    comp_name: comp_name.clone(),
                    old_name: old_name.clone(),
                    new_name: new_name.clone(),
                }),
            ExpressionMember::GlobalProperty { component, .. } => Some(RegistrySync::Property {
                comp_name: component.name().to_string(),
                old_name: old_name.clone(),
                new_name: new_name.clone(),
            }),
            ExpressionMember::Component(component) => Some(RegistrySync::InlineComponent {
                old_name: component.name().to_string(),
                new_name: new_name.clone(),
            }),
            ExpressionMember::IdReference(_) => None,
        };

    let changes = collect_project_wide_edits(
        &member,
        &old_name,
        &new_name,
        Some((uri, &current_doc_refs)),
        documents,
        registry,
    );

    let edit = WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    };

    Some((edit, sync))
}

pub fn will_rename_files(
    renames: &[(String, String)],
    documents: &DashMap<Url, DocState>,
    registry: &ComponentRegistry,
) -> Option<WorkspaceEdit> {
    let mut all_changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

    renames.into_iter().for_each(|(old_name, new_name)| {
        let Some(comp) = registry.component(old_name) else {
            return;
        };
        let member = ExpressionMember::Component(comp);

        collect_project_wide_edits(&member, old_name, new_name, None, documents, registry)
            .into_iter()
            .for_each(|(uri, edits)| all_changes.entry(uri).or_default().extend(edits));
    });

    Some(WorkspaceEdit {
        changes: Some(all_changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn node_to_text_edits<'a>(
    nodes: impl IntoIterator<Item = Node<'a>>,
    new_name: &str,
) -> Vec<TextEdit> {
    nodes
        .into_iter()
        .map(|n| TextEdit {
            range: node_to_lsp_range(n),
            new_text: new_name.to_string(),
        })
        .collect()
}

fn collect_project_wide_edits(
    member: &ExpressionMember,
    old_name: &str,
    new_name: &str,
    current_doc: Option<(&Url, &[Node])>,
    documents: &DashMap<Url, DocState>,
    registry: &ComponentRegistry,
) -> HashMap<Url, Vec<TextEdit>> {
    let mut project_urls: HashSet<Url> = registry
        .components()
        .filter_map(|comp| comp.location.as_ref().map(|loc| loc.uri.clone()))
        .collect();

    if let Some((uri, _)) = current_doc {
        project_urls.insert(uri.clone());
    }
    documents.iter().for_each(|entry| {
        project_urls.insert(entry.key().clone());
    });

    let mut changes = HashMap::new();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_qmljs::LANGUAGE.into())
        .unwrap();

    for file_url in project_urls {
        let edits: Vec<TextEdit> = if let Some((current_uri, current_refs)) = current_doc
            && &file_url == current_uri
        {
            node_to_text_edits(current_refs.iter().copied(), new_name)
        } else if let Some(open_doc) = documents.get(&file_url) {
            let refs = find_member_references_in_doc(&member, &old_name, &open_doc, registry);
            node_to_text_edits(refs, new_name)
        } else if let Ok(path) = file_url.to_file_path()
            && let Ok(text) = std::fs::read_to_string(&path)
            && text.contains(&old_name)
        {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let temp_doc = DocState::new(stem, text, &mut parser);
            let refs = find_member_references_in_doc(&member, &old_name, &temp_doc, registry);
            node_to_text_edits(refs, new_name)
        } else {
            Vec::new()
        };

        if !edits.is_empty() {
            changes.insert(file_url, edits);
        }
    }

    changes
}

fn is_renameable(node_text: &str, member: &ExpressionMember) -> bool {
    match member {
        ExpressionMember::LocalProperty { .. } => true,
        ExpressionMember::GlobalProperty { component, .. } => component.location.is_some(),
        ExpressionMember::Component(component) => {
            component.location.is_some() && component.name().contains('.')
        }
        ExpressionMember::IdReference(_) => node_text != "parent",
    }
}

/// Checks whether a member is defined in a certain file
fn is_local_member(member: &ExpressionMember) -> bool {
    match member {
        ExpressionMember::LocalProperty { instance, .. } => instance.registered_name.is_none(),
        ExpressionMember::GlobalProperty { .. } | ExpressionMember::Component(_) => false,
        ExpressionMember::IdReference(_) => true,
    }
}
