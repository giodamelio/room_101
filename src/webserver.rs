use chrono::{DateTime, Utc};
use chrono_humanize::HumanTime;
use iroh::NodeId;
use maud::{DOCTYPE, Markup, html};
use poem::{
    Endpoint, EndpointExt, Route, Server, get, handler,
    listener::TcpListener,
    web::{Data, Form},
};
use serde::Deserialize;
use surrealdb::Datetime;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::{
    db::{DB, Peer},
    error::{AppError, Result},
    middleware::HtmxErrorMiddleware,
};

fn format_relative_time(datetime: &Datetime) -> String {
    // Use serde to serialize to a clean format
    let serialized = serde_json::to_string(datetime).unwrap();
    // Remove quotes from the JSON string
    let cleaned = serialized.trim_matches('"');
    let dt = cleaned.parse::<DateTime<Utc>>().unwrap();
    HumanTime::from(dt).to_string()
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
                li {
                    div { "Peer: " (peer.node_id) }
                    @if let Some(last_seen) = &peer.last_seen {
                        div style="font-size: 0.8em; color: #666;" {
                            "Last seen " (format_relative_time(last_seen))
                        }
                    } @else {
                        div style="font-size: 0.8em; color: #999;" {
                            "Never seen"
                        }
                    }
                    @if let Some(hostname) = &peer.hostname {
                        div style="font-size: 0.8em; color: #666;" {
                            "Hostname " (hostname)
                        }
                    }
                }
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

async fn server_subsystem(
    subsys: SubsystemHandle,
    app: impl Endpoint + 'static,
) -> anyhow::Result<()> {
    let result = Server::new(TcpListener::bind("0.0.0.0:3000"))
        .run_with_graceful_shutdown(
            app,
            async {
                subsys.on_shutdown_requested().await;
                info!("Poem server received shutdown signal");
            },
            Some(std::time::Duration::from_secs(30)),
        )
        .await;

    match result {
        Ok(_) => info!("Poem server shutdown complete"),
        Err(e) => tracing::error!("Poem server error: {}", e),
    }

    Ok(())
}

pub async fn webserver_subsystem(subsys: SubsystemHandle, db: DB) -> anyhow::Result<()> {
    info!("WebServer subsystem started");

    let app = create_app(db);

    // Start the server as a nested subsystem
    subsys.start(tokio_graceful_shutdown::SubsystemBuilder::new(
        "poem-server",
        move |server_subsys| server_subsystem(server_subsys, app),
    ));

    // Wait for shutdown signal
    subsys.on_shutdown_requested().await;
    info!("WebServer subsystem received shutdown signal");

    info!("WebServer subsystem stopped");
    Ok(())
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
