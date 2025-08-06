//! Server that accepts connections and echoes data back.

use iroh::{
    Endpoint, Watcher,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler, Router},
};
use miette::{IntoDiagnostic, Result};

/// Each protocol is identified by its ALPN string.
const ALPN: &[u8] = b"room_101/secrets/0";

#[tokio::main]
async fn main() -> Result<()> {
    let router = start_accept_side().await?;
    let node_addr = router.endpoint().node_addr().initialized().await;

    println!("Server started!");
    println!("Node ID: {}", node_addr.node_id);
    println!("Full address: {node_addr:#?}");
    println!("Waiting for connections...");

    // Keep the server running
    tokio::signal::ctrl_c().await.into_diagnostic()?;
    println!("\nShutting down server...");

    // This makes sure the endpoint in the router is closed properly and connections close gracefully
    router.shutdown().await.into_diagnostic()?;

    Ok(())
}

async fn start_accept_side() -> Result<Router> {
    let endpoint = Endpoint::builder()
        .discovery_n0()
        .bind()
        .await
        .into_diagnostic()?;

    // Build our protocol handler and add our protocol, identified by its ALPN, and spawn the node.
    let router = Router::builder(endpoint).accept(ALPN, Echo).spawn();

    Ok(router)
}

#[derive(Debug, Clone)]
struct Echo;

impl ProtocolHandler for Echo {
    /// The `accept` method is called for each incoming connection for our ALPN.
    ///
    /// The returned future runs on a newly spawned tokio task, so it can run as long as
    /// the connection lasts.
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        // We can get the remote's node id from the connection.
        let node_id = connection.remote_node_id()?;
        println!("Accepted connection from {node_id}");

        // Our protocol is a simple request-response protocol, so we expect the
        // connecting peer to open a single bi-directional stream.
        let (mut send, mut recv) = connection.accept_bi().await?;

        // Echo any bytes received back directly.
        // This will keep copying until the sender signals the end of data on the stream.
        let bytes_sent = tokio::io::copy(&mut recv, &mut send).await?;
        println!("Echoed {bytes_sent} byte(s) back to {node_id}");

        // By calling `finish` on the send stream we signal that we will not send anything
        // further, which makes the receive stream on the other end terminate.
        send.finish()?;

        // Wait until the remote closes the connection, which it does once it
        // received the response.
        connection.closed().await;

        Ok(())
    }
}