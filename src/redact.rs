// Redaction of URLs in error text before it reaches stderr, JSON or MCP output.
// Exports: urls.
// Deps: std only.

/// Replace the path, query, fragment and userinfo of every http(s)/ws(s) URL in `text`, keeping
/// the scheme, host and port. RPC providers carry API keys in the path or query, and transport
/// errors quote the full request URL.
pub fn urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = next_scheme(rest) {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail.find(is_url_end).unwrap_or(tail.len());
        out.push_str(&origin(&tail[..end]));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn next_scheme(text: &str) -> Option<usize> {
    ["https://", "http://", "wss://", "ws://"]
        .iter()
        .filter_map(|scheme| text.find(scheme))
        .min()
}

fn is_url_end(c: char) -> bool {
    c.is_whitespace() || matches!(c, ')' | '(' | '"' | '\'' | '<' | '>' | ',' | ']' | '[' | '`')
}

fn origin(url: &str) -> String {
    let Some((scheme, after)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority_end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let authority = &after[..authority_end];
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let path = &after[authority_end..];
    let suffix = match path {
        "" | "/" if host.len() == authority.len() => path,
        _ => "/[redacted]",
    };
    format!("{scheme}://{host}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_error_keeps_the_host_and_drops_the_keyed_path() {
        let text = "error sending request for url (https://bsc-mainnet.nodereal.io/v1/KEY123?apikey=Q)";
        assert_eq!(urls(text), "error sending request for url (https://bsc-mainnet.nodereal.io/[redacted])");
    }

    #[test]
    fn userinfo_query_and_port_forms_are_redacted() {
        assert_eq!(urls("http://user:pw@127.0.0.1:8545"), "http://127.0.0.1:8545/[redacted]");
        assert_eq!(urls("see https://rpc.example?key=abc."), "see https://rpc.example/[redacted]");
        assert_eq!(urls("wss://node.example/ws/KEY and http://h/x"), "wss://node.example/[redacted] and http://h/[redacted]");
    }

    #[test]
    fn bare_hosts_and_plain_text_are_unchanged() {
        assert_eq!(urls("https://mainnet.base.org"), "https://mainnet.base.org");
        assert_eq!(urls("for url (https://mainnet.base.org/)"), "for url (https://mainnet.base.org/)");
        assert_eq!(urls("no url here: 0xabc"), "no url here: 0xabc");
        assert_eq!(urls(""), "");
    }
}
