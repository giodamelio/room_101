use poem::{
    Endpoint, IntoResponse, Request, Response,
    error::ResponseError,
    http::StatusCode,
    middleware::Middleware,
};

use crate::error::AppError;

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

#[cfg(test)]
mod tests {
    use super::*;
    use poem::{
        EndpointExt, handler, get, Route,
        http::StatusCode,
        test::TestClient,
    };
    use crate::error::AppError;

    #[handler]
    async fn success_handler() -> poem::Result<String> {
        Ok("success".to_string())
    }

    #[handler]
    async fn error_handler() -> poem::Result<String> {
        Err(AppError::BadRequest("test error".to_string()).into())
    }

    #[tokio::test]
    async fn test_middleware_passes_through_success() {
        let app = Route::new()
            .at("/success", get(success_handler))
            .with(HtmxErrorMiddleware);

        let client = TestClient::new(app);
        let response = client.get("/success").send().await;
        
        response.assert_status_is_ok();
        response.assert_text("success").await;
    }

    #[tokio::test]
    async fn test_middleware_handles_non_htmx_errors_normally() {
        let app = Route::new()
            .at("/error", get(error_handler))
            .with(HtmxErrorMiddleware);

        let client = TestClient::new(app);
        let response = client.get("/error").send().await;
        
        response.assert_status(StatusCode::BAD_REQUEST);
        response.assert_text("Invalid input: test error").await;
    }

    #[tokio::test]
    async fn test_middleware_handles_htmx_errors_with_headers() {
        let app = Route::new()
            .at("/error", get(error_handler))
            .with(HtmxErrorMiddleware);

        let client = TestClient::new(app);
        let response = client
            .get("/error")
            .header("HX-Request", "true")
            .send()
            .await;
        
        response.assert_status(StatusCode::BAD_REQUEST);
        response.assert_header("HX-Retarget", "#error-message");
        response.assert_header("HX-Reswap", "innerHTML");
        response.assert_text("Invalid input: test error").await;
    }

    #[tokio::test]
    async fn test_middleware_handles_unknown_errors_for_htmx() {
        #[handler]
        async fn unknown_error_handler() -> poem::Result<String> {
            Err(poem::Error::from_string("unknown error", StatusCode::INTERNAL_SERVER_ERROR))
        }

        let app = Route::new()
            .at("/unknown", get(unknown_error_handler))
            .with(HtmxErrorMiddleware);

        let client = TestClient::new(app);
        let response = client
            .get("/unknown")
            .header("HX-Request", "true")
            .send()
            .await;
        
        response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
        response.assert_header("HX-Retarget", "#error-message");
        response.assert_header("HX-Reswap", "innerHTML");
        response.assert_text("An error occurred").await;
    }
}