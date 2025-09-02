use std::time::Duration;

use chrono::{DateTime, Utc};
use chrono_humanize::HumanTime;
use iroh::NodeId;
use maud::{DOCTYPE, Markup, html};
use poem::{
    Body, Endpoint, EndpointExt, IntoResponse, Route, Server, get, handler, listener::TcpListener,
    post, web::Form, web::Path, web::Query, web::Redirect,
};
use ractor::ActorRef;
use ractor::registry;
use ractor::{Actor, ActorProcessingErr};
use serde::Deserialize;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::info;
use tracing::{debug, error};
use url::form_urlencoded;

use crate::{
    db::{Event, EventType, GroupedSecret, Identity, Peer, Secret, decrypt_secret_for_identity},
    error::{AppError, Result},
    middleware::HtmxErrorMiddleware,
    web_components::*,
};

pub struct WebServerActor;

#[derive(Debug)]
pub enum WebServerMessage {}

impl Actor for WebServerActor {
    type Msg = WebServerMessage;
    type State = (Option<oneshot::Sender<()>>, JoinHandle<()>, u64);
    type Arguments = (u16, u64);

    async fn pre_start(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        (port, shutdown_timeout): Self::Arguments,
    ) -> std::result::Result<Self::State, ActorProcessingErr> {
        info!("Web Server Actor pre start");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let server_task_handle = tokio::spawn(async move {
            let bind_addr = format!("0.0.0.0:{port}");
            let app = create_app();

            let server = Server::new(TcpListener::bind(&bind_addr)).run_with_graceful_shutdown(
                app,
                async {
                    let _ = shutdown_rx.await;
                    debug!("Poem server received shutdown signal");
                },
                Some(std::time::Duration::from_secs(shutdown_timeout)),
            );

            if let Err(e) = server.await {
                error!("Web server error: {}", e);
            }
        });

        Ok((Some(shutdown_tx), server_task_handle, shutdown_timeout))
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        (shutdown_tx, server_task_handle, shutdown_timeout): &mut Self::State,
    ) -> std::result::Result<(), ActorProcessingErr> {
        // Send the shutdown signal to the Poem server
        if let Some(tx) = shutdown_tx.take() {
            if let Err(e) = tx.send(()) {
                debug!("Failed to send shutdown signal to Poem ({e:?})");
            }
        }

        // Wait for the server task to actually finish
        // Do the time for the Poem plus 3 seconds before killing it
        match tokio::time::timeout(
            Duration::from_secs(*shutdown_timeout + 3),
            server_task_handle,
        )
        .await
        {
            Ok(Ok(_)) => debug!("Web server task shut down cleanly"),
            Ok(Err(e)) => error!("Web server task error: {e:?}"),
            Err(_) => error!("Web server shutdown timed out"),
        }

        Ok(())
    }
}

fn format_relative_time(datetime: &DateTime<Utc>) -> String {
    HumanTime::from(*datetime).to_string()
}

fn copy_button_component(text: &str, button_style: &str) -> Markup {
    html! {
        button
            onclick=(format!("navigator.clipboard.writeText('{}'); this.textContent = '✓'; setTimeout(() => this.textContent = '📋', 1000);", text))
            style=(button_style)
            title=(format!("Copy {}", if text.len() > 20 { "Hash" } else { "Node ID" }))
        {
            "📋"
        }
    }
}

fn node_id_with_copy(node_id: &str, style_class: &str) -> Markup {
    html! {
        div style="display: flex; align-items: center; gap: 6px;" {
            code style=(style_class) { (node_id) }
            (copy_button_component(node_id, "background: #f3f4f6; border: 1px solid #d1d5db; padding: 2px 6px; border-radius: 3px; cursor: pointer; font-size: 0.7em; color: #6b7280;"))
        }
    }
}

fn hash_with_copy(hash: &str, style_class: &str) -> Markup {
    html! {
        div style="display: flex; align-items: center; gap: 8px;" {
            code style=(format!("{}; flex: 1;", style_class)) { (hash) }
            (copy_button_component(hash, "background: #f3f4f6; border: 1px solid #d1d5db; padding: 4px 8px; border-radius: 3px; cursor: pointer; font-size: 0.7em; color: #6b7280; flex-shrink: 0;"))
        }
    }
}

fn format_json_for_ui(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(obj) => {
            let mut formatted_obj = serde_json::Map::new();
            for (key, val) in obj {
                formatted_obj.insert(key.clone(), format_json_value_for_ui(val));
            }
            serde_json::to_string_pretty(&serde_json::Value::Object(formatted_obj))
                .unwrap_or_else(|_| "Invalid JSON".to_string())
        }
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| "Invalid JSON".to_string()),
    }
}

fn format_json_value_for_ui(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(arr) => {
            // Check if this looks like a byte array (all numbers 0-255)
            if arr.len() > 20
                && arr.iter().all(|v| {
                    if let serde_json::Value::Number(n) = v {
                        n.as_u64().is_some_and(|n| n <= 255)
                    } else {
                        false
                    }
                })
            {
                // Convert to compact representation
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                let preview_len = std::cmp::min(16, bytes.len());
                let preview_hex = hex::encode(&bytes[..preview_len]);
                serde_json::Value::String(format!(
                    "[{} bytes: 0x{}{}]",
                    bytes.len(),
                    preview_hex,
                    if bytes.len() > 16 { "..." } else { "" }
                ))
            } else {
                // Regular array, recurse
                serde_json::Value::Array(arr.iter().map(format_json_value_for_ui).collect())
            }
        }
        serde_json::Value::Object(obj) => {
            let mut formatted_obj = serde_json::Map::new();
            for (key, val) in obj {
                formatted_obj.insert(key.clone(), format_json_value_for_ui(val));
            }
            serde_json::Value::Object(formatted_obj)
        }
        _ => value.clone(),
    }
}

async fn get_current_identity() -> Result<Identity> {
    Identity::get_or_create()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))
}

async fn announce_secret_via_gossip(secret: &Secret) -> Result<()> {
    let gossip_actor = registry::where_is("gossip".to_string())
        .ok_or_else(|| AppError::Internal("GossipActor not found in registry".to_string()))?;
    let gossip_actor: ActorRef<crate::actors::gossip::GossipMessage> = gossip_actor.into();

    gossip_actor
        .cast(crate::actors::gossip::GossipMessage::AnnounceSecret(
            secret.clone(),
        ))
        .map_err(|e| AppError::Internal(format!("Failed to send announce secret message: {e}")))?;

    Ok(())
}

async fn announce_secret_deletion_via_gossip(
    name: String,
    hash: String,
    target_node_id: iroh::NodeId,
) -> Result<()> {
    let gossip_actor = registry::where_is("gossip".to_string())
        .ok_or_else(|| AppError::Internal("GossipActor not found in registry".to_string()))?;
    let gossip_actor: ActorRef<crate::actors::gossip::GossipMessage> = gossip_actor.into();

    gossip_actor
        .cast(
            crate::actors::gossip::GossipMessage::AnnounceSecretDeletion {
                name,
                hash,
                target_node_id,
            },
        )
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to send announce secret deletion message: {e}"
            ))
        })?;

    Ok(())
}

async fn send_secret_sync_request_via_gossip(target_node_id: iroh::NodeId) -> Result<()> {
    let gossip_actor = registry::where_is("gossip".to_string())
        .ok_or_else(|| AppError::Internal("GossipActor not found in registry".to_string()))?;
    let gossip_actor: ActorRef<crate::actors::gossip::GossipMessage> = gossip_actor.into();

    gossip_actor
        .cast(crate::actors::gossip::GossipMessage::SendSecretSyncRequest(
            target_node_id,
        ))
        .map_err(|e| {
            AppError::Internal(format!("Failed to send secret sync request message: {e}"))
        })?;

    Ok(())
}

async fn sync_all_secrets_to_systemd_via_actor() -> Result<()> {
    let systemd_actor = registry::where_is("systemd".to_string())
        .ok_or_else(|| AppError::Internal("SystemdActor not found in registry".to_string()))?;
    let systemd_actor: ActorRef<crate::actors::systemd::SystemdMessage> = systemd_actor.into();

    systemd_actor
        .cast(crate::actors::systemd::SystemdMessage::SyncAllSecrets)
        .map_err(|e| AppError::Internal(format!("Failed to send sync all secrets message: {e}")))?;

    Ok(())
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
            (empty_state("📡", "No peers connected", "Add a peer below to get started with the network."))
        } @else {
            div id="peer-list" style="display: flex; flex-direction: column; gap: 16px;" {
                @for peer in peers {
                    (list_item_card(html! {
                        div style="display: flex; align-items: center; margin-bottom: 12px;" {
                            span style="font-size: 1.5em; margin-right: 8px;" { "🖥️" }
                            div style="flex: 1;" {
                                a href=(format!("/peers/{}", peer.node_id))
                                  style="text-decoration: none; color: inherit;"
                                {
                                    (node_id_with_copy(&peer.node_id, "font-weight: bold; font-size: 0.9em; color: #2563eb; font-family: monospace;"))
                                }
                            }
                            div style="width: 8px; height: 8px; border-radius: 50%; background: #22c55e;" {}
                        }

                        @if let Some(_last_seen) = &peer.last_seen {
                            div style="display: flex; align-items: center; margin-bottom: 6px; font-size: 0.85em; color: #666;" {
                                span style="margin-right: 6px;" { "🕒" }
                                span { "Last seen " (peer.get_last_seen_utc().map_or("invalid timestamp".to_string(), |dt| format_relative_time(&dt))) }
                            }
                        } @else {
                            div style="display: flex; align-items: center; margin-bottom: 6px; font-size: 0.85em; color: #999;" {
                                span style="margin-right: 6px;" { "❓" }
                                span { "Never seen" }
                            }
                        }

                        @if let Some(hostname) = &peer.hostname {
                            div style="display: flex; align-items: center; margin-bottom: 6px; font-size: 0.85em; color: #666;" {
                                span style="margin-right: 6px;" { "🏠" }
                                span { (hostname) }
                            }
                        }

                        @if let Some(age_key) = &peer.age_public_key {
                            div style="display: flex; align-items: center; margin-bottom: 12px; font-size: 0.85em; color: #666;" {
                                span style="margin-right: 6px;" { "🔐" }
                                span style="font-family: monospace; word-break: break-all;" { (age_key) }
                            }
                        }

                        div style="text-align: right;" {
                            a
                                href=(format!("/secrets?peer={}", peer.node_id))
                                style="background: #3b82f6; color: white; padding: 6px 12px; border-radius: 4px; text-decoration: none; font-size: 0.8em; display: inline-block;"
                            {
                                "🔍 View Secrets"
                            }
                        }
                    }))
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
                li style="margin: 10px 0;" {
                    a href="/secrets" style="display: block; padding: 10px; background: #f0f0f0; text-decoration: none; border-radius: 5px;" {
                        "🔐 Secrets"
                    }
                }
            }
        }
    })
}

fn tmpl_list_peers(peers: Vec<Peer>, current_node_id: NodeId) -> Markup {
    layout(html! {
        (nav_breadcrumb("/", "Home"))

        h1 { "Peers" }

        h2 style="margin-bottom: 12px;" { "This Node" }
        div style="background: #f0f9ff; border: 2px solid #3b82f6; border-radius: 8px; padding: 16px; margin-bottom: 24px;" {
            div style="display: flex; align-items: center; margin-bottom: 12px;" {
                span style="font-size: 1.5em; margin-right: 8px;" { "🏠" }
                div style="flex: 1;" {
                    a href=(format!("/peers/{}", current_node_id.to_string()))
                      style="text-decoration: none; color: inherit;"
                    {
                        (node_id_with_copy(&current_node_id.to_string(), "font-weight: bold; font-size: 0.9em; color: #2563eb; font-family: monospace;"))
                    }
                }
                span style="background: #3b82f6; color: white; padding: 4px 12px; border-radius: 12px; font-size: 0.8em;" {
                    "YOU"
                }
            }
            div style="display: flex; align-items: center; font-size: 0.85em; color: #666;" {
                span style="margin-right: 6px;" { "🔗" }
                span { "Your local node - always connected" }
            }
        }

        h2 { "Add New Peer" }
        div id="error-message" style="color: red; margin-bottom: 10px;" {}
        form method="POST" action="/peers" hx-post="/peers" hx-target="#peer-list" hx-swap="outerHTML" style="margin-bottom: 20px;" {
            input type="text" name="id" placeholder="Node ID" required;
            input type="submit" value="Add Peer";
        }

        h2 { "Network Peers" }
        (tmpl_peer_list(&peers))
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
                            (format_relative_time(&event.get_time_utc()))
                        }
                        td style="border: 1px solid #ddd; padding: 8px;" {
                            @if let Ok(event_type) = event.get_event_type() {
                                @match &event_type {
                                    EventType::PeerMessage { .. } => {
                                        span style="background: #e3f2fd; padding: 2px 6px; border-radius: 3px; font-size: 0.9em;" {
                                            "PeerMessage"
                                        }
                                    }
                                }
                            }
                        }
                        td style="border: 1px solid #ddd; padding: 8px;" {
                            @if let Ok(event_type) = event.get_event_type() {
                                @match &event_type {
                                    EventType::PeerMessage { message_type } => {
                                        span style="background: #f3e5f5; padding: 2px 6px; border-radius: 3px; font-size: 0.9em;" {
                                            (message_type)
                                        }
                                    }
                                }
                            }
                        }
                        td style="border: 1px solid #ddd; padding: 8px;" {
                            (event.message)
                        }
                        td style="border: 1px solid #ddd; padding: 8px; font-family: monospace; font-size: 0.8em;" {
                            pre style="margin: 0; white-space: pre-wrap; word-break: break-all;" {
                                @if let Ok(data) = event.get_data() {
                                    (format_json_for_ui(&data))
                                } @else {
                                    "Invalid JSON"
                                }
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
        (nav_breadcrumb("/", "Home"))

        h1 { "Events" }
        p { "Last 100 events" }

        @if events.is_empty() {
            p style="color: #666;" { "No events yet" }
        } @else {
            (tmpl_event_list(&events))
        }
    })
}

fn tmpl_grouped_secret_list(
    grouped_secrets: &[GroupedSecret],
    current_node_id: NodeId,
    peers: &[Peer],
) -> Markup {
    // Create a map of node_id to hostname for easy lookup
    let peer_hostnames: std::collections::HashMap<String, Option<String>> = peers
        .iter()
        .map(|p| (p.node_id.clone(), p.hostname.clone()))
        .collect();

    html! {
        @if grouped_secrets.is_empty() {
            (empty_state("🔐", "No secrets stored", "Add a secret below to start sharing encrypted data with peers."))
        } @else {
            div id="secret-list" style="display: flex; flex-direction: column; gap: 16px;" {
                @for grouped_secret in grouped_secrets {
                    (list_item_card(html! {
                        div style="display: flex; align-items: center; margin-bottom: 12px;" {
                            @if grouped_secret.has_target_node(&current_node_id) {
                                span style="font-size: 1.5em; margin-right: 8px;" { "🔑" }
                            } @else {
                                span style="font-size: 1.5em; margin-right: 8px;" { "🔒" }
                            }
                            div style="flex: 1;" {
                                a href=(format!("/secrets/{}/{}", grouped_secret.name, grouped_secret.hash))
                                  style="font-weight: bold; font-size: 1.1em; color: #2563eb; text-decoration: none;" {
                                    (grouped_secret.name)
                                }
                            }
                            div style="display: flex; align-items: center; gap: 8px;" {
                                @if grouped_secret.has_target_node(&current_node_id) {
                                    span style="background: #dcfce7; color: #166534; padding: 2px 8px; border-radius: 12px; font-size: 0.8em;" {
                                        "For You"
                                    }
                                    button
                                        hx-post=(format!("/secrets/{}/{}/delete", grouped_secret.name, grouped_secret.hash))
                                        hx-target="body"
                                        hx-swap="innerHTML"
                                        hx-confirm="Are you sure you want to delete this secret? This action cannot be undone."
                                        style="background: #dc2626; color: white; border: none; padding: 4px 8px; border-radius: 4px; cursor: pointer; font-size: 0.8em;"
                                        title="Delete this secret"
                                    {
                                        "🗑️"
                                    }
                                } @else {
                                    span style="background: #f3f4f6; color: #6b7280; padding: 2px 8px; border-radius: 12px; font-size: 0.8em;" {
                                        "For Others"
                                    }
                                }
                            }
                        }

                        div style="display: flex; align-items: center; margin-bottom: 6px; font-size: 0.85em; color: #666;" {
                            span style="margin-right: 6px;" { "🎯" }
                            span { "Targets: " }
                            div style="display: flex; flex-wrap: wrap; gap: 4px;" {
                                @for target_node_id in grouped_secret.get_target_node_ids() {
                                    div style="background: #f1f5f9; padding: 2px 6px; border-radius: 3px; font-size: 0.8em; display: flex; align-items: center; gap: 4px;" {
                                        code { (target_node_id) }
                                        button
                                            onclick=(format!("navigator.clipboard.writeText('{}'); this.textContent = '✓'; setTimeout(() => this.textContent = '📋', 500);", target_node_id))
                                            style="background: #f9fafb; border: 1px solid #d1d5db; padding: 1px 3px; border-radius: 2px; cursor: pointer; font-size: 0.6em; color: #6b7280;"
                                            title="Copy Node ID"
                                        {
                                            "📋"
                                        }
                                        @if let Some(hostname) = peer_hostnames.get(&target_node_id).and_then(|h| h.as_ref()) {
                                            span style="color: #059669; margin-left: 2px;" { "(" (hostname) ")" }
                                        }
                                    }
                                }
                            }
                        }

                        div style="display: flex; align-items: center; margin-bottom: 6px; font-size: 0.85em; color: #666;" {
                            span style="margin-right: 6px;" { "🏷️" }
                            span style="margin-right: 6px;" { "Hash: " }
                            (hash_with_copy(&grouped_secret.hash, "background: #f1f5f9; padding: 2px 4px; border-radius: 3px; font-size: 0.8em; word-break: break-all;"))
                        }

                        div style="display: flex; justify-content: space-between; font-size: 0.8em; color: #888;" {
                            span { "Created " (format_relative_time(&grouped_secret.get_created_at_utc())) }
                            span { "Updated " (format_relative_time(&grouped_secret.get_updated_at_utc())) }
                        }
                    }))
                }
            }
        }
    }
}

fn tmpl_list_grouped_secrets(
    grouped_secrets: Vec<GroupedSecret>,
    current_node_id: NodeId,
    peers: Vec<Peer>,
    peer_filter: Option<String>,
) -> Markup {
    // Find the peer hostname if filtering
    let filtered_peer_hostname = if let Some(ref filter_id) = peer_filter {
        peers
            .iter()
            .find(|p| p.node_id == *filter_id)
            .and_then(|p| p.hostname.as_ref())
    } else {
        None
    };

    layout(html! {
        (nav_breadcrumb("/", "Home"))

        @if let Some(filter_id) = &peer_filter {
            div style="background: #f0f9ff; border: 1px solid #0ea5e9; border-radius: 8px; padding: 12px; margin-bottom: 20px;" {
                div style="display: flex; align-items: center; gap: 8px;" {
                    span style="color: #0369a1; font-size: 1.2em;" { "🔍" }
                    span style="font-weight: bold; color: #0369a1;" { "Filtered by Peer:" }
                    code style="background: #f1f5f9; padding: 4px 8px; border-radius: 4px;" { (filter_id) }
                    @if let Some(hostname) = filtered_peer_hostname {
                        span style="color: #059669;" { "(" (hostname) ")" }
                    }
                    a href="/secrets" style="background: #6b7280; color: white; padding: 4px 8px; border-radius: 4px; text-decoration: none; font-size: 0.8em; margin-left: 8px;" {
                        "Clear Filter"
                    }
                }
            }
        }

        h1 {
            "Secrets"
            @if let Some(_) = &peer_filter {
                span style="color: #6b7280; font-weight: normal; font-size: 0.8em;" { " (Filtered)" }
            }
        }

        div style="margin-bottom: 20px;" {
            a href="/secrets/new" style="display: inline-block; padding: 10px 20px; background: #2563eb; color: white; text-decoration: none; border-radius: 5px;" {
                "➕ Add Secret"
            }
        }

        (tmpl_grouped_secret_list(&grouped_secrets, current_node_id, &peers))
    })
}

#[handler]
async fn index() -> Result<Markup> {
    Ok(tmpl_index())
}

#[handler]
async fn list_peers() -> Result<Markup> {
    let peers = Peer::list().await?;
    let identity = get_current_identity().await?;
    Ok(tmpl_list_peers(peers, identity.id()))
}

#[handler]
async fn list_events() -> Result<Markup> {
    let events = Event::list().await?;
    Ok(tmpl_list_events(events))
}

#[handler]
async fn list_secrets(query: Query<SecretsQuery>) -> Result<Markup> {
    let mut grouped_secrets = Secret::list_all_grouped()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Filter by peer if specified
    if let Some(peer_filter) = &query.peer {
        grouped_secrets.retain(|secret| secret.has_target_node_str(peer_filter));
    }

    let peers = Peer::list()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let identity = get_current_identity().await?;

    Ok(tmpl_list_grouped_secrets(
        grouped_secrets,
        identity.id(),
        peers,
        query.peer.clone(),
    ))
}

fn tmpl_secret_detail_grouped(
    secrets: &[Secret],
    current_node_id: NodeId,
    peers: Vec<Peer>,
) -> Markup {
    let secret = &secrets[0]; // All secrets have same name/hash, use first for metadata
    let is_for_current_node = secrets
        .iter()
        .any(|s| s.get_target_node_id().is_ok_and(|id| id == current_node_id));

    // Create hostname lookup map
    let peer_hostnames: std::collections::HashMap<String, Option<String>> = peers
        .iter()
        .map(|p| (p.node_id.clone(), p.hostname.clone()))
        .collect();

    layout(html! {
        (nav_breadcrumb("/secrets", "Back to Secrets"))

        h1 { "Secret: " (secret.name) }

        (card_container(html! {
            div style="display: grid; gap: 16px;" {
                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Name" }
                    code style="background: #f1f5f9; padding: 8px; border-radius: 4px; display: block; font-size: 1.1em;" {
                        (secret.name)
                    }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Target Nodes" }
                    div style="display: flex; flex-wrap: wrap; gap: 8px;" {
                        @for secret in secrets {
                            div style="background: #f1f5f9; padding: 8px 12px; border-radius: 4px; border: 1px solid #e2e8f0; display: flex; align-items: center; gap: 6px;" {
                                code { (secret.target_node_id) }
                                button
                                    onclick=(format!("navigator.clipboard.writeText('{}'); this.textContent = '✓'; setTimeout(() => this.textContent = '📋', 1000);", secret.target_node_id))
                                    style="background: #f9fafb; border: 1px solid #d1d5db; padding: 1px 4px; border-radius: 2px; cursor: pointer; font-size: 0.6em; color: #6b7280;"
                                    title="Copy Node ID"
                                {
                                    "📋"
                                }
                                @if let Some(hostname) = peer_hostnames.get(&secret.target_node_id).and_then(|h| h.as_ref()) {
                                    span style="color: #059669; margin-left: 2px;" { "(" (hostname) ")" }
                                }
                                @if secret.get_target_node_id().is_ok_and(|id| id == current_node_id) {
                                    span style="background: #dcfce7; color: #166534; padding: 2px 6px; border-radius: 8px; font-size: 0.7em; margin-left: 2px;" {
                                        "YOU"
                                    }
                                }
                            }
                        }
                    }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Hash" }
                    (hash_with_copy(&secret.hash, "background: #f1f5f9; padding: 8px; border-radius: 4px; word-break: break-all;"))
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Created" }
                    span style="color: #6b7280;" { (format_relative_time(&secret.get_created_at_utc())) }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Last Updated" }
                    span style="color: #6b7280;" { (format_relative_time(&secret.get_updated_at_utc())) }
                }

                @if is_for_current_node {
                    div {
                        label style="font-weight: bold; color: #374151; display: block; margin-bottom: 8px;" { "Encrypted Content" }
                        div style="display: flex; gap: 12px; margin-bottom: 12px;" {
                            button
                                hx-post=(format!("/secrets/{}/{}/reveal", secret.name, secret.hash))
                                hx-target="#secret-content"
                                style="padding: 8px 16px; background: #059669; color: white; border: none; border-radius: 4px; cursor: pointer;"
                            {
                                "🔓 Reveal Secret"
                            }
                            button
                                hx-get=(format!("/secrets/{}/{}/share", secret.name, secret.hash))
                                hx-target="#share-content"
                                style="padding: 8px 16px; background: #3b82f6; color: white; border: none; border-radius: 4px; cursor: pointer;"
                            {
                                "🔗 Share Secret"
                            }
                            button
                                hx-post=(format!("/secrets/{}/{}/delete", secret.name, secret.hash))
                                hx-target="body"
                                hx-swap="innerHTML"
                                hx-confirm="Are you sure you want to delete this secret? This action cannot be undone and will notify all peers."
                                style="padding: 8px 16px; background: #dc2626; color: white; border: none; border-radius: 4px; cursor: pointer;"
                            {
                                "🗑️ Delete Secret"
                            }
                        }
                        div id="secret-content" style="margin-top: 12px;" {
                            // Content will be loaded here by htmx
                        }
                        div id="share-content" style="margin-top: 12px;" {
                            // Share form will be loaded here by htmx
                        }
                    }
                }
            }
        }, None))
    })
}

#[allow(dead_code)] // Replaced by tmpl_grouped_secret_detail, kept for reference
fn tmpl_secret_detail(
    secret: &Secret,
    current_node_id: NodeId,
    peer_hostname: Option<String>,
) -> Markup {
    let is_for_current_node = secret
        .get_target_node_id()
        .is_ok_and(|id| id == current_node_id);

    layout(html! {
        nav style="margin-bottom: 20px;" {
            a href="/secrets" { "← Back to Secrets" }
        }

        h1 { "Secret: " (secret.name) }

        (card_container(html! {
            div style="display: grid; gap: 16px;" {
                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Name" }
                    code style="background: #f1f5f9; padding: 8px; border-radius: 4px; display: block; font-size: 1.1em;" {
                        (secret.name)
                    }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Target Node" }
                    div style="display: flex; align-items: center; gap: 8px;" {
                        code style="background: #f1f5f9; padding: 8px; border-radius: 4px; font-size: 0.9em;" {
                            (secret.target_node_id)
                        }
                        @if let Some(hostname) = peer_hostname {
                            span style="background: #dcfce7; color: #166534; padding: 4px 8px; border-radius: 12px; font-size: 0.8em;" {
                                (hostname)
                            }
                        }
                        @if is_for_current_node {
                            span style="background: #fef3c7; color: #d97706; padding: 4px 8px; border-radius: 12px; font-size: 0.8em;" {
                                "For You"
                            }
                        }
                    }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Hash" }
                    code style="background: #f1f5f9; padding: 8px; border-radius: 4px; display: block; font-size: 0.9em; word-break: break-all;" {
                        (secret.hash)
                    }
                }

                div style="display: grid; grid-template-columns: 1fr 1fr; gap: 16px;" {
                    div {
                        label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Created" }
                        span style="color: #6b7280;" { (format_relative_time(&secret.get_created_at_utc())) }
                    }
                    div {
                        label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Updated" }
                        span style="color: #6b7280;" { (format_relative_time(&secret.get_updated_at_utc())) }
                    }
                }
            }
        }, None))

        @if is_for_current_node {
            div style="background: #fef3c7; border: 1px solid #f59e0b; border-radius: 8px; padding: 16px; margin-bottom: 20px;" {
                h3 style="margin: 0 0 12px 0; color: #d97706;" { "🔓 Secret Content" }
                p style="margin: 0 0 12px 0; color: #92400e; font-size: 0.9em;" {
                    "This secret is encrypted for your node. Click below to reveal the content."
                }

                div id="secret-content" {
                    button
                        hx-post=(format!("/secrets/{}/{}/reveal", secret.name, secret.hash))
                        hx-target="#secret-content"
                        hx-swap="innerHTML"
                        style="background: #d97706; color: white; border: none; padding: 8px 16px; border-radius: 4px; cursor: pointer;" {
                        "👁️ Reveal Secret"
                    }
                }
            }
        } @else {
            div style="background: #fef2f2; border: 1px solid #f87171; border-radius: 8px; padding: 16px; margin-bottom: 20px;" {
                h3 style="margin: 0 0 8px 0; color: #dc2626;" { "🔒 Access Denied" }
                p style="margin: 0; color: #7f1d1d; font-size: 0.9em;" {
                    "This secret is encrypted for another node and cannot be revealed on this device."
                }
            }
        }
    })
}

#[handler]
async fn get_secret_detail(
    poem::web::Path((name, hash)): poem::web::Path<(String, String)>,
) -> Result<Markup> {
    let secrets = Secret::find_by_name_and_hash(&name, &hash)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if secrets.is_empty() {
        return Err(AppError::NotFound("Secret not found".to_string()));
    }

    let peers = Peer::list()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let identity = get_current_identity().await?;

    Ok(tmpl_secret_detail_grouped(&secrets, identity.id(), peers))
}

#[handler]
async fn reveal_secret(
    poem::web::Path((name, hash)): poem::web::Path<(String, String)>,
) -> Result<Markup> {
    let identity = get_current_identity().await?;

    let secrets = Secret::find_by_name_and_hash(&name, &hash)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Find the secret meant for the current node
    let secret = secrets
        .into_iter()
        .find(|s| s.get_target_node_id().is_ok_and(|id| id == identity.id()))
        .ok_or_else(|| AppError::Forbidden("No secret found for your node".to_string()))?;

    let decrypted_content = decrypt_secret_for_identity(&secret.encrypted_data, &identity)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to decrypt secret: {e}")))?;

    let content_str = String::from_utf8(decrypted_content)
        .map_err(|e| AppError::Internal(format!("Secret content is not valid UTF-8: {e}")))?;

    Ok(html! {
        div style="margin-bottom: 12px;" {
            label style="font-weight: bold; color: #374151; display: block; margin-bottom: 8px;" { "Decrypted Content" }
            pre style="background: #f9fafb; border: 1px solid #d1d5db; padding: 12px; border-radius: 4px; white-space: pre-wrap; word-break: break-word; max-height: 300px; overflow-y: auto;" {
                (content_str)
            }
        }
        button
            hx-post=(format!("/secrets/{}/{}/hide", secret.name, secret.hash))
            hx-target="#secret-content"
            hx-swap="innerHTML"
            style="background: #6b7280; color: white; border: none; padding: 8px 16px; border-radius: 4px; cursor: pointer; margin-right: 8px;" {
            "🙈 Hide Secret"
        }
        button
            onclick=(format!("navigator.clipboard.writeText('{}'); this.textContent = 'Copied!'; setTimeout(() => this.textContent = '📋 Copy to Clipboard', 2000);", content_str.replace('\'', "\\'")))
            style="background: #059669; color: white; border: none; padding: 8px 16px; border-radius: 4px; cursor: pointer;" {
            "📋 Copy to Clipboard"
        }
    })
}

#[handler]
async fn hide_secret(
    poem::web::Path((name, hash)): poem::web::Path<(String, String)>,
) -> Result<Markup> {
    Ok(html! {
        button
            hx-post=(format!("/secrets/{}/{}/reveal", name, hash))
            hx-target="#secret-content"
            hx-swap="innerHTML"
            style="background: #d97706; color: white; border: none; padding: 8px 16px; border-radius: 4px; cursor: pointer;" {
            "👁️ Reveal Secret"
        }
    })
}

#[handler]
async fn share_secret_form(
    poem::web::Path((name, hash)): poem::web::Path<(String, String)>,
) -> Result<Markup> {
    let identity = get_current_identity().await?;
    let current_node_id = identity.id();

    // Get the secrets with this name/hash
    let secrets = Secret::find_by_name_and_hash(&name, &hash)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Verify user owns this secret
    let _owned_secret = secrets
        .iter()
        .find(|s| s.get_target_node_id().is_ok_and(|id| id == current_node_id))
        .ok_or_else(|| AppError::Forbidden("You can only share secrets you own".to_string()))?;

    // Get all peers
    let all_peers = Peer::list()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Create hostname lookup map
    let peer_hostnames: std::collections::HashMap<String, Option<String>> = all_peers
        .iter()
        .map(|p| (p.node_id.clone(), p.hostname.clone()))
        .collect();

    // Get existing target node IDs for this secret
    let existing_targets: std::collections::HashSet<String> =
        secrets.iter().map(|s| s.target_node_id.clone()).collect();

    // Filter out peers who already have this secret
    let available_peers: Vec<Peer> = all_peers
        .into_iter()
        .filter(|peer| !existing_targets.contains(&peer.node_id))
        .collect();

    Ok(tmpl_share_secret(
        &name,
        &hash,
        &secrets,
        available_peers,
        current_node_id,
        &peer_hostnames,
    ))
}

#[handler]
async fn process_share_secret(
    poem::web::Path((name, hash)): poem::web::Path<(String, String)>,
    body: Body,
) -> Result<Markup> {
    let identity = get_current_identity().await?;
    let current_node_id = identity.id();

    // Parse form data
    let body_bytes = body
        .into_bytes()
        .await
        .map_err(|e| AppError::BadRequest(format!("Failed to read body: {e}")))?;

    let mut target_nodes = Vec::new();
    for (key, value) in form_urlencoded::parse(&body_bytes) {
        if key.as_ref() == "target_nodes[]" {
            target_nodes.push(value.into_owned());
        }
    }

    if target_nodes.is_empty() {
        return Err(AppError::BadRequest(
            "At least one target node must be selected".to_string(),
        ));
    }

    // Get the secrets with this name/hash
    let secrets = Secret::find_by_name_and_hash(&name, &hash)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Find the secret owned by current node
    let owned_secret = secrets
        .iter()
        .find(|s| s.get_target_node_id().is_ok_and(|id| id == current_node_id))
        .ok_or_else(|| AppError::Forbidden("You can only share secrets you own".to_string()))?;

    // Decrypt the secret content
    let decrypted_content = decrypt_secret_for_identity(&owned_secret.encrypted_data, &identity)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to decrypt secret: {e}")))?;

    // Create secret copies for each selected target node
    for node_id_str in target_nodes {
        let target_node_id = node_id_str
            .parse::<NodeId>()
            .map_err(|e| AppError::BadRequest(format!("Invalid node ID {node_id_str}: {e}")))?;

        // Check if this target already has the secret
        let already_exists = secrets
            .iter()
            .any(|s| s.get_target_node_id().is_ok_and(|id| id == target_node_id));

        if already_exists {
            debug!(
                "Secret '{}' already exists for node {}, skipping",
                name, target_node_id
            );
            continue;
        }

        let new_secret = Secret::create(name.clone(), &decrypted_content, target_node_id)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create secret copy: {e}")))?;

        // Announce the secret to the network
        if let Err(e) = announce_secret_via_gossip(&new_secret).await {
            error!(
                "Failed to announce shared secret '{}': {}",
                new_secret.name, e
            );
        }

        debug!(
            "Shared secret '{}' with node {}",
            new_secret.name, new_secret.target_node_id
        );
    }

    // Get updated secrets and return complete page with success notification
    let updated_secrets = Secret::find_by_name_and_hash(&name, &hash)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let peers = Peer::list()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Create hostname lookup map
    let peer_hostnames: std::collections::HashMap<String, Option<String>> = peers
        .iter()
        .map(|p| (p.node_id.clone(), p.hostname.clone()))
        .collect();

    let secret = &updated_secrets[0];

    Ok(layout(html! {
        // Success notification
        div style="background: #dcfce7; border: 1px solid #22c55e; border-radius: 8px; padding: 16px; margin-bottom: 20px;" {
            div style="display: flex; align-items: center;" {
                span style="color: #22c55e; font-size: 1.2em; margin-right: 8px;" { "✅" }
                span style="font-weight: bold; color: #166534;" { "Secret Shared Successfully" }
            }
            p style="margin: 8px 0 0 0; color: #166534;" {
                "The secret has been shared with the selected peers and announced to the network."
            }
        }

        (nav_breadcrumb("/secrets", "Back to Secrets"))

        h1 { "Secret: " (secret.name) }

        (card_container(html! {
            div style="display: grid; gap: 16px;" {
                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Name" }
                    code style="background: #f1f5f9; padding: 8px; border-radius: 4px; display: block; font-size: 1.1em;" {
                        (secret.name)
                    }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Target Nodes" }
                    div style="display: flex; flex-wrap: wrap; gap: 8px;" {
                        @for secret in &updated_secrets {
                            div style="background: #f1f5f9; padding: 8px 12px; border-radius: 4px; border: 1px solid #e2e8f0; display: flex; align-items: center; gap: 6px;" {
                                code { (secret.target_node_id) }
                                button
                                    onclick=(format!("navigator.clipboard.writeText('{}'); this.textContent = '✓'; setTimeout(() => this.textContent = '📋', 1000);", secret.target_node_id))
                                    style="background: #f9fafb; border: 1px solid #d1d5db; padding: 1px 4px; border-radius: 2px; cursor: pointer; font-size: 0.6em; color: #6b7280;"
                                    title="Copy Node ID"
                                {
                                    "📋"
                                }
                                @if let Some(hostname) = peer_hostnames.get(&secret.target_node_id).and_then(|h| h.as_ref()) {
                                    span style="color: #059669; margin-left: 2px;" { "(" (hostname) ")" }
                                }
                                @if secret.get_target_node_id().is_ok_and(|id| id == current_node_id) {
                                    span style="background: #dcfce7; color: #166534; padding: 2px 6px; border-radius: 8px; font-size: 0.7em; margin-left: 2px;" {
                                        "YOU"
                                    }
                                }
                            }
                        }
                    }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Hash" }
                    (hash_with_copy(&secret.hash, "background: #f1f5f9; padding: 8px; border-radius: 4px; word-break: break-all;"))
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Created" }
                    span style="color: #6b7280;" { (format_relative_time(&secret.get_created_at_utc())) }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Last Updated" }
                    span style="color: #6b7280;" { (format_relative_time(&secret.get_updated_at_utc())) }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 8px;" { "Encrypted Content" }
                    div style="display: flex; gap: 12px; margin-bottom: 12px;" {
                        button
                            hx-post=(format!("/secrets/{}/{}/reveal", secret.name, secret.hash))
                            hx-target="#secret-content"
                            style="padding: 8px 16px; background: #059669; color: white; border: none; border-radius: 4px; cursor: pointer;"
                        {
                            "🔓 Reveal Secret"
                        }
                        button
                            hx-get=(format!("/secrets/{}/{}/share", secret.name, secret.hash))
                            hx-target="#share-content"
                            style="padding: 8px 16px; background: #3b82f6; color: white; border: none; border-radius: 4px; cursor: pointer;"
                        {
                            "🔗 Share Secret"
                        }
                        button
                            hx-post=(format!("/secrets/{}/{}/delete", secret.name, secret.hash))
                            hx-target="body"
                            hx-swap="innerHTML"
                            hx-confirm="Are you sure you want to delete this secret? This action cannot be undone and will notify all peers."
                            style="padding: 8px 16px; background: #dc2626; color: white; border: none; border-radius: 4px; cursor: pointer;"
                        {
                            "🗑️ Delete Secret"
                        }
                    }
                    div id="secret-content" style="margin-top: 12px;" {
                        // Content will be loaded here by htmx
                    }
                    div id="share-content" style="margin-top: 12px;" {
                        // Share form will be loaded here by htmx
                    }
                }
            }
        }, None))
    }))
}

fn tmpl_share_secret(
    secret_name: &str,
    secret_hash: &str,
    current_targets: &[Secret],
    available_peers: Vec<Peer>,
    current_node_id: NodeId,
    peer_hostnames: &std::collections::HashMap<String, Option<String>>,
) -> Markup {
    html! {
        div style="background: #f0f9ff; border: 1px solid #0ea5e9; border-radius: 8px; padding: 16px; margin-top: 16px;" {
            h3 style="margin: 0 0 12px 0; color: #0369a1;" { "🔗 Share Secret" }

            @if available_peers.is_empty() {
                p style="color: #6b7280; margin: 0;" {
                    "This secret is already shared with all available peers."
                }
            } @else {
                div style="margin-bottom: 16px;" {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 8px;" {
                        "Current Access"
                    }
                    div style="display: flex; flex-wrap: wrap; gap: 4px;" {
                        @for secret in current_targets {
                            div style="background: #dcfce7; padding: 4px 8px; border-radius: 4px; font-size: 0.8em; color: #166534;" {
                                code { (secret.target_node_id) }
                                @if let Some(hostname) = peer_hostnames.get(&secret.target_node_id).and_then(|h| h.as_ref()) {
                                    span style="margin-left: 4px;" { "(" (hostname) ")" }
                                }
                                @if secret.get_target_node_id().is_ok_and(|id| id == current_node_id) {
                                    span style="background: #fef3c7; color: #d97706; padding: 1px 4px; border-radius: 3px; font-size: 0.7em; margin-left: 4px;" {
                                        "YOU"
                                    }
                                }
                            }
                        }
                    }
                }

                form hx-post=(format!("/secrets/{}/{}/share", secret_name, secret_hash)) hx-target="body" hx-swap="innerHTML" {
                    div style="margin-bottom: 16px;" {
                        label style="font-weight: bold; color: #374151; display: block; margin-bottom: 8px;" {
                            "Share With Additional Peers"
                        }
                        div style="display: grid; gap: 8px; max-height: 150px; overflow-y: auto; border: 1px solid #d1d5db; border-radius: 4px; padding: 8px;" {
                            @for peer in &available_peers {
                                div style="display: flex; align-items: center; padding: 4px;" {
                                    input
                                        type="checkbox"
                                        name="target_nodes[]"
                                        value=(peer.node_id)
                                        id=(format!("share-peer-{}", peer.node_id))
                                        style="margin-right: 8px;"
                                        ;
                                    label
                                        for=(format!("share-peer-{}", peer.node_id))
                                        style="flex: 1; display: flex; align-items: center; cursor: pointer;"
                                        {
                                        div style="flex: 1;" {
                                            div style="font-family: monospace; font-size: 0.8em;" {
                                                (peer.node_id)
                                            }
                                            @if let Some(hostname) = &peer.hostname {
                                                div style="font-size: 0.8em; color: #059669;" {
                                                    (hostname)
                                                }
                                            }
                                        }
                                        @if let Some(_) = peer.last_seen {
                                            span style="color: #22c55e; margin-left: 8px;" { "●" }
                                        } @else {
                                            span style="color: #6b7280; margin-left: 8px;" { "○" }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div style="display: flex; gap: 12px;" {
                        input
                            type="submit"
                            value="🔗 Share Secret"
                            style="background: #3b82f6; color: white; border: none; padding: 8px 16px; border-radius: 4px; cursor: pointer;"
                            ;
                        button
                            type="button"
                            hx-get=""
                            hx-target="#share-content"
                            hx-swap="innerHTML"
                            style="background: #6b7280; color: white; border: none; padding: 8px 16px; border-radius: 4px; cursor: pointer;"
                            { "Cancel" }
                    }
                }
            }
        }
    }
}

fn tmpl_add_secret(peers: Vec<Peer>, current_node_id: NodeId) -> Markup {
    layout(html! {
        (nav_breadcrumb("/secrets", "Back to Secrets"))

        h1 { "Add New Secret" }

        @if peers.is_empty() {
            div style="background: #fef2f2; border: 1px solid #f87171; border-radius: 8px; padding: 16px; margin-bottom: 20px;" {
                h3 style="margin: 0 0 8px 0; color: #dc2626;" { "⚠️ No Peers Available" }
                p style="margin: 0; color: #7f1d1d;" {
                    "You need to have at least one peer to share secrets with. "
                    a href="/peers" style="color: #2563eb;" { "Add some peers first" }
                    "."
                }
            }
        } @else {
            div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 20px;" {
                div id="error-message" style="color: red; margin-bottom: 10px;" {}

                form method="POST" action="/secrets" hx-post="/secrets" hx-target="body" hx-swap="innerHTML" {
                    div style="margin-bottom: 16px;" {
                        label for="secret-name" style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" {
                            "Secret Name"
                        }
                        input
                            type="text"
                            id="secret-name"
                            name="name"
                            placeholder="my-secret-key"
                            required
                            pattern="[a-zA-Z0-9._-]+"
                            title="Only letters, numbers, periods, underscores, and hyphens allowed"
                            style="width: 100%; padding: 8px; border: 1px solid #d1d5db; border-radius: 4px; font-family: monospace;"
                            ;
                        p style="font-size: 0.8em; color: #6b7280; margin: 4px 0 0 0;" {
                            "Use filesystem-safe characters only (letters, numbers, ., _, -)"
                        }
                    }

                    div style="margin-bottom: 16px;" {
                        label for="secret-content" style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" {
                            "Secret Content"
                        }
                        textarea
                            id="secret-content"
                            name="content"
                            placeholder="Paste your secret content here..."
                            required
                            rows="6"
                            style="width: 100%; padding: 8px; border: 1px solid #d1d5db; border-radius: 4px; font-family: monospace; resize: vertical;"
                            {}
                        p style="font-size: 0.8em; color: #6b7280; margin: 4px 0 0 0;" {
                            "Whitespace will be trimmed from start and end"
                        }
                    }

                    div style="margin-bottom: 20px;" {
                        label style="font-weight: bold; color: #374151; display: block; margin-bottom: 8px;" {
                            "Target Nodes"
                        }
                        p style="font-size: 0.9em; color: #6b7280; margin: 0 0 8px 0;" {
                            "Select one or more peers to share this secret with:"
                        }
                        div style="display: grid; gap: 8px; max-height: 200px; overflow-y: auto; border: 1px solid #d1d5db; border-radius: 4px; padding: 8px;" {
                            // Current node option
                            div style="display: flex; align-items: center; padding: 4px; background: #f0f9ff; border-radius: 4px;" {
                                input
                                    type="checkbox"
                                    name="target_nodes[]"
                                    value=(current_node_id.to_string())
                                    id="peer-current"
                                    style="margin-right: 8px;"
                                    ;
                                label
                                    for="peer-current"
                                    style="flex: 1; display: flex; align-items: center; cursor: pointer;"
                                    {
                                    div style="flex: 1;" {
                                        div style="font-family: monospace; font-size: 0.8em;" {
                                            (current_node_id.to_string())
                                        }
                                        div style="font-size: 0.8em; color: #2563eb; font-weight: bold;" {
                                            "This Node (You)"
                                        }
                                    }
                                    span style="color: #2563eb; margin-left: 8px;" { "🏠" }
                                }
                            }

                            @for peer in &peers {
                                div style="display: flex; align-items: center; padding: 4px;" {
                                    input
                                        type="checkbox"
                                        name="target_nodes[]"
                                        value=(peer.node_id)
                                        id=(format!("peer-{}", peer.node_id))
                                        style="margin-right: 8px;"
                                        ;
                                    label
                                        for=(format!("peer-{}", peer.node_id))
                                        style="flex: 1; display: flex; align-items: center; cursor: pointer;"
                                        {
                                        div style="flex: 1;" {
                                            div style="font-family: monospace; font-size: 0.8em;" {
                                                (peer.node_id)
                                            }
                                            @if let Some(hostname) = &peer.hostname {
                                                div style="font-size: 0.8em; color: #059669;" {
                                                    (hostname)
                                                }
                                            }
                                        }
                                        @if let Some(_) = peer.last_seen {
                                            span style="color: #22c55e; margin-left: 8px;" { "●" }
                                        } @else {
                                            span style="color: #6b7280; margin-left: 8px;" { "○" }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div style="display: flex; gap: 12px;" {
                        input
                            type="submit"
                            value="🔐 Create Secret"
                            style="background: #2563eb; color: white; border: none; padding: 10px 20px; border-radius: 4px; cursor: pointer; flex: 1;"
                            ;
                        a
                            href="/secrets"
                            style="background: #6b7280; color: white; text-decoration: none; padding: 10px 20px; border-radius: 4px; text-align: center; display: block; flex: 1;"
                            { "Cancel" }
                    }
                }
            }
        }
    })
}

#[handler]
async fn add_secret_form() -> Result<Markup> {
    let peers = Peer::list()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let identity = get_current_identity().await?;
    Ok(tmpl_add_secret(peers, identity.id()))
}

#[handler]
async fn create_secret(body: Body) -> Result<Markup> {
    let body_bytes = body
        .into_bytes()
        .await
        .map_err(|e| AppError::BadRequest(format!("Failed to read body: {e}")))?;

    // Parse the form data using url::form_urlencoded
    let mut name = String::new();
    let mut content = String::new();
    let mut target_nodes = Vec::new();

    for (key, value) in form_urlencoded::parse(&body_bytes) {
        match key.as_ref() {
            "name" => name = value.into_owned(),
            "content" => content = value.into_owned(),
            "target_nodes[]" => target_nodes.push(value.into_owned()),
            _ => {} // Ignore unknown fields
        }
    }

    if name.is_empty() {
        return Err(AppError::BadRequest("Secret name is required".to_string()));
    }

    if target_nodes.is_empty() {
        return Err(AppError::BadRequest(
            "At least one target node must be selected".to_string(),
        ));
    }

    // Trim whitespace from content
    let trimmed_content = content.trim();
    if trimmed_content.is_empty() {
        return Err(AppError::BadRequest(
            "Secret content cannot be empty".to_string(),
        ));
    }

    let content_bytes = trimmed_content.as_bytes();

    // Create secret for each target node
    info!(
        "🆕 Creating secret '{}' for {} target nodes",
        name,
        target_nodes.len()
    );
    for (i, node_id_str) in target_nodes.iter().enumerate() {
        debug!(
            "📝 Processing target {}/{}: {}",
            i + 1,
            target_nodes.len(),
            node_id_str
        );
        let target_node_id = node_id_str
            .parse::<NodeId>()
            .map_err(|e| AppError::BadRequest(format!("Invalid node ID {node_id_str}: {e}")))?;

        info!("💾 Creating secret '{}' for node {}", name, target_node_id);
        let secret = Secret::create(name.clone(), content_bytes, target_node_id)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create secret: {e}")))?;
        info!(
            "✅ Secret '{}' created in database for node {}",
            secret.name, secret.target_node_id
        );

        // Announce the secret to the network
        info!(
            "📡 Announcing secret '{}' to gossip network for node {}",
            secret.name, target_node_id
        );
        if let Err(e) = announce_secret_via_gossip(&secret).await {
            error!("❌ Failed to announce secret '{}': {}", secret.name, e);
        } else {
            info!(
                "✅ Successfully announced secret '{}' for node {}",
                secret.name, target_node_id
            );
        }

        debug!(
            "Created and announced secret '{}' for node {}",
            secret.name, secret.target_node_id
        );
    }
    info!(
        "🎉 Completed creating secret '{}' for all {} targets",
        name,
        target_nodes.len()
    );

    // Redirect to secrets list
    let grouped_secrets = Secret::list_all_grouped()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let peers = Peer::list()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let identity = get_current_identity().await?;

    Ok(tmpl_list_grouped_secrets(
        grouped_secrets,
        identity.id(),
        peers,
        None,
    ))
}

#[derive(Deserialize, Debug)]
struct CreatePeer {
    id: String,
}

#[derive(Deserialize, Debug)]
struct SecretsQuery {
    peer: Option<String>,
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

#[handler]
async fn delete_secret(
    poem::web::Path((name, hash)): poem::web::Path<(String, String)>,
) -> Result<Markup> {
    let identity = get_current_identity().await?;

    // Parse the target node ID from the hash - we need to find the secret first
    let secrets = Secret::find_by_name_and_hash(&name, &hash)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Find the secret meant for the current node
    let secret = secrets
        .into_iter()
        .find(|s| s.get_target_node_id().is_ok_and(|id| id == identity.id()))
        .ok_or_else(|| {
            AppError::Forbidden("You can only delete secrets that belong to you".to_string())
        })?;

    let target_node_id = secret
        .get_target_node_id()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Delete the secret from the database
    let was_deleted = Secret::delete(&name, &hash, target_node_id)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to delete secret: {e}")))?;

    if was_deleted {
        // Announce the deletion to the network
        if let Err(e) =
            announce_secret_deletion_via_gossip(name.clone(), hash.clone(), target_node_id).await
        {
            error!("Failed to announce secret deletion '{}': {}", name, e);
        }

        debug!(
            "Deleted and announced deletion of secret '{}' for node {}",
            name, target_node_id
        );
    }

    // Redirect back to secrets list
    let grouped_secrets = Secret::list_all_grouped()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let peers = Peer::list()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(tmpl_list_grouped_secrets(
        grouped_secrets,
        identity.id(),
        peers,
        None,
    ))
}

fn tmpl_peer_detail(peer: &Peer, secrets: Vec<GroupedSecret>, current_node_id: NodeId) -> Markup {
    let is_current_node = peer.node_id == current_node_id.to_string();

    layout(html! {
        (nav_breadcrumb("/peers", "Back to Peers"))

        h1 {
            @if is_current_node {
                "Your Node Details"
            } @else {
                "Peer Details"
            }
        }

        div style=(format!("background: white; border: {}; border-radius: 8px; padding: 20px; margin-bottom: 20px;",
                          if is_current_node { "2px solid #3b82f6" } else { "1px solid #ddd" })) {
            div style="display: flex; align-items: center; margin-bottom: 16px;" {
                span style="font-size: 2em; margin-right: 12px;" {
                    @if is_current_node {
                        "🏠"
                    } @else {
                        "🖥️"
                    }
                }
                div {
                    h2 style="margin: 0; color: #333;" {
                        @if is_current_node {
                            "Your Node Information"
                        } @else {
                            "Node Information"
                        }
                    }
                }
                @if let Some(_last_seen) = &peer.last_seen {
                    div style="width: 12px; height: 12px; border-radius: 50%; background: #22c55e; margin-left: auto;" {}
                } @else {
                    div style="width: 12px; height: 12px; border-radius: 50%; background: #6b7280; margin-left: auto;" {}
                }
            }

            div style="display: grid; gap: 16px;" {
                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Node ID" }
                    (node_id_with_copy(&peer.node_id, "background: #f1f5f9; padding: 8px; border-radius: 4px; font-size: 0.9em;"))
                }

                @if let Some(hostname) = &peer.hostname {
                    div {
                        label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Hostname" }
                        div style="display: flex; align-items: center; gap: 8px;" {
                            span style="font-size: 1.2em;" { "🏠" }
                            code style="background: #f1f5f9; padding: 8px; border-radius: 4px;" { (hostname) }
                        }
                    }
                }

                @if let Some(age_key) = &peer.age_public_key {
                    div {
                        label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Age Public Key" }
                        div style="display: flex; align-items: center; gap: 8px;" {
                            span style="font-size: 1.2em;" { "🔐" }
                            code style="background: #f1f5f9; padding: 8px; border-radius: 4px; word-break: break-all; font-size: 0.8em;" { (age_key) }
                        }
                    }
                }

                div style="display: grid; grid-template-columns: 1fr 1fr; gap: 16px;" {
                    div {
                        label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Connection Status" }
                        @if is_current_node {
                            span style="background: #dbeafe; color: #1d4ed8; padding: 4px 12px; border-radius: 12px; font-size: 0.9em; display: inline-flex; align-items: center; gap: 4px;" {
                                span { "●" }
                                "Local Node"
                            }
                        } @else if let Some(_last_seen) = &peer.last_seen {
                            span style="background: #dcfce7; color: #166534; padding: 4px 12px; border-radius: 12px; font-size: 0.9em; display: inline-flex; align-items: center; gap: 4px;" {
                                span { "●" }
                                "Connected"
                            }
                        } @else {
                            span style="background: #f3f4f6; color: #6b7280; padding: 4px 12px; border-radius: 12px; font-size: 0.9em; display: inline-flex; align-items: center; gap: 4px;" {
                                span { "○" }
                                "Never Seen"
                            }
                        }
                    }

                    @if let Some(_last_seen) = &peer.last_seen {
                        div {
                            label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Last Seen" }
                            span style="color: #6b7280;" { (peer.get_last_seen_utc().map_or("invalid timestamp".to_string(), |dt| format_relative_time(&dt))) }
                        }
                    }
                }
            }
        }

        div style="margin: 20px 0;" {
            form method="POST" action=(format!("/peers/{}/sync-secrets", peer.node_id)) style="display: inline;" {
                button type="submit" style="background: #059669; color: white; border: none; padding: 8px 16px; border-radius: 4px; cursor: pointer;" {
                    @if is_current_node {
                        "🔄 Sync Your Secrets to SystemD"
                    } @else {
                        "🔄 Request SystemD Sync"
                    }
                }
            }
        }

        h2 style="margin: 24px 0 16px 0;" {
            @if is_current_node {
                "Your Secrets"
            } @else {
                "Secrets for This Peer"
            }
        }
        @if secrets.is_empty() {
            div style="text-align: center; padding: 40px; background: white; border-radius: 8px; border: 1px solid #ddd;" {
                div style="font-size: 3em; margin-bottom: 16px; color: #999;" { "🔐" }
                h3 style="margin: 0 0 8px 0; color: #666;" {
                    @if is_current_node {
                        "No secrets stored"
                    } @else {
                        "No secrets for this peer"
                    }
                }
                p style="margin: 0; color: #888;" {
                    @if is_current_node {
                        "You don't have any secrets stored yet."
                    } @else {
                        "This peer doesn't have any secrets shared with them yet."
                    }
                }
            }
        } @else {
            (tmpl_grouped_secret_list(&secrets, current_node_id, &[peer.clone()]))
        }
    })
}

#[handler]
async fn get_peer_detail(poem::web::Path(node_id): poem::web::Path<String>) -> Result<Markup> {
    // Parse the node ID to validate it
    let parsed_node_id = node_id
        .parse::<NodeId>()
        .map_err(|e| AppError::BadRequest(format!("Invalid node ID: {e}")))?;

    let identity = get_current_identity().await?;

    // Check if this is the current node
    let peer = if parsed_node_id == identity.id() {
        // Create a virtual peer entry for the current node
        Peer {
            node_id: node_id.clone(),
            last_seen: Some(chrono::Utc::now().naive_utc()), // Always "connected"
            hostname: Some("localhost".to_string()),
            age_public_key: Some(crate::db::age_public_key_to_string(&identity.age_key)),
        }
    } else {
        // Get the peer from database
        Peer::find_by_node_id(&node_id)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Peer not found".to_string()))?
    };

    // Get secrets for this peer
    let mut grouped_secrets = Secret::list_all_grouped()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Filter to only secrets for this peer
    grouped_secrets.retain(|secret| secret.has_target_node_str(&node_id));

    Ok(tmpl_peer_detail(&peer, grouped_secrets, identity.id()))
}

#[handler]
async fn sync_peer_secrets(Path(node_id): Path<String>) -> Result<impl IntoResponse> {
    // Parse the node ID
    let target_node_id: NodeId = node_id
        .parse()
        .map_err(|e| AppError::BadRequest(format!("Invalid node ID: {e}")))?;

    // Get current identity to check if this is a self-sync
    let identity = get_current_identity().await?;
    let current_node_id = identity.id();

    if target_node_id == current_node_id {
        debug!("Syncing secrets to systemd for current node (direct call)");
        // For current node, sync directly to systemd
        sync_all_secrets_to_systemd_via_actor()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to sync secrets to systemd: {e}")))?;
        debug!("Successfully synced current node secrets to systemd");
    } else {
        debug!("Sending sync request to remote node {}", target_node_id);
        // For remote nodes, send sync request message
        send_secret_sync_request_via_gossip(target_node_id)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        debug!("Successfully sent sync request to remote node");
    }

    // Redirect back to the peer detail page
    Ok(Redirect::temporary(format!("/peers/{node_id}")))
}

pub fn create_app() -> impl Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/peers", get(list_peers).post(create_peer))
        .at("/peers/:node_id", get(get_peer_detail))
        .at("/peers/:node_id/sync-secrets", post(sync_peer_secrets))
        .at("/events", get(list_events))
        .at("/secrets", get(list_secrets).post(create_secret))
        .at("/secrets/new", get(add_secret_form))
        .at("/secrets/:name/:hash", get(get_secret_detail))
        .at("/secrets/:name/:hash/reveal", poem::post(reveal_secret))
        .at("/secrets/:name/:hash/hide", poem::post(hide_secret))
        .at(
            "/secrets/:name/:hash/share",
            get(share_secret_form).post(process_share_secret),
        )
        .at("/secrets/:name/:hash/delete", poem::post(delete_secret))
        .with(HtmxErrorMiddleware)
}
