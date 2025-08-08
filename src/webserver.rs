use anyhow::{Context, Result};
use iroh::NodeId;
use maud::{DOCTYPE, Markup, html};
use poem::{
    EndpointExt, Route, Server, get, handler,
    listener::TcpListener,
    web::{Data, Form},
};
use serde::Deserialize;
use tokio::sync::broadcast;
use tracing::info;

use crate::db::{DB, Peer};

fn layout(content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        script src="https://cdn.jsdelivr.net/npm/htmx.org@2.0.6/dist/htmx.min.js" {};
        body {
            (content)
        }
    }
}

fn tmpl_list_peers(peers: Vec<Peer>) -> Markup {
    layout(html! {
        h1 { "Peers" }
        ul {
            @for peer in peers {
                li { "Peer " (peer.node_id) }
            }
        }

        h2 { "Add New Peer" }
        form method="POST" action="/peers" {
            input type="text" name="id" placeholder="Node ID" required;
            input type="submit" value="Add Peer";
        }
    })
}

#[handler]
async fn list_peers(Data(db): Data<&DB>) -> Result<Markup> {
    let peers = Peer::list(db).await?;

    Ok(tmpl_list_peers(peers))
}

#[derive(Deserialize, Debug)]
struct CreatePeer {
    id: String,
}

#[handler]
async fn create_peer(Data(db): Data<&DB>, form: poem::Result<Form<CreatePeer>>) -> Result<Markup> {
    let Form(CreatePeer { id }) =
        form.map_err(|e| anyhow::anyhow!("Invalid Node ID format: {e}"))?;

    let node_id = id
        .parse::<NodeId>()
        .map_err(|e| anyhow::anyhow!("Invalid Node ID format: {e}"))?;

    Peer::create(db, node_id).await?;

    let peers = Peer::list(db).await?;
    Ok(tmpl_list_peers(peers))
}

pub async fn task(shutdown_tx: broadcast::Sender<()>, db: DB) -> Result<()> {
    let app = Route::new()
        .at("/peers", get(list_peers).post(create_peer))
        .data(db);

    info!("Starting web ui on port 3000");

    let mut shutdown_rx = shutdown_tx.subscribe();
    Server::new(TcpListener::bind("0.0.0.0:3000"))
        .run_with_graceful_shutdown(
            app,
            async move {
                let _ = shutdown_rx.recv().await;
                info!("Poem server received shutdown signal");
            },
            None,
        )
        .await
        .context("Failed to start web server")?;

    Ok(())
}
