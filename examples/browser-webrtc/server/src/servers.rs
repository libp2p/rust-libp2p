use axum::{
    body::{boxed, Body, BoxBody},
    http::Method,
    http::{Request, Response, StatusCode, Uri},
    routing::get,
    Router,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tower::ServiceExt;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

pub const STATIC_PORT: u16 = 8080;
pub const MA_PORT: u16 = 4455;

/// Serve the Multiaddr we are listening on
pub(crate) async fn serve_multiaddr(address: String) {
    let multiaddr_server = Router::new().route("/", get(|| async { address })).layer(
        // allow cors
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET]),
    );

    axum::Server::bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), MA_PORT))
        .serve(multiaddr_server.into_make_service())
        .await
        .unwrap();
}

/// Serve the static files (index.html)
pub(crate) async fn serve_files() {
    let app = Router::new().nest_service("/", get(handler));

    let addr = SocketAddr::from((IpAddr::V4(Ipv4Addr::LOCALHOST), STATIC_PORT));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn handler(uri: Uri) -> Result<Response<BoxBody>, (StatusCode, String)> {
    let res = get_static_file(uri.clone()).await?;

    if res.status() == StatusCode::NOT_FOUND {
        // try with `.html`
        // TODO: handle if the Uri has query parameters
        match format!("{}.html", uri).parse() {
            Ok(uri_html) => get_static_file(uri_html).await,
            Err(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, "Invalid URI".to_string())),
        }
    } else {
        Ok(res)
    }
}

async fn get_static_file(uri: Uri) -> Result<Response<BoxBody>, (StatusCode, String)> {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();

    log::info!("Serving static file: {}", req.uri());

    // `ServeDir` implements `tower::Service` so we can call it with `tower::ServiceExt::oneshot`
    match ServeDir::new("../client").oneshot(req).await {
        Ok(res) => Ok(res.map(boxed)),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", err),
        )),
    }
}
