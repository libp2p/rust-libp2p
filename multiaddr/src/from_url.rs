use crate::{Multiaddr, Protocol};
use std::{error, fmt, iter, net::IpAddr};

/// Attempts to parse an URL into a multiaddress.
///
/// This function will return an error if some information in the URL cannot be retained in the
/// generated multiaddress. This includes a username, password, path (if not supported by the
/// multiaddr), and query string.
///
/// This function is only present if the `url` feature is enabled, and it is
/// enabled by default.
///
/// The supported URL schemes are:
///
/// - `ws://example.com/`
/// - `wss://example.com/`
/// - `http://example.com/`
/// - `https://example.com/`
/// - `unix:/foo/bar`
///
/// # Example
///
/// ```
/// let addr = multiaddr::from_url("ws://127.0.0.1:8080/").unwrap();
/// assert_eq!(addr, "/ip4/127.0.0.1/tcp/8080/ws".parse().unwrap());
/// ```
///
pub fn from_url(url: &str) -> std::result::Result<Multiaddr, FromUrlErr> {
    from_url_inner(url, false)
}

/// Attempts to parse an URL into a multiaddress. Ignores possible loss of information.
///
/// This function is similar to [`from_url`], except that we don't return an error if some
/// information in the URL cannot be retain in the generated multiaddres.
///
/// This function is only present if the `url` feature is enabled, and it is
/// enabled by default.
///
/// # Example
///
/// ```
/// let addr = "ws://user:pass@127.0.0.1:8080/";
/// assert!(multiaddr::from_url(addr).is_err());
/// assert!(multiaddr::from_url_lossy(addr).is_ok());
/// ```
///
pub fn from_url_lossy(url: &str) -> std::result::Result<Multiaddr, FromUrlErr> {
    from_url_inner(url, true)
}

/// Underlying implementation of `from_url` and `from_url_lossy`.
fn from_url_inner(url: &str, lossy: bool) -> std::result::Result<Multiaddr, FromUrlErr> {
    let url = url::Url::parse(url).map_err(|_| FromUrlErr::BadUrl)?;

    match url.scheme() {
        // Note: if you add support for a new scheme, please update the documentation as well.
        "ws" | "wss" | "http" | "https" => from_url_inner_http_ws(url, lossy),
        "unix" => from_url_inner_path(url, lossy),
        _ => Err(FromUrlErr::UnsupportedScheme),
    }
}

/// Called when `url.scheme()` is an Internet-like URL.
fn from_url_inner_http_ws(
    url: url::Url,
    lossy: bool,
) -> std::result::Result<Multiaddr, FromUrlErr> {
    let (protocol, lost_path, default_port) = match url.scheme() {
        "ws" => (Protocol::Ws(url.path().to_owned().into()), false, 80),
        "wss" => (Protocol::Wss(url.path().to_owned().into()), false, 443),
        "http" => (Protocol::Http, true, 80),
        "https" => (Protocol::Https, true, 443),
        _ => unreachable!("We only call this function for one of the given schemes; qed"),
    };

    let port = Protocol::Tcp(url.port().unwrap_or(default_port));
    let ip = if let Some(hostname) = url.host_str() {
        if let Ok(ip) = hostname.parse::<IpAddr>() {
            Protocol::from(ip)
        } else {
            Protocol::Dns(hostname.into())
        }
    } else {
        return Err(FromUrlErr::BadUrl);
    };

    if !lossy
        && (!url.username().is_empty()
            || url.password().is_some()
            || (lost_path && url.path() != "/" && !url.path().is_empty())
            || url.query().is_some()
            || url.fragment().is_some())
    {
        return Err(FromUrlErr::InformationLoss);
    }

    Ok(iter::once(ip)
        .chain(iter::once(port))
        .chain(iter::once(protocol))
        .collect())
}

/// Called when `url.scheme()` is a path-like URL.
fn from_url_inner_path(url: url::Url, lossy: bool) -> std::result::Result<Multiaddr, FromUrlErr> {
    let protocol = match url.scheme() {
        "unix" => Protocol::Unix(url.path().to_owned().into()),
        _ => unreachable!("We only call this function for one of the given schemes; qed"),
    };

    if !lossy
        && (!url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some())
    {
        return Err(FromUrlErr::InformationLoss);
    }

    Ok(Multiaddr::from(protocol))
}

/// Error while parsing an URL.
#[derive(Debug)]
pub enum FromUrlErr {
    /// Failed to parse the URL.
    BadUrl,
    /// The URL scheme was not recognized.
    UnsupportedScheme,
    /// Some information in the URL would be lost. Never returned by `from_url_lossy`.
    InformationLoss,
}

impl fmt::Display for FromUrlErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FromUrlErr::BadUrl => write!(f, "Bad URL"),
            FromUrlErr::UnsupportedScheme => write!(f, "Unrecognized URL scheme"),
            FromUrlErr::InformationLoss => write!(f, "Some information in the URL would be lost"),
        }
    }
}

impl error::Error for FromUrlErr {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_garbage_doesnt_panic() {
        for _ in 0..50 {
            let url = (0..16).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
            let url = String::from_utf8_lossy(&url);
            assert!(from_url(&url).is_err());
        }
    }

    #[test]
    fn normal_usage_ws() {
        let addr = from_url("ws://127.0.0.1:8000").unwrap();
        assert_eq!(addr, "/ip4/127.0.0.1/tcp/8000/ws".parse().unwrap());
    }

    #[test]
    fn normal_usage_wss() {
        let addr = from_url("wss://127.0.0.1:8000").unwrap();
        assert_eq!(addr, "/ip4/127.0.0.1/tcp/8000/wss".parse().unwrap());
    }

    #[test]
    fn default_ws_port() {
        let addr = from_url("ws://127.0.0.1").unwrap();
        assert_eq!(addr, "/ip4/127.0.0.1/tcp/80/ws".parse().unwrap());
    }

    #[test]
    fn default_http_port() {
        let addr = from_url("http://127.0.0.1").unwrap();
        assert_eq!(addr, "/ip4/127.0.0.1/tcp/80/http".parse().unwrap());
    }

    #[test]
    fn default_wss_port() {
        let addr = from_url("wss://127.0.0.1").unwrap();
        assert_eq!(addr, "/ip4/127.0.0.1/tcp/443/wss".parse().unwrap());
    }

    #[test]
    fn default_https_port() {
        let addr = from_url("https://127.0.0.1").unwrap();
        assert_eq!(addr, "/ip4/127.0.0.1/tcp/443/https".parse().unwrap());
    }

    #[test]
    fn dns_addr_ws() {
        let addr = from_url("ws://example.com").unwrap();
        assert_eq!(addr, "/dns/example.com/tcp/80/ws".parse().unwrap());
    }

    #[test]
    fn dns_addr_http() {
        let addr = from_url("http://example.com").unwrap();
        assert_eq!(addr, "/dns/example.com/tcp/80/http".parse().unwrap());
    }

    #[test]
    fn dns_addr_wss() {
        let addr = from_url("wss://example.com").unwrap();
        assert_eq!(addr, "/dns/example.com/tcp/443/wss".parse().unwrap());
    }

    #[test]
    fn dns_addr_https() {
        let addr = from_url("https://example.com").unwrap();
        assert_eq!(addr, "/dns/example.com/tcp/443/https".parse().unwrap());
    }

    #[test]
    fn bad_hostname() {
        let addr = from_url("wss://127.0.0.1x").unwrap();
        assert_eq!(addr, "/dns/127.0.0.1x/tcp/443/wss".parse().unwrap());
    }

    #[test]
    fn wrong_scheme() {
        match from_url("foo://127.0.0.1") {
            Err(FromUrlErr::UnsupportedScheme) => {}
            _ => panic!(),
        }
    }

    #[test]
    fn dns_and_port() {
        let addr = from_url("http://example.com:1000").unwrap();
        assert_eq!(addr, "/dns/example.com/tcp/1000/http".parse().unwrap());
    }

    #[test]
    fn username_lossy() {
        let addr = "http://foo@example.com:1000/";
        assert!(from_url(addr).is_err());
        assert!(from_url_lossy(addr).is_ok());
        assert!(from_url("http://@example.com:1000/").is_ok());
    }

    #[test]
    fn password_lossy() {
        let addr = "http://:bar@example.com:1000/";
        assert!(from_url(addr).is_err());
        assert!(from_url_lossy(addr).is_ok());
    }

    #[test]
    fn path_lossy() {
        let addr = "http://example.com:1000/foo";
        assert!(from_url(addr).is_err());
        assert!(from_url_lossy(addr).is_ok());
    }

    #[test]
    fn fragment_lossy() {
        let addr = "http://example.com:1000/#foo";
        assert!(from_url(addr).is_err());
        assert!(from_url_lossy(addr).is_ok());
    }

    #[test]
    fn unix() {
        let addr = from_url("unix:/foo/bar").unwrap();
        assert_eq!(addr, Multiaddr::from(Protocol::Unix("/foo/bar".into())));
    }

    #[test]
    fn ws_path() {
        let addr = from_url("ws://1.2.3.4:1000/foo/bar").unwrap();
        assert_eq!(
            addr,
            "/ip4/1.2.3.4/tcp/1000/x-parity-ws/%2ffoo%2fbar"
                .parse()
                .unwrap()
        );

        let addr = from_url("ws://1.2.3.4:1000/").unwrap();
        assert_eq!(addr, "/ip4/1.2.3.4/tcp/1000/ws".parse().unwrap());

        let addr = from_url("wss://1.2.3.4:1000/foo/bar").unwrap();
        assert_eq!(
            addr,
            "/ip4/1.2.3.4/tcp/1000/x-parity-wss/%2ffoo%2fbar"
                .parse()
                .unwrap()
        );

        let addr = from_url("wss://1.2.3.4:1000").unwrap();
        assert_eq!(addr, "/ip4/1.2.3.4/tcp/1000/wss".parse().unwrap());
    }
}
