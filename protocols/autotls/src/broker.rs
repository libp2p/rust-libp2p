use base64::Engine;
use serde::Serialize;

/// Default forge domain under which certificates are issued (`libp2p.direct`).
pub const DEFAULT_FORGE_DOMAIN: &str = "libp2p.direct";
/// Default registration broker endpoint.
pub const DEFAULT_FORGE_ENDPOINT: &str = "https://registration.libp2p.direct";
/// Path of the challenge registration endpoint, relative to the broker endpoint.
pub const CHALLENGE_PATH: &str = "/v1/_acme-challenge";
/// Path of the broker health endpoint (`GET` returns `204`).
pub const HEALTH_PATH: &str = "/v1/health";

/// The JSON body `POST`ed to the broker's `/v1/_acme-challenge` endpoint.
///
/// The field names are lowercase on the wire (the broker rejects unknown fields, and the
/// capitalized form shown in some p2p-forge documentation is stale).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChallengeRequest {
    /// The ACME `dns-01` without padding.
    pub value: String,
    /// The node's publicly dialable transport multiaddrs (no relay/`p2p-circuit`).
    pub addresses: Vec<String>,
}

/// Whether `value` is a valid `dns-01` value: unpadded base64url of a 32-byte SHA-256 digest.
///
/// The broker rejects anything else (padded, standard base64, or not 32 bytes).
pub fn is_valid_dns01_value(value: &str) -> bool {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|digest| digest.len() == 32)
}

#[cfg(feature = "tokio")]
pub use client::{BrokerClient, Error};

#[cfg(feature = "tokio")]
mod client {
    use libp2p_identity::Keypair;
    use reqwest::{
        StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
    };

    use super::{CHALLENGE_PATH, ChallengeRequest};
    use crate::peer_id_auth::{self, PeerIdAuthClient};

    /// Header carrying the optional shared secret for private broker deployments.
    const FORGE_AUTHORIZATION: &str = "Forge-Authorization";

    /// Errors from the registration broker exchange.
    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        /// The HTTP request failed.
        #[error("broker request failed: {0}")]
        Http(#[from] reqwest::Error),
        /// Building or verifying a `libp2p-PeerID` authentication header failed.
        #[error("broker authentication failed: {0}")]
        Auth(#[from] peer_id_auth::Error),
        /// The configured broker endpoint is not a valid URL with a host.
        #[error("invalid broker endpoint")]
        InvalidEndpoint,
        /// The broker did not return a well-formed `WWW-Authenticate` challenge.
        #[error("the broker did not initiate the authentication handshake")]
        MissingChallenge,
        /// The broker failed to prove its identity to us.
        #[error("the broker failed to authenticate")]
        ServerAuthentication,
        /// The broker rejected the registration.
        #[error("the broker rejected the registration: {status}: {body}")]
        Rejected {
            /// The HTTP status code returned by the broker.
            status: StatusCode,
            /// The response body.
            body: String,
        },
    }

    /// Client for the p2p-forge registration broker.
    ///
    /// Registers the ACME `dns-01` challenge value with the broker over the client-initiated
    /// `libp2p-PeerID` authentication handshake, signing with the node's identity key.
    pub struct BrokerClient {
        http: reqwest::Client,
        endpoint: String,
        hostname: String,
        auth: PeerIdAuthClient,
        forge_auth_token: Option<String>,
    }

    impl BrokerClient {
        /// Create a client for the broker at `endpoint`, authenticating with `keypair`.
        pub fn new(
            endpoint: String,
            keypair: Keypair,
            forge_auth_token: Option<String>,
        ) -> Result<Self, Error> {
            let hostname = reqwest::Url::parse(&endpoint)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .ok_or(Error::InvalidEndpoint)?;
            Ok(Self {
                http: reqwest::Client::builder().build()?,
                endpoint,
                hostname,
                auth: PeerIdAuthClient::new(keypair),
                forge_auth_token,
            })
        }

        /// Register the `dns-01` `value` and the node's public `addresses` with the broker.
        pub async fn register(&self, value: &str, addresses: &[String]) -> Result<(), Error> {
            let url = format!("{}{CHALLENGE_PATH}", self.endpoint);

            let challenge_server = PeerIdAuthClient::generate_challenge();
            let public_key = self.auth.public_key_param();
            let opening = peer_id_auth::build_auth_header(&[
                ("challenge-server", challenge_server.as_str()),
                ("public-key", public_key.as_str()),
            ]);
            let response = self
                .http
                .post(&url)
                .header(AUTHORIZATION, opening)
                .send()
                .await?;
            if response.status() != StatusCode::UNAUTHORIZED {
                return Err(rejected(response).await);
            }

            let params = response
                .headers()
                .get(WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .and_then(peer_id_auth::parse_auth_params)
                .ok_or(Error::MissingChallenge)?;
            let (Some(challenge_client), Some(server_key), Some(server_sig), Some(opaque)) = (
                params.get("challenge-client"),
                params.get("public-key"),
                params.get("sig"),
                params.get("opaque"),
            ) else {
                return Err(Error::MissingChallenge);
            };
            let server_public_key = peer_id_auth::decode_public_key(server_key)?;
            if !self.auth.verify_server(
                &challenge_server,
                &server_public_key,
                &self.hostname,
                server_sig,
            )? {
                return Err(Error::ServerAuthentication);
            }

            let signature =
                self.auth
                    .sign_challenge(challenge_client, &server_public_key, &self.hostname)?;
            let authorization = peer_id_auth::build_auth_header(&[
                ("opaque", opaque.as_str()),
                ("sig", signature.as_str()),
            ]);
            let body = ChallengeRequest {
                value: value.to_owned(),
                addresses: addresses.to_vec(),
            };
            let mut request = self
                .http
                .post(&url)
                .header(AUTHORIZATION, authorization)
                .json(&body);
            if let Some(token) = &self.forge_auth_token {
                request = request.header(FORGE_AUTHORIZATION, token);
            }
            let response = request.send().await?;
            if response.status() != StatusCode::OK {
                return Err(rejected(response).await);
            }
            Ok(())
        }
    }

    async fn rejected(response: reqwest::Response) -> Error {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Error::Rejected { status, body }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns01_value_validation() {
        let valid = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 32]);
        assert!(is_valid_dns01_value(&valid));

        let padded = base64::engine::general_purpose::URL_SAFE.encode([0u8; 32]);
        assert!(padded.ends_with('='));
        assert!(!is_valid_dns01_value(&padded));
        assert!(!is_valid_dns01_value(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 31])
        ));
        assert!(!is_valid_dns01_value("not base64!"));
    }
}
