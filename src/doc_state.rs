use std::{fmt::Display, result, sync::LazyLock};

use crop::Rope;
use tower_lsp::lsp_types::{Position, Range, TextDocumentContentChangeEvent};
use tree_sitter::{Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};

pub static SYMBOLS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let language = tree_sitter_qmljs::LANGUAGE.into();
    let query_string = include_str!("../queries/symbols.scm");

    Query::new(&language, query_string).expect("Failed to compile symbols query")
});

pub static LOCALS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let language = tree_sitter_qmljs::LANGUAGE.into();
    let query_string = include_str!("../queries/locals.scm");

    Query::new(&language, query_string).expect("Failed to compile locals query")
});

pub struct DocumentState {
    text: Rope,
    tree: Tree,
    symbols: Vec<Symbol>,
}

struct Symbol {
    name: String,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
    detail: Option<String>,
    documentation: Option<String>,
    children: Vec<Symbol>,
}

#[derive(Clone, PartialEq)]
enum SymbolKind {
    Component,
    Property,
    Signal,
    Id,
}

impl Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind_string = match self {
            SymbolKind::Component => "Component".to_string(),
            SymbolKind::Property => "Property".to_string(),
            SymbolKind::Signal => "Signal".to_string(),
            SymbolKind::Id => "Id".to_string(),
        };
        write!(f, "{kind_string}")
    }
}

impl DocumentState {
    pub fn new(text: String, parser: &mut Parser) -> Self {
        let tree = parser.parse(&text, None).unwrap();
        let rope = Rope::from(text);
        let mut symbols = Vec::new();

        let text_provider = |node: Node| {
            let byte_range = node.byte_range();
            rope.byte_slice(byte_range)
                .chunks()
                .map(|chunk| chunk.as_bytes())
        };
        let mut parent_stack: Vec<(Symbol, usize)> = Vec::new();

        let mut cursor = QueryCursor::new();
        cursor
            .matches(&SYMBOLS_QUERY, tree.root_node(), text_provider)
            .for_each(|query_match| {
                query_match.captures().iter().for_each(|capture| {
                    let node = capture.node;
                    let block_node = node.parent().unwrap_or(node);

                    let name: String = rope.byte_slice(node.byte_range()).to_string();
                    let kind = match SYMBOLS_QUERY.capture_names()[capture.index as usize] {
                        "property" | "property.name" => SymbolKind::Property,
                        "component.name" => SymbolKind::Component,
                        "variable.parameter" => SymbolKind::Id,
                        "function.signal" => SymbolKind::Signal,
                        _ => return,
                    };
                    let selection_range = Range {
                        start: ts_point_to_pos(node.start_position()),
                        end: ts_point_to_pos(node.end_position()),
                    };
                    let range = Range {
                        start: ts_point_to_pos(block_node.start_position()),
                        end: ts_point_to_pos(block_node.end_position()),
                    };

                    let symbol = Symbol {
                        name,
                        kind: kind.clone(),
                        range,
                        selection_range,
                        detail: None,
                        documentation: None,
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

                    if kind == SymbolKind::Component {
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

        Self {
            text: rope,
            tree,
            symbols,
        }
    }

    pub fn update(
        &mut self,
        _changes: Vec<TextDocumentContentChangeEvent>,
        _parser: &mut tree_sitter::Parser,
    ) {
    }

    pub fn print_symbols(&self) -> String {
        let mut result = String::new();
        self.symbols
            .iter()
            .for_each(|symbol| result = format!("{}{}", result, print_symbol(symbol, 0)));
        result
    }
}

fn print_symbol(symbol: &Symbol, depth: usize) -> String {
    let mut result: String = format!("{}{}: {}\n", "  ".repeat(depth), symbol.kind, symbol.name);

    symbol
        .children
        .iter()
        .for_each(|child| result = format!("{}{}", result, print_symbol(child, depth + 1)));

    result
}

fn ts_point_to_pos(point: Point) -> Position {
    Position::new(point.row as u32, point.column as u32)
}
