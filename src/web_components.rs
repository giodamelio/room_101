use maud::{Markup, html};

pub fn button_link(text: &str, href: &str, _color: &str) -> Markup {
    html! {
        a href=(href) role="button" {
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
        nav {
            ul {
                li {
                    a href="/" class="navbar-brand" {
                        "Room 101"
                    }
                }
            }
            ul {
                li {
                    a href="/peers"
                      role=(if current_page == "peers" { "button" } else { "" })
                      class=(if current_page == "peers" { "contrast" } else { "" })
                    {
                        span { "📡" }
                        " Peers"
                    }
                }
                li {
                    a href="/secrets"
                      role=(if current_page == "secrets" { "button" } else { "" })
                      class=(if current_page == "secrets" { "contrast" } else { "" })
                    {
                        span { "🔐" }
                        " Secrets"
                    }
                }
                li {
                    a href="/events"
                      role=(if current_page == "events" { "button" } else { "" })
                      class=(if current_page == "events" { "contrast" } else { "" })
                    {
                        span { "📋" }
                        " Events"
                    }
                }
                li {
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

// Navigation breadcrumb component
pub fn nav_breadcrumb(href: &str, text: &str) -> Markup {
    html! {
        nav aria-label="breadcrumb" {
            ul {
                li {
                    a href=(href) { "← " (text) }
                }
            }
        }
    }
}

// Card container component
pub fn card_container(content: Markup, _padding: Option<&str>) -> Markup {
    html! {
        article {
            (content)
        }
    }
}

// List item card (for peer/secret lists)
pub fn list_item_card(content: Markup) -> Markup {
    html! {
        article {
            (content)
        }
    }
}

// Empty state component
pub fn empty_state(icon: &str, title: &str, description: &str) -> Markup {
    html! {
        article style="text-align: center;" {
            div style="font-size: 3em; margin-bottom: 1rem;" { (icon) }
            h3 { (title) }
            p { (description) }
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
        small {
            span class="status-indicator node" {
                span { "🏠" }
                span {
                    @if let Some(host) = hostname {
                        (host)
                    } @else {
                        "unknown"
                    }
                    span class="node-id" {
                        " (" (short_node_id) "...)"
                    }
                }
            }
            " | "
            span class="status-indicator online" {
                span { "●" }
                " " (peer_count) " peers"
            }
            " | "
            span class="status-indicator secret" {
                span { "🔐" }
                " " (secret_count) " secrets"
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
                link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/@picocss/pico@2/css/pico.min.css";
                style { (include_str!("../styles.css")) }
                title { "Room 101" }
            }
            body {
                (navbar(current_page, peer_count, secret_count, node_id, hostname))

                main class="container" {
                    (content)
                }
            }
        }
    }
}
