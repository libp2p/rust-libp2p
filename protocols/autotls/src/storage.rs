// #[cfg(feature = "tokio")]
// use std::path::PathBuf;
use std::{
    future::ready,
    io,
    sync::{Arc, Mutex},
};

/// A stored certificate: the PEM certificate chain and its PKCS#8 PEM private key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCertificate {
    /// The PEM-encoded certificate chain returned by the ACME server.
    pub chain_pem: String,
    /// The PKCS#8 PEM-encoded private key for the certificate.
    pub key_pem: String,
}

/// Storage for the ACME account credentials and the issued certificate.
pub trait CertStore: Send + Sync {
    /// Load the serialized ACME account credentials, or `None` if none are stored.
    fn load_account(&self) -> impl Future<Output = io::Result<Option<String>>> + Send;
    /// Persist the serialized ACME account credentials.
    fn store_account(&self, credentials: &str) -> impl Future<Output = io::Result<()>> + Send;
    /// Load the stored certificate, or `None` if none is stored.
    fn load_certificate(
        &self,
    ) -> impl Future<Output = io::Result<Option<StoredCertificate>>> + Send;
    /// Persist the certificate.
    fn store_certificate(
        &self,
        certificate: &StoredCertificate,
    ) -> impl Future<Output = io::Result<()>> + Send;
}

/// An in-memory [`CertStore`].
#[derive(Debug, Clone, Default)]
pub struct MemCertStore {
    inner: Arc<Mutex<MemState>>,
}

#[derive(Debug, Default)]
struct MemState {
    account: Option<String>,
    certificate: Option<StoredCertificate>,
}

impl MemCertStore {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemState> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl CertStore for MemCertStore {
    fn load_account(&self) -> impl Future<Output = io::Result<Option<String>>> + Send {
        ready(Ok(self.lock().account.clone()))
    }

    fn store_account(&self, credentials: &str) -> impl Future<Output = io::Result<()>> + Send {
        self.lock().account = Some(credentials.to_owned());
        ready(Ok(()))
    }

    fn load_certificate(
        &self,
    ) -> impl Future<Output = io::Result<Option<StoredCertificate>>> + Send {
        ready(Ok(self.lock().certificate.clone()))
    }

    fn store_certificate(
        &self,
        certificate: &StoredCertificate,
    ) -> impl Future<Output = io::Result<()>> + Send {
        self.lock().certificate = Some(certificate.clone());
        ready(Ok(()))
    }
}

// /// A [`CertStore`] backed by a directory on the filesystem.
// #[cfg(feature = "tokio")]
// #[derive(Debug, Clone)]
// pub struct FileCertStore {
//     directory: PathBuf,
// }
//
// #[cfg(feature = "tokio")]
// impl FileCertStore {
//     /// Create a store rooted at the given directory.
//     pub fn new(directory: impl Into<PathBuf>) -> Self {
//         Self {
//             directory: directory.into(),
//         }
//     }
// }
//
// #[cfg(feature = "tokio")]
// impl CertStore for FileCertStore {
//     fn load_account(&self) -> impl Future<Output = io::Result<Option<String>>> + Send {
//         read_optional(self.directory.join("account.json"))
//     }
//
//     fn store_account(&self, credentials: &str) -> impl Future<Output = io::Result<()>> + Send {
//         write_file(
//             self.directory.clone(),
//             "account.json",
//             credentials.to_owned(),
//         )
//     }
//
//     fn load_certificate(
//         &self,
//     ) -> impl Future<Output = io::Result<Option<StoredCertificate>>> + Send {
//         let directory = self.directory.clone();
//         async move {
//             let (Some(chain_pem), Some(key_pem)) = (
//                 read_optional(directory.join("cert.pem")).await?,
//                 read_optional(directory.join("key.pem")).await?,
//             ) else {
//                 return Ok(None);
//             };
//             Ok(Some(StoredCertificate { chain_pem, key_pem }))
//         }
//     }
//
//     fn store_certificate(
//         &self,
//         certificate: &StoredCertificate,
//     ) -> impl Future<Output = io::Result<()>> + Send {
//         let directory = self.directory.clone();
//         let chain_pem = certificate.chain_pem.clone();
//         let key_pem = certificate.key_pem.clone();
//         async move {
//             write_file(directory.clone(), "cert.pem", chain_pem).await?;
//             write_file(directory, "key.pem", key_pem).await
//         }
//     }
// }
//
// #[cfg(feature = "tokio")]
// async fn write_file(directory: PathBuf, file: &'static str, contents: String) -> io::Result<()> {
//     tokio::fs::create_dir_all(&directory).await?;
//     tokio::fs::write(directory.join(file), contents).await
// }
//
// #[cfg(feature = "tokio")]
// async fn read_optional(path: PathBuf) -> io::Result<Option<String>> {
//     match tokio::fs::read_to_string(path).await {
//         Ok(contents) => Ok(Some(contents)),
//         Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
//         Err(e) => Err(e),
//     }
// }

#[cfg(all(test, feature = "tokio"))]
mod tests {
    use super::*;

    // #[tokio::test]
    // async fn file_store_account_round_trips_and_missing_is_none() {
    //     let dir = tempfile::tempdir().unwrap();
    //     let store = FileCertStore::new(dir.path());
    //
    //     assert_eq!(store.load_account().await.unwrap(), None);
    //     store.store_account("creds-blob").await.unwrap();
    //     assert_eq!(
    //         store.load_account().await.unwrap().as_deref(),
    //         Some("creds-blob")
    //     );
    // }
    //
    // #[tokio::test]
    // async fn file_store_certificate_round_trips_and_partial_is_none() {
    //     let dir = tempfile::tempdir().unwrap();
    //     let store = FileCertStore::new(dir.path());
    //
    //     assert_eq!(store.load_certificate().await.unwrap(), None);
    //     let cert = StoredCertificate {
    //         chain_pem: "CHAIN".to_owned(),
    //         key_pem: "KEY".to_owned(),
    //     };
    //     store.store_certificate(&cert).await.unwrap();
    //     assert_eq!(store.load_certificate().await.unwrap(), Some(cert));
    // }

    #[tokio::test]
    async fn mem_store_round_trips_and_clone_shares_state() {
        let store = MemCertStore::new();
        let clone = store.clone();

        assert_eq!(store.load_account().await.unwrap(), None);
        store.store_account("creds-blob").await.unwrap();
        assert_eq!(
            clone.load_account().await.unwrap().as_deref(),
            Some("creds-blob")
        );

        let cert = StoredCertificate {
            chain_pem: "CHAIN".to_owned(),
            key_pem: "KEY".to_owned(),
        };
        store.store_certificate(&cert).await.unwrap();
        assert_eq!(clone.load_certificate().await.unwrap(), Some(cert));
    }
}
