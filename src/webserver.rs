use chrono::{DateTime, Utc};
use chrono_humanize::HumanTime;
use iroh::NodeId;
use maud::{DOCTYPE, Markup, html};
use poem::{Endpoint, EndpointExt, Route, Server, get, handler, listener::TcpListener, web::Form};
use serde::Deserialize;
use surrealdb::Datetime;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::{
    db::{Event, Peer},
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
        body style="font-family: Arial, sans-serif; margin: 0; padding: 20px; background-color: #f5f5f5;" {
            (content)
        }
    }
}

fn tmpl_peer_list(peers: &Vec<Peer>) -> Markup {
    html! {
        @if peers.is_empty() {
            div style="text-align: center; padding: 40px; background: white; border-radius: 8px; border: 1px solid #ddd;" {
                div style="font-size: 3em; margin-bottom: 16px; color: #999;" { "📡" }
                h3 style="margin: 0 0 8px 0; color: #666;" { "No peers connected" }
                p style="margin: 0; color: #888;" { "Add a peer below to get started with the network." }
            }
        } @else {
            div id="peer-list" style="display: flex; flex-direction: column; gap: 16px;" {
                @for peer in peers {
                    div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 16px; box-shadow: 0 2px 4px rgba(0,0,0,0.1);" {
                        div style="display: flex; align-items: center; margin-bottom: 12px;" {
                            span style="font-size: 1.5em; margin-right: 8px;" { "🖥️" }
                            div style="flex: 1;" {
                                div style="font-weight: bold; font-size: 0.9em; color: #333; font-family: monospace;" {
                                    (peer.node_id.to_string())
                                }
                            }
                            div style="width: 8px; height: 8px; border-radius: 50%; background: #22c55e;" {}
                        }

                        @if let Some(last_seen) = &peer.last_seen {
                            div style="display: flex; align-items: center; margin-bottom: 6px; font-size: 0.85em; color: #666;" {
                                span style="margin-right: 6px;" { "🕒" }
                                span { "Last seen " (format_relative_time(last_seen)) }
                            }
                        } @else {
                            div style="display: flex; align-items: center; margin-bottom: 6px; font-size: 0.85em; color: #999;" {
                                span style="margin-right: 6px;" { "❓" }
                                span { "Never seen" }
                            }
                        }

                        @if let Some(hostname) = &peer.hostname {
                            div style="display: flex; align-items: center; font-size: 0.85em; color: #666;" {
                                span style="margin-right: 6px;" { "🏠" }
                                span { (hostname) }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn tmpl_index() -> Markup {
    layout(html! {
        h1 { "Room 101" }
        p { "A peer-to-peer networking application" }

        nav {
            ul style="list-style: none; padding: 0;" {
                li style="margin: 10px 0;" {
                    a href="/peers" style="display: block; padding: 10px; background: #f0f0f0; text-decoration: none; border-radius: 5px;" {
                        "📡 Peers"
                    }
                }
                li style="margin: 10px 0;" {
                    a href="/events" style="display: block; padding: 10px; background: #f0f0f0; text-decoration: none; border-radius: 5px;" {
                        "📋 Events"
                    }
                }
            }
        }
    })
}

fn tmpl_list_peers(peers: Vec<Peer>) -> Markup {
    layout(html! {
        nav style="margin-bottom: 20px;" {
            a href="/" { "← Home" }
        }

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

fn tmpl_event_list(events: &Vec<Event>) -> Markup {
    html! {
        table style="width: 100%; border-collapse: collapse;" {
            thead {
                tr {
                    th style="border: 1px solid #ddd; padding: 8px; text-align: left;" { "Time" }
                    th style="border: 1px solid #ddd; padding: 8px; text-align: left;" { "Event Type" }
                    th style="border: 1px solid #ddd; padding: 8px; text-align: left;" { "Details" }
                    th style="border: 1px solid #ddd; padding: 8px; text-align: left;" { "Message" }
                    th style="border: 1px solid #ddd; padding: 8px; text-align: left;" { "JSON Data" }
                }
            }
            tbody {
                @for event in events {
                    tr {
                        td style="border: 1px solid #ddd; padding: 8px;" {
                            (format_relative_time(&event.time))
                        }
                        td style="border: 1px solid #ddd; padding: 8px;" {
                            @match &event.event_type {
                                crate::db::EventType::PeerMessage { .. } => {
                                    span style="background: #e3f2fd; padding: 2px 6px; border-radius: 3px; font-size: 0.9em;" {
                                        "PeerMessage"
                                    }
                                }
                            }
                        }
                        td style="border: 1px solid #ddd; padding: 8px;" {
                            @match &event.event_type {
                                crate::db::EventType::PeerMessage { message_type } => {
                                    span style="background: #f3e5f5; padding: 2px 6px; border-radius: 3px; font-size: 0.9em;" {
                                        (message_type)
                                    }
                                }
                            }
                        }
                        td style="border: 1px solid #ddd; padding: 8px;" {
                            (event.message)
                        }
                        td style="border: 1px solid #ddd; padding: 8px; font-family: monospace; font-size: 0.8em;" {
                            pre style="margin: 0; white-space: pre-wrap; word-break: break-all;" {
                                (serde_json::to_string_pretty(&event.data).unwrap_or_else(|_| "Invalid JSON".to_string()))
                            }
                        }
                    }
                }
            }
        }
    }
}

fn tmpl_list_events(events: Vec<Event>) -> Markup {
    layout(html! {
        nav style="margin-bottom: 20px;" {
            a href="/" { "← Home" }
        }

        h1 { "Events" }
        p { "Last 100 events" }

        @if events.is_empty() {
            p style="color: #666;" { "No events yet" }
        } @else {
            (tmpl_event_list(&events))
        }
    })
}

#[handler]
async fn index() -> Result<Markup> {
    Ok(tmpl_index())
}

#[handler]
async fn list_peers() -> Result<Markup> {
    let peers = Peer::list().await?;
    Ok(tmpl_list_peers(peers))
}

#[handler]
async fn list_events() -> Result<Markup> {
    let events = Event::list().await?;
    Ok(tmpl_list_events(events))
}

#[derive(Deserialize, Debug)]
struct CreatePeer {
    id: String,
}

#[handler]
async fn create_peer(form: poem::Result<Form<CreatePeer>>) -> Result<Markup> {
    let Form(CreatePeer { id }) =
        form.map_err(|e| AppError::BadRequest(format!("Invalid form data: {e}")))?;

    let node_id = id
        .parse::<NodeId>()
        .map_err(|e| AppError::BadRequest(format!("Invalid Node ID format: {e}")))?;

    Peer::create(node_id)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let peers = Peer::list().await?;
    Ok(tmpl_peer_list(&peers))
}

pub fn create_app() -> impl Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/peers", get(list_peers).post(create_peer))
        .at("/events", get(list_events))
        .with(HtmxErrorMiddleware)
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

pub async fn webserver_subsystem(subsys: SubsystemHandle) -> anyhow::Result<()> {
    info!("WebServer subsystem started");

    let app = create_app();

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

    async fn setup_test_db() {
        // For tests, we need to initialize the global database instance
        // This is a bit tricky since OnceCell can only be initialized once
        // In a real test setup, you'd want to use a different approach
        // But for now, we'll just ensure the test database is available
        let _db = crate::db::db().await;
    }

    #[tokio::test]
    async fn test_list_peers_empty() {
        setup_test_db().await;
        let app = create_app();
        let client = TestClient::new(app);

        let response = client.get("/peers").send().await;
        response.assert_status_is_ok();

        let body = response.0.into_body().into_string().await.unwrap();
        assert!(body.contains("Peers"));
        assert!(body.contains("Add New Peer"));
    }

    #[tokio::test]
    async fn test_create_peer_invalid_node_id() {
        setup_test_db().await;
        let app = create_app();
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
        setup_test_db().await;
        let app = create_app();
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
        setup_test_db().await;
        let app = create_app();
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
        setup_test_db().await;
        let app = create_app();
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
