// Copyright 2021 David Craven.
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

use libp2p_core::identity::Keypair;
use quinn_proto::TransportConfig;
use std::sync::Arc;

pub struct CryptoConfig {
    pub keypair: Keypair,
    pub keylogger: Option<Arc<dyn rustls::KeyLog>>,
    pub transport: Arc<TransportConfig>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TlsCrypto;

impl TlsCrypto {
    pub fn new_server_config(config: &CryptoConfig) -> Arc<rustls::ServerConfig> {
        let mut server = crate::tls::make_server_config(&config.keypair).expect("invalid config");
        if let Some(key_log) = config.keylogger.clone() {
            server.key_log = key_log;
        }
        Arc::new(server)
    }

    pub fn new_client_config(config: &CryptoConfig) -> Arc<rustls::ClientConfig> {
        let mut client = crate::tls::make_client_config(&config.keypair).expect("invalid config");
        if let Some(key_log) = config.keylogger.clone() {
            client.key_log = key_log;
        }
        Arc::new(client)
    }

    pub fn keylogger() -> Arc<dyn rustls::KeyLog> {
        Arc::new(rustls::KeyLogFile::new())
    }
}
