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

- [x] Document Synchronization
- [x] Hover (only for components for now)
- [ ] Go to definition
- [ ] Formatting
- [ ] Diagnostics
- [ ] Signature help
- [ ] Completions
- [ ] Rename
