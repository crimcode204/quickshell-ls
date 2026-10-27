use tower_lsp::{LspService, Server};

use crate::server::QuickshellLanguageServer;

mod doc_state;
mod handlers;
mod qml;
mod registry;
mod server;
mod ts_kinds;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| QuickshellLanguageServer::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
