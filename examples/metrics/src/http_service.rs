// Copyright 2022 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

const METRICS_CONTENT_TYPE: &str = "application/openmetrics-text;charset=utf-8;version=1.0.0";

pub(crate) async fn metrics_server(registry: Registry) -> Result<(), std::io::Error> {
    // Serve on localhost.
    let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
    let service = MetricService::new(registry);
    let server = Router::new()
        .route("/metrics", get(respond_with_metrics))
        .fallback(respond_with_404_not_found)
        .with_state(service);
    let tcp_listener = TcpListener::bind(addr).await.unwrap();
    let local_addr = tcp_listener.local_addr()?;
    tracing::info!(metrics_server=%format!("http://{}/metrics", local_addr));
    axum::serve(tcp_listener, server.into_make_service())
        .await
        .unwrap();
    Ok(())
}

#[derive(Clone)]
pub(crate) struct MetricService {
    reg: Arc<Mutex<Registry>>,
}

async fn respond_with_metrics(state: State<MetricService>) -> Response<String> {
    state.respond_with_metrics()
}

async fn respond_with_404_not_found(state: State<MetricService>) -> Response<String> {
    state.respond_with_404_not_found()
}

type SharedRegistry = Arc<Mutex<Registry>>;

impl MetricService {
    fn new(registry: Registry) -> Self {
        Self {
            reg: Arc::new(Mutex::new(registry)),
        }
    }

    fn get_reg(&self) -> SharedRegistry {
        Arc::clone(&self.reg)
    }
    fn respond_with_metrics(&self) -> Response<String> {
        let mut response: Response<String> = Response::default();

        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            METRICS_CONTENT_TYPE.try_into().unwrap(),
        );

        let reg = self.get_reg();
        encode(&mut response.body_mut(), &reg.lock().unwrap()).unwrap();

        *response.status_mut() = StatusCode::OK;

        response
    }
    fn respond_with_404_not_found(&self) -> Response<String> {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body("Not found try localhost:[port]/metrics".to_string())
            .unwrap()
    }
}
