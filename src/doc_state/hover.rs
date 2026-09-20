use tower_lsp::lsp_types::HoverParams;

use crate::doc_state::DocState;

impl DocState {
    pub fn get_hover_info(&self, params: HoverParams) {}
}
