use maud::{Markup, html};

pub fn button_link(text: &str, href: &str, color: &str) -> Markup {
    html! {
        a href=(href) style={"display: inline-block; padding: 10px 20px; background: " (color) "; color: white; text-decoration: none; border-radius: 5px;"} {
            (text)
        }
    }
}

// Top navbar component
pub fn navbar(
    current_page: &str,
    peer_count: Option<usize>,
    secret_count: Option<usize>,
    node_id: Option<&str>,
    hostname: Option<&str>,
) -> Markup {
    html! {
        nav style="background: #1f2937; border-bottom: 1px solid #374151; margin-bottom: 20px;" {
            div style="max-width: 1200px; margin: 0 auto; padding: 0 20px;" {
                div style="display: flex; justify-content: space-between; align-items: center; height: 60px;" {
                    // Left side - Brand and main navigation
                    div style="display: flex; align-items: center; gap: 32px;" {
                        a href="/" style="color: #f9fafb; text-decoration: none; font-size: 1.5em; font-weight: bold;" {
                            "Room 101"
                        }

                        div style="display: flex; gap: 24px;" {
                            a href="/peers"
                              style=(format!("color: {}; text-decoration: none; padding: 8px 16px; border-radius: 4px; display: inline-flex; align-items: center; gap: 6px; {}",
                                     if current_page == "peers" { "#3b82f6" } else { "#d1d5db" },
                                     if current_page == "peers" { "background: #1e3a8a;" } else { "" }))
                            {
                                span { "📡" }
                                span { "Peers" }
                            }
                            a href="/secrets"
                              style=(format!("color: {}; text-decoration: none; padding: 8px 16px; border-radius: 4px; display: inline-flex; align-items: center; gap: 6px; {}",
                                     if current_page == "secrets" { "#3b82f6" } else { "#d1d5db" },
                                     if current_page == "secrets" { "background: #1e3a8a;" } else { "" }))
                            {
                                span { "🔐" }
                                span { "Secrets" }
                            }
                            a href="/events"
                              style=(format!("color: {}; text-decoration: none; padding: 8px 16px; border-radius: 4px; display: inline-flex; align-items: center; gap: 6px; {}",
                                     if current_page == "events" { "#3b82f6" } else { "#d1d5db" },
                                     if current_page == "events" { "background: #1e3a8a;" } else { "" }))
                            {
                                span { "📋" }
                                span { "Events" }
                            }
                        }
                    }

                    // Right side - Status info
                    div style="color: #d1d5db; font-size: 0.9em;" {
                        @if let (Some(peers), Some(secrets), Some(node)) = (peer_count, secret_count, node_id) {
                            (status_bar(peers, secrets, node, hostname))
                        } @else {
                            span { "Loading..." }
                        }
                    }
                }
            }
        }
    }
}

// Navigation breadcrumb component
pub fn nav_breadcrumb(href: &str, text: &str) -> Markup {
    html! {
        nav style="margin-bottom: 20px;" {
            a href=(href) { "← " (text) }
        }
    }
}

// Card container component
pub fn card_container(content: Markup, _padding: Option<&str>) -> Markup {
    html! {
        div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 20px; margin-bottom: 20px;" {
            (content)
        }
    }
}

// List item card (for peer/secret lists)
pub fn list_item_card(content: Markup) -> Markup {
    html! {
        div style="background: white; border: 1px solid #ddd; border-radius: 8px; padding: 16px; box-shadow: 0 2px 4px rgba(0,0,0,0.1);" {
            (content)
        }
    }
}

// Empty state component
pub fn empty_state(icon: &str, title: &str, description: &str) -> Markup {
    html! {
        div style="text-align: center; padding: 40px; background: white; border-radius: 8px; border: 1px solid #ddd;" {
            div style="font-size: 3em; margin-bottom: 16px; color: #999;" { (icon) }
            h3 style="margin: 0 0 8px 0; color: #666;" { (title) }
            p style="margin: 0; color: #888;" { (description) }
        }
    }
}

// Status bar component showing peer and secret counts plus node info
pub fn status_bar(
    peer_count: usize,
    secret_count: usize,
    node_id: &str,
    hostname: Option<&str>,
) -> Markup {
    // Get first 8 characters of node ID for display
    let short_node_id = if node_id.len() > 8 {
        &node_id[..8]
    } else {
        node_id
    };

    html! {
        div style="display: flex; gap: 16px; align-items: center;" {
            // Node info
            div style="display: flex; align-items: center; gap: 4px;" {
                span style="color: #f59e0b;" { "🏠" }
                span {
                    @if let Some(host) = hostname {
                        (host)
                    } @else {
                        "unknown"
                    }
                    span style="color: #9ca3af; font-family: monospace; margin-left: 4px;" {
                        "(" (short_node_id) "...)"
                    }
                }
            }
            // Peer count
            div style="display: flex; align-items: center; gap: 4px;" {
                span style="color: #22c55e;" { "●" }
                span { (peer_count) " peers" }
            }
            // Secret count
            div style="display: flex; align-items: center; gap: 4px;" {
                span style="color: #3b82f6;" { "🔐" }
                span { (secret_count) " secrets" }
            }
        }
    }
}

// Enhanced layout component with navbar
pub fn layout_with_navbar(
    content: Markup,
    current_page: &str,
    peer_count: Option<usize>,
    secret_count: Option<usize>,
    node_id: Option<&str>,
    hostname: Option<&str>,
) -> Markup {
    html! {
        (maud::DOCTYPE)
        html {
            head {
                meta name="htmx-config" content=r#"{"responseHandling":[{"code":".*", "swap": true}]}"#;
                script src="https://cdn.jsdelivr.net/npm/htmx.org@2.0.6/dist/htmx.min.js" {};
                title { "Room 101" }
            }
            body style="font-family: Arial, sans-serif; margin: 0; padding: 0; background-color: #f5f5f5;" {
                (navbar(current_page, peer_count, secret_count, node_id, hostname))

                div style="max-width: 1200px; margin: 0 auto; padding: 0 20px;" {
                    (content)
                }
            }
        }
    }
}
