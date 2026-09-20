use crop::Rope;
use tower_lsp::lsp_types::TextDocumentContentChangeEvent;

use crate::doc_state::doc_symbol::DocSymbol;

pub mod doc_symbol;
mod hover;
mod symbols;

pub struct DocState {
    pub(crate) text: Rope,
    pub(crate) tree: tree_sitter::Tree,
    pub symbols: Vec<DocSymbol>,
}

impl DocState {
    pub fn new(text: String, parser: &mut tree_sitter::Parser) -> Self {
        let tree = parser.parse(&text, None).unwrap();
        let rope = Rope::from(text);
        let symbols = DocState::build_symbols_tree(&rope, &tree);

        Self {
            text: rope,
            tree,
            symbols,
        }
    }

    pub fn update(
        &mut self,
        changes: Vec<TextDocumentContentChangeEvent>,
        parser: &mut tree_sitter::Parser,
    ) {
        let mut is_parsed = false;
        // This assumes only UTF-8 chars, but changes can contain UTF-16 characters
        // todo: rewrite for UTF-16
        changes.into_iter().for_each(|change| {
            let Some(range) = change.range else {
                self.tree = parser.parse(&change.text, None).unwrap();
                self.text = Rope::from(change.text);
                is_parsed = true;
                return;
            };
            let start_byte =
                self.text.byte_of_line(range.start.line as usize) + range.start.character as usize;
            let old_end_byte =
                self.text.byte_of_line(range.end.line as usize) + range.end.character as usize;
            let new_end_byte = start_byte + change.text.len();

            let lines_added_count = change.text.chars().filter(|&c| c == '\n').count();
            let new_end_row = range.start.line as usize + lines_added_count;
            let new_end_column = if lines_added_count > 0 {
                change.text.len() - change.text.rfind('\n').unwrap() - 1
            } else {
                range.start.character as usize + change.text.len()
            };

            let ts_edit = tree_sitter::InputEdit {
                start_byte,
                old_end_byte,
                new_end_byte,
                start_position: tree_sitter::Point {
                    row: range.start.line as usize,
                    column: range.start.character as usize,
                },
                old_end_position: tree_sitter::Point {
                    row: range.end.line as usize,
                    column: range.end.character as usize,
                },
                new_end_position: tree_sitter::Point {
                    row: new_end_row,
                    column: new_end_column,
                },
            };
            self.tree.edit(&ts_edit);

            self.text.replace(start_byte..old_end_byte, &change.text);
        });

        if !is_parsed {
            let text = self.text.to_string();
            let mut text_callback =
                |byte_offset: usize, _: tree_sitter::Point| &text.as_bytes()[byte_offset..];

            self.tree = parser
                .parse_with_options(&mut text_callback, Some(&self.tree), None)
                .unwrap();
        }

        self.symbols = DocState::build_symbols_tree(&self.text, &self.tree)
    }

    pub fn get_node_at(
        &self,
        position: tower_lsp::lsp_types::Position,
    ) -> Option<tree_sitter::Node<'_>> {
        let point = tree_sitter::Point::new(position.line as usize, position.character as usize);

        self.tree
            .root_node()
            .named_descendant_for_point_range(point, point)
    }

    // maybe name this better
    pub fn get_text_for_node(&self, node: &tree_sitter::Node) -> String {
        self.text
            .byte_slice(node.start_byte()..node.end_byte())
            .to_string()
    }
}
