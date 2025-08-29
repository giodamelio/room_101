use chrono::{DateTime, Utc};
use chrono_humanize::HumanTime;
use iroh::NodeId;
use maud::{DOCTYPE, Markup, html};
use poem::{
    Body, Endpoint, EndpointExt, Route, Server, get, handler, listener::TcpListener, web::Data,
    web::Form,
};
use serde::Deserialize;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error};
use url::form_urlencoded;

use crate::{
    db::{Event, EventType, GroupedSecret, Identity, Peer, Secret, decrypt_secret_for_identity},
    error::{AppError, Result},
    middleware::HtmxErrorMiddleware,
    network::{PeerMessage, announce_secret, announce_secret_deletion},
};

fn format_relative_time(datetime: &DateTime<Utc>) -> String {
    HumanTime::from(*datetime).to_string()
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
                                    (peer.node_id)
                                }
                            }
                            div style="width: 8px; height: 8px; border-radius: 50%; background: #22c55e;" {}
                        }

                        @if let Some(_last_seen) = &peer.last_seen {
                            div style="display: flex; align-items: center; margin-bottom: 6px; font-size: 0.85em; color: #666;" {
                                span style="margin-right: 6px;" { "🕒" }
                                span { "Last seen " (format_relative_time(&peer.get_last_seen_utc().unwrap())) }
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
                            div style="display: flex; align-items: center; font-size: 0.85em; color: #666;" {
                                span style="margin-right: 6px;" { "🔐" }
                                span style="font-family: monospace; word-break: break-all;" { (age_key) }
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
                li style="margin: 10px 0;" {
                    a href="/secrets" style="display: block; padding: 10px; background: #f0f0f0; text-decoration: none; border-radius: 5px;" {
                        "🔐 Secrets"
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
            div style="text-align: center; padding: 40px; background: white; border-radius: 8px; border: 1px solid #ddd;" {
                div style="font-size: 3em; margin-bottom: 16px; color: #999;" { "🔐" }
                h3 style="margin: 0 0 8px 0; color: #666;" { "No secrets stored" }
                p style="margin: 0; color: #888;" { "Add a secret below to start sharing encrypted data with peers." }
            }
        } @else {
            div id="secret-list" style="display: flex; flex-direction: column; gap: 16px;" {
                @for grouped_secret in grouped_secrets {
                    div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 16px; box-shadow: 0 2px 4px rgba(0,0,0,0.1);" {
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
                                    div style="background: #f1f5f9; padding: 2px 6px; border-radius: 3px; font-size: 0.8em;" {
                                        code { (target_node_id) }
                                        @if let Some(hostname) = peer_hostnames.get(&target_node_id).and_then(|h| h.as_ref()) {
                                            span style="color: #059669; margin-left: 4px;" { "(" (hostname) ")" }
                                        }
                                    }
                                }
                            }
                        }

                        div style="display: flex; align-items: center; margin-bottom: 6px; font-size: 0.85em; color: #666;" {
                            span style="margin-right: 6px;" { "🏷️" }
                            span { "Hash: " }
                            code style="background: #f1f5f9; padding: 2px 4px; border-radius: 3px; font-size: 0.8em; word-break: break-all;" {
                                (grouped_secret.hash)
                            }
                        }

                        div style="display: flex; justify-content: space-between; font-size: 0.8em; color: #888;" {
                            span { "Created " (format_relative_time(&grouped_secret.get_created_at_utc())) }
                            span { "Updated " (format_relative_time(&grouped_secret.get_updated_at_utc())) }
                        }
                    }
                }
            }
        }
    }
}

fn tmpl_list_grouped_secrets(
    grouped_secrets: Vec<GroupedSecret>,
    current_node_id: NodeId,
    peers: Vec<Peer>,
) -> Markup {
    layout(html! {
        nav style="margin-bottom: 20px;" {
            a href="/" { "← Home" }
        }

        h1 { "Secrets" }
        (tmpl_grouped_secret_list(&grouped_secrets, current_node_id, &peers))

        h2 { "Add New Secret" }
        div style="margin-top: 20px;" {
            a href="/secrets/new" style="display: inline-block; padding: 10px 20px; background: #2563eb; color: white; text-decoration: none; border-radius: 5px;" {
                "➕ Add Secret"
            }
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

#[handler]
async fn list_secrets() -> Result<Markup> {
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
    ))
}

fn tmpl_secret_detail_grouped(
    secrets: &[Secret],
    current_node_id: NodeId,
    peers: Vec<Peer>,
) -> Markup {
    let secret = &secrets[0]; // All secrets have same name/hash, use first for metadata
    let is_for_current_node = secrets.iter().any(|s| {
        s.get_target_node_id().is_ok() && s.get_target_node_id().unwrap() == current_node_id
    });

    // Create hostname lookup map
    let peer_hostnames: std::collections::HashMap<String, Option<String>> = peers
        .iter()
        .map(|p| (p.node_id.clone(), p.hostname.clone()))
        .collect();

    layout(html! {
        nav style="margin-bottom: 20px;" {
            a href="/secrets" { "← Back to Secrets" }
        }

        h1 { "Secret: " (secret.name) }

        div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 20px; margin-bottom: 20px;" {
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
                            div style="background: #f1f5f9; padding: 8px 12px; border-radius: 4px; border: 1px solid #e2e8f0;" {
                                code { (secret.target_node_id) }
                                @if let Some(hostname) = peer_hostnames.get(&secret.target_node_id).and_then(|h| h.as_ref()) {
                                    span style="color: #059669; margin-left: 8px;" { "(" (hostname) ")" }
                                }
                                @if secret.get_target_node_id().is_ok() && secret.get_target_node_id().unwrap() == current_node_id {
                                    span style="background: #dcfce7; color: #166534; padding: 2px 6px; border-radius: 8px; font-size: 0.7em; margin-left: 8px;" {
                                        "YOU"
                                    }
                                }
                            }
                        }
                    }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Hash" }
                    code style="background: #f1f5f9; padding: 8px; border-radius: 4px; display: block; word-break: break-all;" {
                        (secret.hash)
                    }
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
        }
    })
}

#[allow(dead_code)] // Replaced by tmpl_grouped_secret_detail, kept for reference
fn tmpl_secret_detail(
    secret: &Secret,
    current_node_id: NodeId,
    peer_hostname: Option<String>,
) -> Markup {
    let is_for_current_node = secret.get_target_node_id().is_ok()
        && secret.get_target_node_id().unwrap() == current_node_id;

    layout(html! {
        nav style="margin-bottom: 20px;" {
            a href="/secrets" { "← Back to Secrets" }
        }

        h1 { "Secret: " (secret.name) }

        div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 20px; margin-bottom: 20px;" {
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
        }

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
        .find(|s| {
            s.get_target_node_id().is_ok() && s.get_target_node_id().unwrap() == identity.id()
        })
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
        .find(|s| {
            s.get_target_node_id().is_ok() && s.get_target_node_id().unwrap() == current_node_id
        })
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
    Data(peer_message_tx): Data<&mpsc::Sender<PeerMessage>>,
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
        .find(|s| {
            s.get_target_node_id().is_ok() && s.get_target_node_id().unwrap() == current_node_id
        })
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
        let already_exists = secrets.iter().any(|s| {
            s.get_target_node_id().is_ok() && s.get_target_node_id().unwrap() == target_node_id
        });

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
        if let Err(e) = announce_secret(&new_secret, peer_message_tx.clone()).await {
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

        nav style="margin-bottom: 20px;" {
            a href="/secrets" { "← Back to Secrets" }
        }

        h1 { "Secret: " (secret.name) }

        div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 20px; margin-bottom: 20px;" {
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
                            div style="background: #f1f5f9; padding: 8px 12px; border-radius: 4px; border: 1px solid #e2e8f0;" {
                                code { (secret.target_node_id) }
                                @if let Some(hostname) = peer_hostnames.get(&secret.target_node_id).and_then(|h| h.as_ref()) {
                                    span style="color: #059669; margin-left: 8px;" { "(" (hostname) ")" }
                                }
                                @if secret.get_target_node_id().is_ok() && secret.get_target_node_id().unwrap() == current_node_id {
                                    span style="background: #dcfce7; color: #166534; padding: 2px 6px; border-radius: 8px; font-size: 0.7em; margin-left: 8px;" {
                                        "YOU"
                                    }
                                }
                            }
                        }
                    }
                }

                div {
                    label style="font-weight: bold; color: #374151; display: block; margin-bottom: 4px;" { "Hash" }
                    code style="background: #f1f5f9; padding: 8px; border-radius: 4px; display: block; word-break: break-all;" {
                        (secret.hash)
                    }
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
        }
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
                                @if secret.get_target_node_id().is_ok() && secret.get_target_node_id().unwrap() == current_node_id {
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
        nav style="margin-bottom: 20px;" {
            a href="/secrets" { "← Back to Secrets" }
        }

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
async fn create_secret(
    body: Body,
    Data(peer_message_tx): Data<&mpsc::Sender<PeerMessage>>,
) -> Result<Markup> {
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
    for node_id_str in target_nodes {
        let target_node_id = node_id_str
            .parse::<NodeId>()
            .map_err(|e| AppError::BadRequest(format!("Invalid node ID {node_id_str}: {e}")))?;

        let secret = Secret::create(name.clone(), content_bytes, target_node_id)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create secret: {e}")))?;

        // Announce the secret to the network
        if let Err(e) = announce_secret(&secret, peer_message_tx.clone()).await {
            error!("Failed to announce secret '{}': {}", secret.name, e);
        }

        debug!(
            "Created and announced secret '{}' for node {}",
            secret.name, secret.target_node_id
        );
    }

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
    ))
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

#[handler]
async fn delete_secret(
    poem::web::Path((name, hash)): poem::web::Path<(String, String)>,
    Data(peer_message_tx): Data<&mpsc::Sender<PeerMessage>>,
) -> Result<Markup> {
    let identity = get_current_identity().await?;

    // Parse the target node ID from the hash - we need to find the secret first
    let secrets = Secret::find_by_name_and_hash(&name, &hash)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Find the secret meant for the current node
    let secret = secrets
        .into_iter()
        .find(|s| {
            s.get_target_node_id().is_ok() && s.get_target_node_id().unwrap() == identity.id()
        })
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
        if let Err(e) = announce_secret_deletion(
            name.clone(),
            hash.clone(),
            target_node_id,
            peer_message_tx.clone(),
        )
        .await
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
    ))
}

pub fn create_app(peer_message_tx: mpsc::Sender<PeerMessage>) -> impl Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/peers", get(list_peers).post(create_peer))
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
        .data(peer_message_tx)
        .with(HtmxErrorMiddleware)
}

pub async fn webserver_task(
    mut shutdown_rx: broadcast::Receiver<()>,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> anyhow::Result<()> {
    debug!("WebServer task starting...");

    let app = create_app(peer_message_tx);

    let result = Server::new(TcpListener::bind("0.0.0.0:3000"))
        .run_with_graceful_shutdown(
            app,
            async {
                let _ = shutdown_rx.recv().await;
                debug!("Poem server received shutdown signal");
            },
            Some(std::time::Duration::from_secs(5)),
        )
        .await;

    match result {
        Ok(_) => debug!("Poem server shutdown complete"),
        Err(e) => error!("Poem server error: {}", e),
    }

    debug!("WebServer task stopped");
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
        let _ = crate::db::init_test_db().await;
    }

    fn create_test_app() -> impl Endpoint {
        let (peer_message_tx, _) = mpsc::channel::<PeerMessage>(100);
        create_app(peer_message_tx)
    }

    #[tokio::test]
    async fn test_list_peers_empty() {
        setup_test_db().await;
        let app = create_test_app();
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
        let app = create_test_app();
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
        let app = create_test_app();
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
        let app = create_test_app();
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
        let app = create_test_app();
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
        assert!(body.contains("UNIQUE constraint failed") || body.contains("already exists"));
    }
}
