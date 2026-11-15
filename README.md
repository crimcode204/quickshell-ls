# quickshell-ls

A [Language Server Protocol (LSP)](https://microsoft.github.io/language-server-protocol/) implementation for QML and the Quickshell framework.

## Installation

```sh
cargo build --release
```

If the qt documentation is outdated, update it with:
```sh
cargo run --bin qt_docs_scraper
```

## Capabilities

- [x] Hover
- [x] Completions
- [x] Document symbols
- [x] Workspace symbols
- [x] Go to definition
- [x] Document highlight
- [x] Rename
- [ ] Diagnostics
- [ ] Semantic tokens
- [ ] Formatting
- [ ] Signature help

## TODO

- [ ] QML value types scraping (QFont)
- [ ] Methods and signals scraping and resolution
