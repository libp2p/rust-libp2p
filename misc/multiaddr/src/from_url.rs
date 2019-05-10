use crate::{Multiaddr, Protocol};
use std::{error, fmt, iter, net::IpAddr};

/// Attempts to parse an URL into a multiaddress.
///
/// # Example
///
/// ```
/// let addr = parity_multiaddr::from_url("ws://127.0.0.1:8080/").unwrap();
/// assert_eq!(addr, "/ip4/127.0.0.1/tcp/8080/ws".parse().unwrap());
/// ```
///
pub fn from_url(url: &str) -> std::result::Result<Multiaddr, FromUrlErr> {
    let url = url::Url::parse(url).map_err(|_| FromUrlErr::BadUrl)?;

    let (protocol, default_port) = match url.scheme() {
        "ws" => (Protocol::Ws, 80),
        "wss" => (Protocol::Wss, 443),
        "http" => (Protocol::Http, 80),
        "https" => (Protocol::Https, 443),
        _ => return Err(FromUrlErr::WrongScheme)
    };

    let port = Protocol::Tcp(url.port().unwrap_or(default_port));
    let ip = if let Some(hostname) = url.host_str() {
        if let Ok(ip) = hostname.parse::<IpAddr>() {
            Protocol::from(ip)
        } else {
            Protocol::Dns4(hostname.into())
        }
    } else {
        return Err(FromUrlErr::BadUrl);
    };

    Ok(iter::once(ip)
        .chain(iter::once(port))
        .chain(iter::once(protocol))
        .collect())
}

/// Error while parsing an URL.
#[derive(Debug)]
pub enum FromUrlErr {
    /// Failed to parse the URL.
    BadUrl,
    /// The URL scheme was not recognized.
    WrongScheme,
}

impl fmt::Display for FromUrlErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FromUrlErr::BadUrl => write!(f, "Bad URL"),
            FromUrlErr::WrongScheme => write!(f, "Unrecognized URL scheme"),
        }
    }
}

impl error::Error for FromUrlErr {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_garbage_doesnt_panic() {
        for _ in 0 .. 50 {
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
        assert_eq!(addr, "/dns4/example.com/tcp/80/ws".parse().unwrap());
    }

    #[test]
    fn dns_addr_http() {
        let addr = from_url("http://example.com").unwrap();
        assert_eq!(addr, "/dns4/example.com/tcp/80/http".parse().unwrap());
    }

    #[test]
    fn dns_addr_wss() {
        let addr = from_url("wss://example.com").unwrap();
        assert_eq!(addr, "/dns4/example.com/tcp/443/wss".parse().unwrap());
    }

    #[test]
    fn dns_addr_https() {
        let addr = from_url("https://example.com").unwrap();
        assert_eq!(addr, "/dns4/example.com/tcp/443/https".parse().unwrap());
    }

    #[test]
    fn bad_hostname() {
        let addr = from_url("wss://127.0.0.1x").unwrap();
        assert_eq!(addr, "/dns4/127.0.0.1x/tcp/443/wss".parse().unwrap());
    }

    #[test]
    fn wrong_scheme() {
        match from_url("foo://127.0.0.1") {
            Err(FromUrlErr::WrongScheme) => {}
            _ => panic!()
        }
    }
}
