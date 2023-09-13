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

use hyper::http::StatusCode;
use hyper::service::Service;
use hyper::{Body, Method, Request, Response, Server};
use log::info;
use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

const METRICS_CONTENT_TYPE: &str = "application/openmetrics-text;charset=utf-8;version=1.0.0";
pub(crate) async fn metrics_server(
    registry: Registry,
    metrics_path: String,
) -> Result<(), hyper::Error> {
    // Serve on localhost.
    let addr = ([0, 0, 0, 0], 8888).into();

    let server = Server::bind(&addr).serve(MakeMetricService::new(registry, metrics_path.clone()));
    info!(
        "Metrics server on http://{}{}",
        server.local_addr(),
        metrics_path
    );
    server.await?;
    Ok(())
}
pub(crate) struct MetricService {
    reg: Arc<Mutex<Registry>>,
    metrics_path: String,
}

type SharedRegistry = Arc<Mutex<Registry>>;

impl MetricService {
    fn get_reg(&mut self) -> SharedRegistry {
        Arc::clone(&self.reg)
    }
    fn respond_with_metrics(&mut self) -> Response<String> {
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
    fn respond_with_404_not_found(&mut self) -> Response<String> {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(format!(
                "Not found try localhost:[port]/{}",
                self.metrics_path
            ))
            .unwrap()
    }
}

impl Service<Request<Body>> for MetricService {
    type Response = Response<String>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
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

pub(crate) struct MakeMetricService {
    reg: SharedRegistry,
    metrics_path: String,
}

impl MakeMetricService {
    pub(crate) fn new(registry: Registry, metrics_path: String) -> MakeMetricService {
        MakeMetricService {
            reg: Arc::new(Mutex::new(registry)),
            metrics_path,
        }
    }
}

impl<T> Service<T> for MakeMetricService {
    type Response = MetricService;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        let reg = self.reg.clone();
        let metrics_path = self.metrics_path.clone();
        let fut = async move { Ok(MetricService { reg, metrics_path }) };
        Box::pin(fut)
    }
}
