use std::{fmt::Display, sync::LazyLock};

use crop::Rope;
use tower_lsp::lsp_types::{
    DocumentSymbol, Position, Range, SymbolKind, TextDocumentContentChangeEvent,
};
use tree_sitter::{InputEdit, Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};

pub static SYMBOLS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let language = tree_sitter_qmljs::LANGUAGE.into();
    let query_string = include_str!("../queries/symbols.scm");

    Query::new(&language, query_string).expect("Failed to compile symbols query")
});

pub static _LOCALS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let language = tree_sitter_qmljs::LANGUAGE.into();
    let query_string = include_str!("../queries/locals.scm");

    Query::new(&language, query_string).expect("Failed to compile locals query")
});

pub struct DocState {
    text: Rope,
    tree: Tree,
    pub symbols: Vec<DocSymbol>,
}

pub struct DocSymbol {
    name: String,
    kind: DocSymbolKind,
    range: Range,
    selection_range: Range,
    detail: Option<String>,
    _documentation: Option<String>,
    children: Vec<DocSymbol>,
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

#[derive(Clone, PartialEq)]
enum DocSymbolKind {
    Component,
    Property,
    Signal,
    Id,
}

impl Display for DocSymbolKind {
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

impl DocState {
    pub fn new(text: String, parser: &mut Parser) -> Self {
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

            let ts_edit = InputEdit {
                start_byte,
                old_end_byte,
                new_end_byte,
                start_position: Point {
                    row: range.start.line as usize,
                    column: range.start.character as usize,
                },
                old_end_position: Point {
                    row: range.end.line as usize,
                    column: range.end.character as usize,
                },
                new_end_position: Point {
                    row: new_end_row,
                    column: new_end_column,
                },
            };
            self.tree.edit(&ts_edit);

            self.text.replace(start_byte..old_end_byte, &change.text);
        });

        if !is_parsed {
            let text = self.text.to_string();
            let mut text_callback = |byte_offset: usize, _: Point| &text.as_bytes()[byte_offset..];

            self.tree = parser
                .parse_with_options(&mut text_callback, Some(&self.tree), None)
                .unwrap();
        }

        self.symbols = DocState::build_symbols_tree(&self.text, &self.tree)
    }

    pub fn print_symbols(&self) -> String {
        let mut result = String::new();
        self.symbols
            .iter()
            .for_each(|symbol| result = format!("{}{}", result, print_symbol(symbol, 0)));
        result
    }

    fn build_symbols_tree(text: &Rope, tree: &Tree) -> Vec<DocSymbol> {
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
                    let selection_range = Range {
                        start: ts_point_to_pos(node.start_position()),
                        end: ts_point_to_pos(node.end_position()),
                    };
                    let range = Range {
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

fn print_symbol(symbol: &DocSymbol, depth: usize) -> String {
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
