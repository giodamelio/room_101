use maud::{Markup, html};
use miette::{IntoDiagnostic, Result};
use poem::{Route, Server, get, handler, listener::TcpListener, web::Path};
use tokio::sync::broadcast;
use tracing::info;

#[handler]
fn hello(Path(name): Path<String>) -> Markup {
    html! {
        h1 { "Hello World!" }
        p { "hello " (name) }
    }
}

pub async fn task(shutdown_tx: broadcast::Sender<()>) -> Result<()> {
    let app = Route::new().at("/hello/:name", get(hello));

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
        .into_diagnostic()?;

    Ok(())
}
