// Copyright 2021 Protocol Labs.
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

//! Example demonstrating `libp2p-metrics`.
//!
//! In one terminal run:
//!
//! ```
//! RUST_LOG=info cargo run --example metrics
//! ```
//!
//! In a second terminal run:
//!
//! ```
//! RUST_LOG=info cargo run --example metrics -- <listen-addr-of-first-node>
//! ```
//!
//! Where `<listen-addr-of-first-node>` is replaced by the listen address of the
//! first node reported in the first terminal. Look for `NewListenAddr`.
//!
//! In a third terminal run:
//!
//! ```
//! curl localhost:<metrics-port-of-first-or-second-node>/metrics
//! ```
//!
//! Where `<metrics-port-of-first-or-second-node>` is replaced by the listen
//! port of the metrics server of the first or the second node. Look for
//! `tide::server Server listening on`.
//!
//! You should see a long list of metrics printed to the terminal. Check the
//! `libp2p_ping` metrics, they should be `>0`.

use futures::executor::block_on;
use futures::stream::StreamExt;
use libp2p::core::Multiaddr;
use libp2p::metrics::{Metrics, Recorder};
use libp2p::ping::{Ping, PingConfig};
use libp2p::swarm::SwarmEvent;
use libp2p::{identity, PeerId, Swarm};
use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::thread;
use log::{error,info,debug};
use hyper::service::Service;
use hyper::{Body, Request, Response, Server};
use hyper::http::{StatusCode};


//fn main() -> Result<(), Box<dyn Error>> {

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    env_logger::init();

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    info!("Local peer id: {:?}", local_peer_id);

    let mut swarm = Swarm::new(
        block_on(libp2p::development_transport(local_key))?,
        Ping::new(PingConfig::new().with_keep_alive(true)),
        local_peer_id,
    );

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse()?;
        swarm.dial(remote)?;
        info!("Dialed {}", addr)
    }

    let mut metric_registry = Registry::default();
    let metrics = Metrics::new(&mut metric_registry);
    thread::spawn(move || block_on(metrics_server(metric_registry)));

    block_on(async {
        loop {
            match swarm.select_next_some().await {
                SwarmEvent::Behaviour(ping_event) => {
                    info!("{:?}", ping_event);
                    metrics.record(&ping_event);
                }
                swarm_event => {
                    info!("{:?}", swarm_event);
                    metrics.record(&swarm_event);
                }
            }
        }
    })
}
pub async fn metrics_server(registry: Registry) -> Result<(), std::io::Error> {
    //TODO: Change to a variable port for multiple instances
    debug!("entered  Metrics server line 116");
    let addr = ([127, 0, 0, 1], 3001).into();
    //TODO: Solve type problems
    let server = Server::bind(&addr).serve(MakeMetricService::new(registry));
    info!("Metrics server on http://{}", addr);
    if let Err(e) = server.await {
        error!("server error: {}", e);
    }

    Ok(())
}

struct MetricService{
    reg: Arc<Mutex<Registry>>,
}

type SharedRegistry = Arc<Mutex<Registry>>;

impl MetricService {
    //note that this ususally handled by MakeMetricsService
    //by cloning the Arc stored in that struct
    //thus all intances have Arc pointers to the same registry
    // fn new(registry: Registry)->MetricService {
    //     MetricService {
    //         reg: Arc::new(Mutex::new(registry)),
    //     }
    // }
    //HUM: Directly reference or use get and clone?
    fn get_reg(&mut self)  -> SharedRegistry{
         Arc::clone(&self.reg)
    }
    fn respond_with_metrics(&mut self, req: Request<Body>)
        -> Response<Body> {
        let mut encoded : Vec<u8> = Vec::new();
        let reg = self.get_reg();
        encode(&mut encoded, &reg.lock().unwrap()).unwrap();
        let metrics_content_type =
          "application/openmetrics-text; version=1.0.0; charset=utf-8";
        Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE,metrics_content_type)
            .body(Body::from(encoded)).unwrap()
    }
    fn respond_with_404_not_found(&mut self) -> Response<Body> {
        Response::builder()
          .status(StatusCode::NOT_FOUND)
          .body(Body::from("Not found try localhost:[port]/metrics")).unwrap()
    }
}

impl Service<Request<Body>> for MetricService {
    type Response = Response<Body>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let resp = 
        if (req.method() == & hyper::Method::GET) &&
            (req.uri().path() == "/metrics") {
            //encode and serve meterics from registry
            self.respond_with_metrics(req)
        }
        else {
            self.respond_with_404_not_found()
        };
        Box::pin(async { Ok(resp)})
    }
}
struct MakeMetricService {
    reg: SharedRegistry,
}
impl MakeMetricService{
    pub fn new(registry:Registry) -> MakeMetricService{
        MakeMetricService{
            reg:Arc::new(Mutex::new(registry)),
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
        let fut = async move { Ok(MetricService { reg }) };
        Box::pin(fut)
    }
}
