use anyhow::Context;
use iroh::NodeId;
use maud::{DOCTYPE, Markup, html};
use poem::{
    Endpoint, EndpointExt, IntoResponse, Request, Response, Route, Server,
    error::ResponseError,
    get, handler,
    http::StatusCode,
    listener::TcpListener,
    middleware::Middleware,
    web::{Data, Form},
};
use serde::Deserialize;
use tokio::sync::broadcast;
use tracing::info;

use crate::db::{DB, Peer};
use crate::error::{AppError, Result};

pub struct HtmxErrorMiddleware;

impl<E: Endpoint> Middleware<E> for HtmxErrorMiddleware {
    type Output = HtmxErrorEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        HtmxErrorEndpoint { inner: ep }
    }
}

pub struct HtmxErrorEndpoint<E> {
    inner: E,
}

impl<E: Endpoint> Endpoint for HtmxErrorEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: Request) -> poem::Result<Self::Output> {
        let is_htmx = req.headers().get("hx-request").is_some();

        match self.inner.call(req).await {
            Ok(resp) => Ok(resp.into_response()),
            Err(err) => {
                if is_htmx {
                    if let Some(app_error) = err.downcast_ref::<AppError>() {
                        Ok(Response::builder()
                            .status(app_error.status())
                            .header("content-type", "text/html")
                            .header("HX-Retarget", "#error-message")
                            .header("HX-Reswap", "innerHTML")
                            .body(app_error.to_string()))
                    } else {
                        Ok(Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header("content-type", "text/html")
                            .header("HX-Retarget", "#error-message")
                            .header("HX-Reswap", "innerHTML")
                            .body("An error occurred".to_string()))
                    }
                } else {
                    Err(err)
                }
            }
        }
    }
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

pub async fn task(shutdown_tx: broadcast::Sender<()>, db: DB) -> anyhow::Result<()> {
    let app = Route::new()
        .at("/peers", get(list_peers).post(create_peer))
        .with(HtmxErrorMiddleware)
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
