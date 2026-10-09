// Optional holdings and catalog HTTP regressions, including the SPA HTML fallback.
// Verifies validated identities, chain filtering, provenance and atomic failure handling.
use super::{discovery::{Candidates, add}, http::Server, sources::*};
use alloy::primitives::Address;
use crate::client::Client;
use serde_json::json;

#[tokio::test]
async fn wallet_tokens_failure_modes_find_nothing() {
    let owner = Address::repeat_byte(4);
    let valid = json!({"chainId":8453,"owner":owner,"indexed":true,"truncated":false,
        "tokens":[{"address":Address::repeat_byte(1),"balanceRaw":"10"}]});
    let mut cases = vec![(500, valid.to_string()), (302, valid.to_string()), (429, valid.to_string()), (200, "<html>SPA fallback</html>".into()),
        (200, "{}".into()), (200, "{\"indexed\":true,\"tokens\":[]}".into())];
    for (field, value) in [("chainId", json!(56)), ("owner", json!(Address::repeat_byte(5))),
        ("tokens", json!([{"address":"invalid","balanceRaw":"10"}])),
        ("tokens", json!([{"address":Address::repeat_byte(1),"balanceRaw":"bad"}])),
        ("tokens", json!([{"address":Address::repeat_byte(1)}]))] {
        let mut invalid = valid.clone(); invalid[field] = value;
        cases.push((200, invalid.to_string()));
    }
    for (status, body) in cases {
        let server = Server::start(vec![(status, String::new(), body)]);
        let (state, truncated, rows) = wallet_tokens(&server.url, 8453, owner).await;
        assert_eq!((state, truncated), (WalletTokens::Unavailable, false));
        assert!(rows.is_empty());
    }
    assert_eq!(wallet_tokens("http://127.0.0.1:1", 8453, owner).await.0, WalletTokens::Unavailable);
}

#[tokio::test]
async fn wallet_tokens_indexed_unindexed_and_truncated() {
    let owner = Address::repeat_byte(4);
    for indexed in [true, false] {
        let server = Server::start(vec![(200, String::new(), json!({"chainId":8453,"owner":owner,
            "indexed":indexed,"truncated":true,"tokens":[{"address":Address::repeat_byte(1),"balanceRaw":"10"},
            {"address":Address::repeat_byte(2),"balanceRaw":"0"}]}).to_string())]);
        let (state, truncated, rows) = wallet_tokens(&server.url, 8453, owner).await;
        assert_eq!(state, if indexed { WalletTokens::Indexed } else { WalletTokens::Unindexed });
        assert_eq!(truncated, indexed);
        assert_eq!(rows.len(), usize::from(indexed));
        if indexed { assert_eq!(rows[0].sources, vec!["wallet-tokens"]); }
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].target, format!("/api/wallet-tokens?chain=8453&owner={owner:?}"));
    }
}

#[tokio::test]
async fn catalog_filters_chain_and_unions_registry() {
    let token = Address::repeat_byte(1);
    let server = Server::start(vec![(200, String::new(), json!({format!("{token:?}"):{"chain_id":8453},
        format!("{:?}", Address::repeat_byte(2)):{"chain_id":56}}).to_string())]);
    let mut candidates = Candidates::new();
    add(&mut candidates, token, "registry");
    catalog(&Client::new(&server.url, None), 8453, &mut candidates).await.unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[&token], ["catalog".into(), "registry".into()].into());
    assert_eq!(server.requests.lock().unwrap()[0].target, crate::routes::TOKENS);
}

#[tokio::test]
async fn catalog_failure_modes_preserve_other_sources() {
    for (status, body) in [(500, "{}"), (200, "<html>SPA</html>"), (200, "[]"),
        (200, "{\"bad\":{\"chain_id\":8453}}"), (200, "{\"bad\":{}}") ] {
        let server = Server::start(vec![(status, String::new(), body.into())]);
        let mut candidates = Candidates::new();
        add(&mut candidates, Address::repeat_byte(1), "registry");
        assert!(catalog(&Client::new(&server.url, None), 8453, &mut candidates).await.is_err());
        assert_eq!(candidates.len(), 1);
    }
    assert!(catalog(&Client::new("http://127.0.0.1:1", None), 8453, &mut Candidates::new()).await.is_err());
}
