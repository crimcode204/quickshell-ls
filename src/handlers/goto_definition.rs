use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Url};

use crate::{
    doc_state::DocState,
    handlers::resolver::{ExpressionMember, resolve_expression},
    registry::ComponentRegistry,
};

pub fn goto_definition(
    uri: &Url,
    position: Position,
    doc_state: &DocState,
    registry: &ComponentRegistry,
) -> Option<GotoDefinitionResponse> {
    let node = doc_state.node_at(position)?;

    let location = match resolve_expression(node, doc_state, registry)? {
        ExpressionMember::LocalProperty { property, .. } => Location {
            uri: uri.clone(),
            range: property.range?,
        },
        ExpressionMember::GlobalProperty {
            property,
            component,
        } => {
            let comp_loc = component.location.as_ref()?;
            Location {
                uri: comp_loc.uri.clone(),
                range: property.range.unwrap_or(comp_loc.range),
            }
        }
        ExpressionMember::Component(component) => component.location.clone()?,
        ExpressionMember::IdReference(instance) => Location {
            uri: uri.clone(),
            range: doc_state.byte_range_to_lsp_range(&instance.byte_range),
        },
    };

    Some(GotoDefinitionResponse::Scalar(location))
}
