//! Server addresses as a person types them, and the WebSocket URLs derived from them, as the app's
//! API client (`packages/client/src/api/url.ts`) treats them.

use url::Url;

use crate::Error;

/// The port of a server addressed by a bare host.
const DEFAULT_PORT: u16 = 8095;

/// Normalizes a typed server address. A bare host is taken to be a server on the local network:
/// plain HTTP on port 8095 unless a port is given. An address with a scheme is used as typed,
/// since it usually points at a reverse proxy. Trailing slashes are dropped.
pub fn normalize_server_url(input: &str) -> Result<String, Error> {
    let trimmed = input.trim().trim_end_matches('/');
    let has_scheme = trimmed.contains("://");
    let text = if has_scheme {
        trimmed.to_owned()
    } else {
        format!("http://{trimmed}")
    };
    let mut url = Url::parse(&text).map_err(|e| Error::InvalidAddress(e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(Error::InvalidAddress(format!(
            "{input} is not an http or https address"
        )));
    }
    if !has_scheme && url.port().is_none() {
        let _ = url.set_port(Some(DEFAULT_PORT));
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// The WebSocket URL for `path` on the server, keeping any path prefix of a reverse proxy.
pub(crate) fn socket_url(server_url: &str, path: &str) -> Result<String, Error> {
    let mut url = Url::parse(server_url).map_err(|e| Error::InvalidAddress(e.to_string()))?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme)
        .map_err(|()| Error::InvalidAddress(server_url.to_owned()))?;
    let prefix = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{prefix}{path}"));
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_host_is_http_on_the_default_port() {
        assert_eq!(
            normalize_server_url(" music.local/ ").unwrap(),
            "http://music.local:8095"
        );
        assert_eq!(
            normalize_server_url("10.0.0.2:9000").unwrap(),
            "http://10.0.0.2:9000"
        );
    }

    #[test]
    fn an_address_with_a_scheme_is_kept() {
        assert_eq!(
            normalize_server_url("https://example.com/bopsie/").unwrap(),
            "https://example.com/bopsie"
        );
    }

    #[test]
    fn other_schemes_are_rejected() {
        assert!(normalize_server_url("ftp://example.com").is_err());
    }

    #[test]
    fn socket_urls_follow_the_scheme_and_keep_the_prefix() {
        assert_eq!(
            socket_url("http://music.local:8095", "/sendspin").unwrap(),
            "ws://music.local:8095/sendspin"
        );
        assert_eq!(
            socket_url("https://example.com/bopsie", "/ws").unwrap(),
            "wss://example.com/bopsie/ws"
        );
    }
}
