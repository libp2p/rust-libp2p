use std::time::Duration;

use hickory_resolver::{
    TokioResolver,
    config::{CLOUDFLARE, ResolverConfig},
    net::runtime::TokioRuntimeProvider,
    proto::rr::RData,
};
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
use libp2p_identity::Keypair;

use crate::{
    broker::{self, BrokerClient, DEFAULT_FORGE_DOMAIN, DEFAULT_FORGE_ENDPOINT},
    cert::{self, CertKey},
    encoding,
    storage::{CertStore, StoredCertificate},
};

/// How long to wait for the `dns-01` `TXT` record to become visible before giving up.
const DNS_PROPAGATION_TIMEOUT: Duration = Duration::from_secs(180);

/// Configuration for the ACME issuance flow.
#[derive(Debug, Clone)]
pub struct AcmeConfig {
    /// The ACME directory URL.
    pub directory_url: String,
    /// Optional contact email registered with the ACME account.
    pub contact_email: Option<String>,
    /// The forge domain under which the certificate is issued.
    pub forge_domain: String,
    /// The registration broker endpoint
    pub forge_endpoint: String,
    /// Optional shared secret for private broker deployments (`Forge-Authorization`).
    pub forge_auth_token: Option<String>,
    /// Install a process-default `rustls` crypto provider if none is set; the ACME client needs
    /// one. Set `false` if you install your own.
    pub install_crypto_provider: bool,
}

impl AcmeConfig {
    /// Configuration against the Let's Encrypt production CA and the public forge broker.
    pub fn production() -> Self {
        Self::with_directory(LetsEncrypt::Production.url())
    }

    /// Configuration against the Let's Encrypt staging CA (untrusted certs, high rate limits).
    pub fn staging() -> Self {
        Self::with_directory(LetsEncrypt::Staging.url())
    }

    fn with_directory(directory_url: &str) -> Self {
        Self {
            directory_url: directory_url.to_owned(),
            contact_email: None,
            forge_domain: DEFAULT_FORGE_DOMAIN.to_owned(),
            forge_endpoint: DEFAULT_FORGE_ENDPOINT.to_owned(),
            forge_auth_token: None,
            install_crypto_provider: true,
        }
    }
}

/// A certificate obtained from the ACME flow.
#[derive(Debug, Clone)]
pub struct ObtainedCertificate {
    /// The PEM-encoded certificate chain.
    pub chain_pem: String,
    /// The PKCS#8 PEM-encoded certificate private key.
    pub key_pem: String,
    /// The Unix timestamp (seconds) at which the certificate expires.
    pub not_after_unix: i64,
}

/// Errors from the issuance flow.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The ACME protocol exchange failed.
    #[error("acme error: {0}")]
    Acme(#[from] instant_acme::Error),
    /// The broker registration failed.
    #[error("broker error: {0}")]
    Broker(#[from] broker::Error),
    /// Handling certificate material failed.
    #[error("certificate error: {0}")]
    Cert(#[from] cert::Error),
    /// Reading or writing storage failed.
    #[error("storage error: {0}")]
    Io(#[from] std::io::Error),
    /// (De)serializing the ACME account credentials failed.
    #[error("account credentials error: {0}")]
    Credentials(#[from] serde_json::Error),
    /// The order contained no usable `dns-01` authorization.
    #[error("the order offered no dns-01 challenge")]
    NoDns01Challenge,
    /// The order did not reach the `ready` state after the challenge was validated.
    #[error("the acme order did not become ready")]
    OrderNotReady,
    /// The `dns-01` `TXT` record did not propagate within the timeout.
    #[error("timed out waiting for the dns-01 TXT record to propagate")]
    DnsPropagationTimeout,
    /// The DNS resolver could not be constructed.
    #[error("failed to set up the DNS resolver")]
    ResolverSetup,
}

/// Obtain a wildcard certificate for the node, persisting the account and certificate via `store`.
///
/// `public_addresses` are the node's publicly dialable transport multiaddrs that the
/// broker dials to verify reachability before publishing the challenge.
pub async fn obtain_certificate<S: CertStore>(
    config: &AcmeConfig,
    identity_keypair: &Keypair,
    public_addresses: &[String],
    store: &S,
) -> Result<ObtainedCertificate, Error> {
    let peer_id = identity_keypair.public().to_peer_id();
    let label = encoding::peer_id_label(peer_id);
    let domain = format!("*.{label}.{}", config.forge_domain);
    let challenge_domain = format!("_acme-challenge.{label}.{}", config.forge_domain);

    let account = load_or_create_account(config, store).await?;
    let mut order = account
        .new_order(&NewOrder::new(&[Identifier::Dns(domain.clone())]))
        .await?;

    let cert_key = CertKey::generate()?;
    let broker = BrokerClient::new(
        config.forge_endpoint.clone(),
        identity_keypair.clone(),
        config.forge_auth_token.clone(),
    )?;

    {
        let mut authorizations = order.authorizations();
        while let Some(authorization) = authorizations.next().await {
            let mut authorization = authorization?;
            if matches!(authorization.status, AuthorizationStatus::Valid) {
                continue;
            }
            let mut challenge = authorization
                .challenge(ChallengeType::Dns01)
                .ok_or(Error::NoDns01Challenge)?;
            let value = challenge.key_authorization().dns_value();

            broker.register(&value, public_addresses).await?;
            wait_for_dns_txt(&challenge_domain, &value, DNS_PROPAGATION_TIMEOUT).await?;

            challenge.set_ready().await?;
        }
    }

    if !matches!(
        order.poll_ready(&RetryPolicy::default()).await?,
        OrderStatus::Ready
    ) {
        return Err(Error::OrderNotReady);
    }

    let csr = cert_key.certificate_signing_request(&domain)?;
    order.finalize_csr(&csr).await?;
    let chain_pem = order.poll_certificate(&RetryPolicy::default()).await?;

    let key_pem = cert_key.to_pkcs8_pem();
    let not_after_unix = cert::not_after_unix(&chain_pem)?;
    store
        .store_certificate(&StoredCertificate {
            chain_pem: chain_pem.clone(),
            key_pem: key_pem.clone(),
        })
        .await?;

    Ok(ObtainedCertificate {
        chain_pem,
        key_pem,
        not_after_unix,
    })
}

async fn load_or_create_account<S: CertStore>(
    config: &AcmeConfig,
    store: &S,
) -> Result<Account, Error> {
    if let Some(stored) = store.load_account().await? {
        let credentials: AccountCredentials = serde_json::from_str(&stored)?;
        return Ok(Account::builder()?.from_credentials(credentials).await?);
    }

    let contacts: Vec<String> = config
        .contact_email
        .iter()
        .map(|email| format!("mailto:{email}"))
        .collect();
    let contacts: Vec<&str> = contacts.iter().map(String::as_str).collect();
    let new_account = NewAccount {
        contact: &contacts,
        terms_of_service_agreed: true,
        only_return_existing: false,
    };
    let (account, credentials) = Account::builder()?
        .create(&new_account, config.directory_url.clone(), None)
        .await?;
    store
        .store_account(&serde_json::to_string(&credentials)?)
        .await?;
    Ok(account)
}

async fn wait_for_dns_txt(name: &str, expected: &str, timeout: Duration) -> Result<(), Error> {
    // TODO: support other resolvers.
    let resolver = TokioResolver::builder_with_config(
        ResolverConfig::udp_and_tcp(&CLOUDFLARE),
        TokioRuntimeProvider::default(),
    )
    .build()
    .map_err(|_| Error::ResolverSetup)?;

    let start = tokio::time::Instant::now();
    let mut delay = Duration::from_secs(1);
    loop {
        if let Ok(lookup) = resolver.txt_lookup(name).await {
            let found = lookup.answers().iter().any(|record| {
                let RData::TXT(txt) = &record.data else {
                    return false;
                };
                let value: Vec<u8> = txt
                    .txt_data
                    .iter()
                    .flat_map(|chunk| chunk.iter().copied())
                    .collect();
                value == expected.as_bytes()
            });
            if found {
                return Ok(());
            }
        }
        if start.elapsed() >= timeout {
            return Err(Error::DnsPropagationTimeout);
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(60));
    }
}
