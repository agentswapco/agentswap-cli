// Grant-request fixture parity and mocked policy, cap and replacement validation.
// Tests call the same service used by CLI and MCP; fixture addresses live in evm.
use super::*;
use crate::service::portfolio::tests::fixture;

fn input() -> Input {
    Input { chain_id: "4663".into(), owner: format!("{:?}", Address::repeat_byte(4)), agent: format!("{:?}", Address::repeat_byte(2)),
        tokens: vec![format!("{:?}", Address::repeat_byte(1))], receive: format!("{:?}", Address::repeat_byte(3)),
        one_shot: true, epoch: None, expiry: None, label: None, note: None, replace: false }
}

#[test]
fn f1_f2_byte_exact_input_to_full_url() {
    for (index, chain, raws, decimals, label, expiry, owner) in [
        (0, 8453, ["1234500", "5", "0"], [6, 18, 6], Some("dust sweep"), "2026-10-10T12:00:00Z", true),
        (1, 42161, ["250000000", "500000000000000000", "0"], [6, 18, 6], None, "30d", false),
    ] {
        let tokens = raws.iter().enumerate().map(|(i, raw)| TokenSummary { address: crate::evm::GRANT_TOKENS[i].into(), symbol: "TEST".into(), decimals: decimals[i], raw: (*raw).into(), human: amount::render(raw, decimals[i] as usize) }).collect::<Vec<_>>();
        let agent = crate::evm::GRANT_AGENT;
        let owner = owner.then_some(crate::evm::GRANT_OWNER);
        assert_eq!(url::build(chain, agent, owner, label, None, &tokens, "1w", expiry).unwrap(), crate::evm::GRANT_FIXTURES[index]);
    }
}

#[test]
fn schedule_iso_and_form_encoding() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-09T12:00:00Z").unwrap().timestamp() as u64;
    let mut request = input();
    assert_eq!(url::schedule(&request, now).unwrap(), ("1w".into(), "2026-10-10T12:00:00Z".into()));
    request.epoch = Some("1d".into());
    assert!(url::schedule(&request, now).is_err());
    request.one_shot = false;
    for expiry in ["7d", "30d", "90d", "2026-10-10T12:00:00Z"] {
        request.expiry = Some(expiry.into()); assert!(url::schedule(&request, now).is_ok());
    }
    for expiry in ["2026-10-10T12:00:00+00:00", "2026-10-10T12:00:00.1Z", "2020-01-01T00:00:00Z", "bad"] {
        request.expiry = Some(expiry.into()); assert!(url::schedule(&request, now).is_err());
    }
    let link = url::build(8453, "agent", None, Some("a & b"), Some("x+y/#"), &[], "1w", "30d").unwrap();
    assert!(link.contains("label=a+%26+b"));
    assert!(link.ends_with("actions=market%2Cintent&note=x%2By%2F%23"));
}

#[test]
fn rejects_invalid_baskets_and_recurring_caps() {
    let mut request = input();
    request.tokens.push(request.tokens[0].clone());
    assert!(url::requests(&request, &request.receive).is_err());
    request = input();
    assert!(url::requests(&request, &request.tokens[0]).is_err());
    request.one_shot = false;
    assert!(url::requests(&request, &request.receive).is_err());
    request.tokens[0].push_str(":0005");
    assert_eq!(url::requests(&request, &request.receive).unwrap()[0].1.as_deref(), Some("5"));
    request.tokens = vec![format!("{}:0", Address::ZERO)];
    assert!(url::requests(&request, &request.receive).is_err());
    request.tokens = (1..20).map(|i| format!("{:?}:1", Address::repeat_byte(i))).collect();
    assert_eq!(url::requests(&request, &Address::repeat_byte(99).to_string()).unwrap().len(), 20);
    request.tokens = vec![input().tokens[0].clone(); 20];
    assert!(url::requests(&request, &request.receive).is_err());
}

#[tokio::test]
async fn live_policy_requires_replace_and_no_proxy_permits() {
    for (live, deployed) in [(true, true), (false, true), (true, false)] {
        let rpc = fixture(false, live, deployed, 6);
        let provider = evm::read_provider(&rpc.url).unwrap();
        let result = read(input(), &provider, 1_791_547_200).await;
        assert_eq!(result.is_err(), live && deployed);
        let mut request = input(); request.replace = true;
        let output = read(request, &provider, 1_791_547_200).await.unwrap();
        assert_eq!(output.replaced_policy.is_some(), live && deployed);
        assert_eq!(output.tokens[0].raw, "1000000");
        assert_eq!(output.tokens[0].human, "1");
        assert_eq!(output.tokens[1].human, "0");
        assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
    }
}

#[tokio::test]
async fn rejects_zero_caps_and_oversized_rendering() {
    let rpc = fixture(false, false, false, 6);
    let provider = evm::read_provider(&rpc.url).unwrap();
    let mut request = input(); request.tokens[0].push_str(":0");
    assert!(read(request, &provider, 1).await.unwrap_err().to_string().contains("positive"));
    let rpc = fixture(false, false, false, 33);
    let provider = evm::read_provider(&rpc.url).unwrap();
    let mut request = input(); request.tokens[0].push_str(":1");
    assert!(read(request, &provider, 1).await.unwrap_err().to_string().contains("32 characters"));
}

#[tokio::test]
async fn gasless_grants_request_both_actions() {
    let rpc = fixture(false, false, false, 6);
    let provider = evm::read_provider(&rpc.url).unwrap();
    for one_shot in [true, false] {
        let mut request = input();
        request.one_shot = one_shot;
        if !one_shot { request.tokens[0].push_str(":100"); request.epoch = Some("1d".into()); request.expiry = Some("7d".into()); }
        let output = read(request, &provider, 1_791_547_200).await.unwrap();
        assert!(serde_json::to_string(&output).unwrap().contains("actions=market%2Cintent"));
    }
}
