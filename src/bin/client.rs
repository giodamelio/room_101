//! Client that connects to a server and sends data to be echoed back.

use iroh::{Endpoint, NodeAddr, NodeId};
use miette::{IntoDiagnostic, Result, bail};
use std::env;
use std::str::FromStr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Each protocol is identified by its ALPN string.
const ALPN: &[u8] = b"room_101/secrets/0";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        bail!("Usage: {} <node_id>", args[0]);
    }

    let node_id_str = &args[1];
    let node_id = NodeId::from_str(node_id_str)
        .map_err(|e| miette::miette!("Invalid node ID '{}': {}", node_id_str, e))?;

    println!("Connecting to node: {node_id}");

    // Create a simple NodeAddr with just the node_id
    // The discovery mechanism will find the actual network addresses
    let node_addr = NodeAddr::new(node_id);

    connect_to_server(node_addr).await?;

    Ok(())
}

async fn connect_to_server(addr: NodeAddr) -> Result<()> {
    let endpoint = Endpoint::builder()
        .discovery_n0()
        .bind()
        .await
        .into_diagnostic()?;

    println!("Attempting to connect...");

    // Open a connection to the accepting node
    let conn = endpoint.connect(addr, ALPN).await.into_diagnostic()?;

    println!("Connected! Opening stream...");

    // Open a bidirectional QUIC stream
    let (mut send, mut recv) = conn.open_bi().await.into_diagnostic()?;

    // Send some data to be echoed
    let message = b"Hello, world from client!";
    println!("Sending: {}", String::from_utf8_lossy(message));
    send.write_all(message).await.into_diagnostic()?;

    // Signal the end of data for this particular stream
    send.finish().into_diagnostic()?;

    // Receive the echo, but limit reading up to maximum 1000 bytes
    let response = recv.read_to_end(1000).await.into_diagnostic()?;
    println!("Received echo: {}", String::from_utf8_lossy(&response));

    if response == message {
        println!("✅ Echo test successful!");
    } else {
        println!("❌ Echo mismatch!");
    }

    // Explicitly close the whole connection.
    conn.close(0u32.into(), b"bye!");

    // The above call only queues a close message to be sent (see how it's not async!).
    // We need to actually call this to make sure this message is sent out.
    endpoint.close().await;

    println!("Connection closed.");
    Ok(())
}