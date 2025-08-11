use iroh::NodeId;
use maud::{html, Markup, DOCTYPE};
use poem::{
    get, handler, listener::TcpListener, web::{Data, Form}, Endpoint, EndpointExt, Route, Server,
};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use crate::{
    db::{Peer, DB},
    error::{AppError, Result},
    middleware::HtmxErrorMiddleware,
};

#[derive(Debug)]
pub enum WebServerMessage {
    Shutdown,
}

fn layout(content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        meta name="htmx-config" content=r#"{"responseHandling":[{"code":".*", "swap": true}]}"#;
        script src="https://cdn.jsdelivr.net/npm/htmx.org@2.0.6/dist/htmx.min.js" {};
        body {
            (content)
        }
    }
}

fn tmpl_peer_list(peers: &Vec<Peer>) -> Markup {
    html! {
        ul id="peer-list" {
            @for peer in peers {
                li { "Peer " (peer.node_id) }
            }
        }
    }
}

fn tmpl_list_peers(peers: Vec<Peer>) -> Markup {
    layout(html! {
        h1 { "Peers" }
        (tmpl_peer_list(&peers))

        h2 { "Add New Peer" }
        div id="error-message" style="color: red; margin-bottom: 10px;" {}
        form method="POST" action="/peers" hx-post="/peers" hx-target="#peer-list" hx-swap="outerHTML" {
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
        form.map_err(|e| AppError::BadRequest(format!("Invalid form data: {e}")))?;

    let node_id = id
        .parse::<NodeId>()
        .map_err(|e| AppError::BadRequest(format!("Invalid Node ID format: {e}")))?;

    Peer::create(db, node_id)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let peers = Peer::list(db).await?;
    Ok(tmpl_peer_list(&peers))
}

pub fn create_app(db: DB) -> impl Endpoint {
    Route::new()
        .at("/peers", get(list_peers).post(create_peer))
        .with(HtmxErrorMiddleware)
        .data(db)
}

async fn run_server(app: impl Endpoint + 'static, shutdown_rx: oneshot::Receiver<()>) {
    let result = Server::new(TcpListener::bind("0.0.0.0:3000"))
        .run_with_graceful_shutdown(
            app,
            async {
                let _ = shutdown_rx.await;
                info!("WebServer received shutdown signal");
            },
            Some(std::time::Duration::from_secs(30)),
        )
        .await;

    match result {
        Ok(_) => info!("WebServer shutdown complete"),
        Err(e) => tracing::error!("WebServer error: {}", e),
    }
}

pub async fn webserver_task(db: DB, mut rx: mpsc::Receiver<WebServerMessage>) {
    info!("WebServer task started");

    let app = create_app(db);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Spawn the server
    let server_handle = tokio::spawn(run_server(app, shutdown_rx));

    // Wait for shutdown message
    while let Some(message) = rx.recv().await {
        match message {
            WebServerMessage::Shutdown => {
                info!("WebServer task received shutdown message");
                let _ = shutdown_tx.send(());
                break;
            }
        }
    }

    // Wait for server to shut down
    let _ = server_handle.await;
    info!("WebServer task stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use poem::test::TestClient;

    #[derive(serde::Serialize)]
    struct TestCreatePeer {
        id: String,
    }

    async fn create_test_db() -> DB {
        let db = crate::db::new_test().await;
        crate::db::initialize_database(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn test_list_peers_empty() {
        let db = create_test_db().await;
        let app = create_app(db);
        let client = TestClient::new(app);

        let response = client.get("/peers").send().await;
        response.assert_status_is_ok();

        let body = response.0.into_body().into_string().await.unwrap();
        assert!(body.contains("Peers"));
        assert!(body.contains("Add New Peer"));
    }

    #[tokio::test]
    async fn test_create_peer_invalid_node_id() {
        let db = create_test_db().await;
        let app = create_app(db);
        let client = TestClient::new(app);

        let response = client
            .post("/peers")
            .form(&TestCreatePeer {
                id: "invalid-node-id".to_string(),
            })
            .send()
            .await;

        response.assert_status(poem::http::StatusCode::BAD_REQUEST);
        response
            .assert_text("Invalid input: Invalid Node ID format: invalid length")
            .await;
    }

    #[tokio::test]
    async fn test_create_peer_htmx_error_handling() {
        let db = create_test_db().await;
        let app = create_app(db);
        let client = TestClient::new(app);

        let response = client
            .post("/peers")
            .header("HX-Request", "true")
            .form(&TestCreatePeer {
                id: "invalid-node-id".to_string(),
            })
            .send()
            .await;

        response.assert_status(poem::http::StatusCode::BAD_REQUEST);
        response.assert_header("HX-Retarget", "#error-message");
        response.assert_header("HX-Reswap", "innerHTML");
        response
            .assert_text("Invalid input: Invalid Node ID format: invalid length")
            .await;
    }

    #[tokio::test]
    async fn test_create_peer_success() {
        let db = create_test_db().await;
        let app = create_app(db);
        let client = TestClient::new(app);

        // Generate a valid iroh NodeId
        use iroh::PublicKey;
        let valid_node_id = PublicKey::from_bytes(&[1u8; 32]).unwrap();

        let response = client
            .post("/peers")
            .form(&TestCreatePeer {
                id: valid_node_id.to_string(),
            })
            .send()
            .await;

        response.assert_status_is_ok();

        let body = response.0.into_body().into_string().await.unwrap();
        assert!(body.contains("peer-list"));
        assert!(body.contains(&valid_node_id.to_string()));
    }

    #[tokio::test]
    async fn test_create_peer_duplicate() {
        let db = create_test_db().await;
        let app = create_app(db);
        let client = TestClient::new(app);

        // Generate a valid iroh NodeId
        use iroh::PublicKey;
        let mut key_bytes = [1u8; 32];
        key_bytes[0] = 2; // Make it different from the first test
        let valid_node_id = PublicKey::from_bytes(&key_bytes).unwrap();

        // Create the peer first time - should succeed
        let response = client
            .post("/peers")
            .form(&TestCreatePeer {
                id: valid_node_id.to_string(),
            })
            .send()
            .await;
        response.assert_status_is_ok();

        // Try to create the same peer again - should fail
        let response = client
            .post("/peers")
            .form(&TestCreatePeer {
                id: valid_node_id.to_string(),
            })
            .send()
            .await;

        response.assert_status(poem::http::StatusCode::BAD_REQUEST);
        let body = response.0.into_body().into_string().await.unwrap();
        assert!(body.contains("already contains"));
    }
}