use tower_lsp::lsp_types::{Location, SymbolInformation, SymbolKind};

use crate::registry::ComponentRegistry;

#[allow(deprecated)]
pub fn workspace_symbol(
    query: &str,
    registry: &ComponentRegistry,
) -> Option<Vec<SymbolInformation>> {
    let query_lower = query.to_lowercase();
    let mut symbols = Vec::new();

    registry.components().for_each(|comp| {
        let Some(location) = &comp.location else {
            return;
        };
        let comp_name = comp.name();

        if comp_name.to_lowercase().contains(&query_lower) {
            symbols.push(SymbolInformation {
                name: comp_name.to_string(),
                kind: SymbolKind::CLASS,
                tags: None,
                deprecated: None,
                location: location.clone(),
                container_name: comp.module.clone(),
            });
        }

        comp.properties.iter().for_each(|prop| {
            if prop.name.to_lowercase().contains(&query_lower) {
                symbols.push(SymbolInformation {
                    name: prop.name.clone(),
                    kind: SymbolKind::PROPERTY,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: location.uri.clone(),
                        range: prop.range.unwrap_or(location.range),
                    },
                    container_name: Some(comp_name.to_string()),
                });
            }
        });
    });

    Some(symbols)
}
