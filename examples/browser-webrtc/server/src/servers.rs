use axum::{
    body::{boxed, Body, BoxBody},
    http::Method,
    http::{Request, Response, Uri},
    routing::get,
    Router,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tower::ServiceExt;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

const PORT: u16 = 8080;

/// Serve the Multiaddr we are listening on and the host files.
pub(crate) async fn serve(address: String) {
    let server = Router::new()
        .route("/address", get(|| async { address }))
        .nest_service("/", get(get_static_file))
        .layer(
            // allow cors
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET]),
        );

    axum::Server::bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), PORT))
        .serve(server.into_make_service())
        .await
        .unwrap();
}

async fn get_static_file(uri: Uri) -> Response<BoxBody> {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();

    log::info!("Serving static file: {}", req.uri());

    // `ServeDir` implements `tower::Service` so we can call it with `tower::ServiceExt::oneshot`
    match ServeDir::new("../client").oneshot(req).await {
        Ok(res) => res.map(boxed),
        Err(err) => match err {},
    }
}
