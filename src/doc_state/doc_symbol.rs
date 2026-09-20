use tower_lsp::lsp_types::{DocumentSymbol, SymbolKind};

#[derive(Clone, PartialEq)]
pub enum DocSymbolKind {
    Component,
    Property,
    Signal,
    Id,
}

impl std::fmt::Display for DocSymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind_string = match self {
            DocSymbolKind::Component => "Component".to_string(),
            DocSymbolKind::Property => "Property".to_string(),
            DocSymbolKind::Signal => "Signal".to_string(),
            DocSymbolKind::Id => "Id".to_string(),
        };
        write!(f, "{kind_string}")
    }
}

pub struct DocSymbol {
    pub(crate) name: String,
    pub(crate) kind: DocSymbolKind,
    pub(crate) range: tower_lsp::lsp_types::Range,
    pub(crate) selection_range: tower_lsp::lsp_types::Range,
    pub(crate) detail: Option<String>,
    pub(crate) _documentation: Option<String>,
    pub(crate) children: Vec<DocSymbol>,
}

impl From<&DocSymbol> for DocumentSymbol {
    fn from(value: &DocSymbol) -> Self {
        #[allow(deprecated)]
        DocumentSymbol {
            name: value.name.clone(),
            detail: value.detail.clone(),
            kind: match value.kind {
                DocSymbolKind::Component => SymbolKind::CLASS,
                DocSymbolKind::Property => SymbolKind::PROPERTY,
                DocSymbolKind::Signal => SymbolKind::EVENT,
                DocSymbolKind::Id => SymbolKind::VARIABLE,
            },
            tags: None,
            deprecated: None,
            range: value.range,
            selection_range: value.selection_range,
            children: if value.children.is_empty() {
                None
            } else {
                Some(value.children.iter().map(Self::from).collect())
            },
        }
    }
}
