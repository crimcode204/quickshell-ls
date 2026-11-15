mod completion;
mod document_symbol;
mod goto_definition;
mod highlight;
mod hover;
mod resolver;
mod workspace_symbol;

use serde::Deserialize;
use std::{collections::HashMap, sync::LazyLock};

pub use completion::completion;
pub use document_symbol::document_symbol;
pub use goto_definition::goto_definition;
pub use highlight::document_highlight;
pub use hover::hover;
pub use workspace_symbol::workspace_symbol;

#[derive(Deserialize)]
struct QtComponentDocs {
    description: String,
    properties: HashMap<String, String>,
}

static QT_DOCS: LazyLock<HashMap<String, QtComponentDocs>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/qt_docs.json")).unwrap_or_default()
});
