mod completion;
mod hover;
mod resolver;

use serde::Deserialize;
use std::{collections::HashMap, sync::LazyLock};

pub use completion::completion;
pub use hover::hover;

#[derive(Deserialize)]
struct QtComponentDocs {
    description: String,
    properties: HashMap<String, String>,
}

static QT_DOCS: LazyLock<HashMap<String, QtComponentDocs>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/qt_docs.json")).unwrap_or_default()
});
