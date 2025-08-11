use iroh::NodeId;
use maud::{DOCTYPE, Markup, html};
use poem::{
    Endpoint, EndpointExt, Route, Server, get, handler,
    listener::TcpListener,
    web::{Data, Form},
};
use ractor::{Actor, ActorProcessingErr, ActorRef};
use serde::Deserialize;
use tokio::sync::oneshot;
use tracing::info;

use crate::error::{AppError, Result};
use crate::middleware::HtmxErrorMiddleware;
use crate::{
    db::{DB, Peer},
    management::WebServerMessage,
};

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

pub struct WebServerActor;

impl WebServerActor {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug)]
pub struct WebServerState {
    db: DB,
    shutdown_tx: Option<oneshot::Sender<()>>,
    server_handle: Option<tokio::task::JoinHandle<std::result::Result<(), std::io::Error>>>,
}

#[derive(Debug)]
pub struct WebServerArgs {
    pub db: DB,
}

#[async_trait::async_trait]
impl Actor for WebServerActor {
    type Msg = WebServerMessage;
    type State = WebServerState;
    type Arguments = WebServerArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> std::result::Result<Self::State, ActorProcessingErr> {
        info!("WebServer actor started");

        let app = create_app(args.db.clone());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Start the web server in the background
        let server_handle = tokio::spawn(async move {
            info!("Starting web ui on port 3000");

            let result = Server::new(TcpListener::bind("0.0.0.0:3000"))
                .run_with_graceful_shutdown(
                    app,
                    async move {
                        let _ = shutdown_rx.await;
                        info!("Poem server received shutdown signal");
                    },
                    Some(std::time::Duration::from_secs(30)), // 30 second graceful shutdown timeout
                )
                .await;

            match &result {
                Ok(_) => info!("Poem server shutdown complete"),
                Err(e) => tracing::error!("Web server error: {}", e),
            }

            result
        });

        Ok(WebServerState {
            db: args.db,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> std::result::Result<(), ActorProcessingErr> {
        match message {
            WebServerMessage::Shutdown => {
                info!("WebServer received shutdown signal, cleaning up...");

                if let Some(shutdown_tx) = state.shutdown_tx.take() {
                    let _ = shutdown_tx.send(());
                }

                // Wait for the server task to complete (no timeout - give it all the time it needs)
                if let Some(server_handle) = state.server_handle.take() {
                    info!("Waiting for Poem server task to complete...");
                    let _ = server_handle.await;
                    info!("Poem server task finished gracefully");
                }

                // Stop the actor after shutdown
                _myself.stop(None);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> std::result::Result<(), ActorProcessingErr> {
        info!("WebServer actor stopping, signaling Poem server shutdown");

        if let Some(shutdown_tx) = state.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        Ok(())
    }
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
