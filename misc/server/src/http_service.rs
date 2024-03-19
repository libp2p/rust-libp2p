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

use hyper::body::Incoming;
use hyper::http::StatusCode;
use hyper::server::conn::http1::Builder;
use hyper::service::Service;
use hyper::{Method, Request, Response};
use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

const METRICS_CONTENT_TYPE: &str = "application/openmetrics-text;charset=utf-8;version=1.0.0";
pub(crate) async fn metrics_server(
    registry: Registry,
    metrics_path: String,
) -> Result<(), std::io::Error> {
    // Serve on localhost.
    let addr: SocketAddr = ([0, 0, 0, 0], 8888).into();
    let tcp_listener_stream = TcpListener::bind(addr).await?;
    let local_addr = tcp_listener_stream.local_addr()?;
    let service = MetricService::new(registry, metrics_path.clone());
    tracing::info!(metrics_server=%format!("http://{}{}", local_addr, metrics_path));
    loop {
        let service = service.clone();
        let (connection, _) = tcp_listener_stream.accept().await?;
        tokio::spawn(async move {
            let server =
                Builder::new().serve_connection(hyper_util::rt::TokioIo::new(connection), service);
            if let Err(e) = server.await {
                tracing::error!("server error: {}", e);
            }
        });
    }
}

#[derive(Clone)]
pub(crate) struct MetricService {
    reg: Arc<Mutex<Registry>>,
    metrics_path: String,
}

type SharedRegistry = Arc<Mutex<Registry>>;

impl MetricService {
    fn new(reg: Registry, metrics_path: String) -> Self {
        Self {
            reg: Arc::new(Mutex::new(reg)),
            metrics_path,
        }
    }

    fn get_reg(&self) -> SharedRegistry {
        Arc::clone(&self.reg)
    }
    fn respond_with_metrics(&self) -> Response<String> {
        let mut response: Response<String> = Response::default();

        response.headers_mut().insert(
            hyper::header::CONTENT_TYPE,
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
            .body(format!(
                "Not found try localhost:[port]/{}",
                self.metrics_path
            ))
            .unwrap()
    }
}

impl Service<Request<Incoming>> for MetricService {
    type Response = Response<String>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let req_path = req.uri().path();
        let req_method = req.method();
        let resp = if (req_method == Method::GET) && (req_path == self.metrics_path) {
            // Encode and serve metrics from registry.
            self.respond_with_metrics()
        } else {
            self.respond_with_404_not_found()
        };
        Box::pin(async { Ok(resp) })
    }
}

// pub(crate) struct MakeMetricService {
//     reg: SharedRegistry,
//     metrics_path: String,
// }

// impl MakeMetricService {
//     pub(crate) fn new(registry: Registry, metrics_path: String) -> MakeMetricService {
//         MakeMetricService {
//             reg: Arc::new(Mutex::new(registry)),
//             metrics_path,
//         }
//     }
// }

// impl<T> Service<T> for MakeMetricService {
//     type Response = MetricService;
//     type Error = hyper::Error;
//     type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

//     fn call(&self, _: T) -> Self::Future {
//         let reg = self.reg.clone();
//         let metrics_path = self.metrics_path.clone();
//         let fut = async move { Ok(MetricService { reg, metrics_path }) };
//         Box::pin(fut)
//     }
// }
