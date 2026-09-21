use std::sync::LazyLock;

use crop::Rope;
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator, Tree};

use crate::doc_state::{
    DocState,
    doc_symbol::{DocSymbol, DocSymbolKind},
};

static SYMBOLS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let language = tree_sitter_qmljs::LANGUAGE.into();
    let query_string = include_str!("../../queries/symbols.scm");

    Query::new(&language, query_string).expect("Failed to compile symbols query")
});

static _LOCALS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let language = tree_sitter_qmljs::LANGUAGE.into();
    let query_string = include_str!("../../queries/locals.scm");

    Query::new(&language, query_string).expect("Failed to compile locals query")
});

impl DocState {
    /// Returns a symbol with the specified name if it exists.
    pub fn find_symbol_by_name(&self, name: &str) -> Option<&DocSymbol> {
        fn search<'a>(symbols: &'a [DocSymbol], target: &str) -> Option<&'a DocSymbol> {
            for symbol in symbols {
                if symbol.name == target {
                    return Some(symbol);
                }
                if let Some(child) = search(&symbol.children, target) {
                    return Some(child);
                }
            }
            None
        }

        search(&self.symbols, name)
    }

    /// Creates a symbol tree based on a given text sequence and its AST.
    pub(crate) fn build_symbols_tree(text: &Rope, tree: &Tree) -> Vec<DocSymbol> {
        let mut symbols = Vec::new();

        let text_provider = |node: Node| {
            let byte_range = node.byte_range();
            text.byte_slice(byte_range)
                .chunks()
                .map(|chunk| chunk.as_bytes())
        };
        let mut parent_stack: Vec<(DocSymbol, usize)> = Vec::new();

        let mut cursor = QueryCursor::new();
        cursor
            .matches(&SYMBOLS_QUERY, tree.root_node(), text_provider)
            .for_each(|query_match| {
                query_match.captures().iter().for_each(|capture| {
                    let node = capture.node;
                    let block_node = node.parent().unwrap_or(node);

                    let name: String = text.byte_slice(node.byte_range()).to_string();
                    let kind = match SYMBOLS_QUERY.capture_names()[capture.index as usize] {
                        "property" | "property.name" => DocSymbolKind::Property,
                        "component.name" => DocSymbolKind::Component,
                        "variable.parameter" => DocSymbolKind::Id,
                        "function.signal" => DocSymbolKind::Signal,
                        _ => return,
                    };
                    let selection_range = tower_lsp::lsp_types::Range {
                        start: ts_point_to_pos(node.start_position()),
                        end: ts_point_to_pos(node.end_position()),
                    };
                    let range = tower_lsp::lsp_types::Range {
                        start: ts_point_to_pos(block_node.start_position()),
                        end: ts_point_to_pos(block_node.end_position()),
                    };

                    let symbol = DocSymbol {
                        name,
                        kind: kind.clone(),
                        range,
                        selection_range,
                        detail: None,
                        _documentation: None,
                        children: Vec::new(),
                    };

                    while let Some(last) = parent_stack.last()
                        && last.1 < node.byte_range().start
                    {
                        let last_symbol = parent_stack.pop().unwrap().0;
                        if let Some((parent, _)) = parent_stack.last_mut() {
                            parent.children.push(last_symbol);
                        } else {
                            symbols.push(last_symbol);
                        }
                    }

                    if kind == DocSymbolKind::Component {
                        parent_stack.push((symbol, block_node.byte_range().end))
                    } else if let Some((parent, _)) = parent_stack.last_mut() {
                        parent.children.push(symbol);
                    } else {
                        symbols.push(symbol);
                    }
                })
            });

        while let Some((symbol, _)) = parent_stack.pop() {
            if let Some((parent, _)) = parent_stack.last_mut() {
                parent.children.push(symbol);
            } else {
                symbols.push(symbol);
            }
        }

        symbols
    }
}

/// Converts a [`tree_sitter::Point`] to a [`tower_lsp::lsp_types::Position`].
fn ts_point_to_pos(point: tree_sitter::Point) -> tower_lsp::lsp_types::Position {
    tower_lsp::lsp_types::Position::new(point.row as u32, point.column as u32)
}
