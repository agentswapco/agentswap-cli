// Fork-only setup helpers: fresh identities, local transactions and captured CLI output.
// Every write targets anvil; the optional fork test exercises these helpers together.
use alloy::{primitives::{Address, U256, keccak256}, sol_types::{SolCall, SolValue}};
use serde_json::{Value, json};
use std::process::{Child, Command};
use crate::{evm, service::token, signer::{Signer, local::LocalKey}};

alloy::sol! {
    function deploy(address owner);
    function deposit();
    function approve(address spender, uint256 amount);
    function grantAgent(address agent, uint64 expiry, uint32 epochLen, uint8 actions, address[] tokens, uint256[] caps);
    event ExecutedAsAgent(address indexed agent, address indexed router, address indexed relayer,
        address tokenIn, address tokenOut, uint256 amountIn, uint256 amountOut);
}

pub struct Fork {
    pub rpc: String,
    pub chain: u64,
    pub owner: Address,
    pub agent: Address,
    pub key: std::path::PathBuf,
    pub tokens: [Address; 3],
    pub cap: U256,
    child: Child,
}

impl Fork {
    pub fn start(url: &str) -> Self {
        let chain = std::env::var("CHAIN").unwrap_or_else(|_| "8453".into()).parse().unwrap();
        assert!(matches!(chain, 8453 | 56), "CHAIN must be 8453 or 56");
        let tokens = if chain == 56 {
            ["BSC_WBNB", "BSC_ETH", "BSC_USDT"].map(|name| std::env::var(name).expect(name).parse().unwrap())
        } else { ["WETH", "cbETH", "USDC"].map(|s| token::from_registry(s, chain).unwrap().address.parse().unwrap()) };
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let child = Command::new("anvil").args(["--fork-url", url, "--port", &port.to_string(), "--silent"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().expect("anvil is required");
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).unwrap();
        let owner = LocalKey::from_private_key(&hex::encode(secret)).unwrap().address();
        getrandom::fill(&mut secret).unwrap();
        let agent = LocalKey::from_private_key(&hex::encode(secret)).unwrap().address();
        let key = std::env::temp_dir().join(format!("batch-fork-{}.key", std::process::id()));
        use std::os::unix::fs::OpenOptionsExt;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&key).unwrap();
        file.write_all(hex::encode(secret).as_bytes()).unwrap();
        let fork = Self { rpc:format!("http://127.0.0.1:{port}"), chain, owner, agent, key, tokens,
            cap:U256::from(if chain == 56 { 100_000_000_000_000_000u64 } else { 10_000_000_000_000_000u64 }), child };
        assert!(Command::new("waitfor").args([&format!("port:{port}"), "-t", "60", "-i", "1"]).status().unwrap().success());
        fork
    }

    pub async fn setup(&self) -> Address {
        assert_eq!(rpc(&self.rpc, "eth_chainId", json!([])).await, format!("0x{:x}",self.chain));
        for who in [self.owner, self.agent] {
            rpc(&self.rpc, "anvil_setBalance", json!([who,format!("0x{:x}",U256::from(100u64)*U256::from(10).pow(U256::from(18)))])).await;
        }
        rpc(&self.rpc, "anvil_impersonateAccount", json!([self.owner])).await;
        let config = evm::chain_config(&self.chain.to_string()).unwrap();
        self.send(config.factory, deployCall {owner:self.owner}.abi_encode(), U256::ZERO).await;
        let provider = evm::read_provider(&self.rpc).unwrap();
        let proxy = crate::order_types::UserProxyFactoryV6::new(config.factory, provider).proxyOf(self.owner).call().await.unwrap();
        self.send(self.tokens[0], depositCall {}.abi_encode(), self.cap).await;
        self.balance_second(self.cap).await;
        proxy
    }

    pub async fn balance_second(&self, balance: U256) {
        let slot = if self.chain == 56 { 1 } else { 51 };
        let index = keccak256((self.owner, U256::from(slot)).abi_encode());
        rpc(&self.rpc, "anvil_setStorageAt", json!([self.tokens[1],index,format!("0x{:064x}",balance)])).await;
        let provider = evm::read_provider(&self.rpc).unwrap();
        assert_eq!(crate::service::portfolio::discovery::balance(&provider,self.tokens[1],self.owner).await.unwrap(),balance);
    }

    pub async fn grant(&self, proxy: Address) {
        let block = rpc(&self.rpc, "eth_getBlockByNumber", json!(["latest",false])).await;
        let now = u64::from_str_radix(block["timestamp"].as_str().unwrap().trim_start_matches("0x"),16).unwrap();
        self.send(proxy, grantAgentCall {agent:self.agent,expiry:now+86400,epochLen:604800,actions:5,
            tokens:self.tokens.to_vec(),caps:vec![self.cap,self.cap,U256::ZERO]}.abi_encode(),U256::ZERO).await;
        for address in &self.tokens[..2] {
            self.send(*address, approveCall {spender:proxy,amount:self.cap*U256::from(2)}.abi_encode(),U256::ZERO).await;
        }
    }

    pub async fn send(&self, to: Address, data: Vec<u8>, value: U256) {
        let hash = rpc(&self.rpc, "eth_sendTransaction", json!([{"from":self.owner,"to":to,
            "data":format!("0x{}",hex::encode(data)),"value":format!("0x{value:x}"),"gas":"0x989680"}])).await;
        let receipt = rpc(&self.rpc,"eth_getTransactionReceipt",json!([hash])).await;
        assert_eq!(receipt["status"],"0x1","setup transaction reverted");
    }

    pub fn cli(&self, app: &str, rpc: &str, args: &[String]) -> Result<Value, String> {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact",super::NAME,"--nocapture"])
            .env("BATCH_FORK_CHILD",app).env("BATCH_FORK_ARGS",serde_json::to_string(args).unwrap())
            .env("AGENTSWAP_RPC_URL",rpc).env(format!("AGENTSWAP_RPC_URL_{}",self.chain),rpc).output().unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        if !output.status.success() { return Err(format!("{text} {}",String::from_utf8_lossy(&output.stderr))); }
        let start = text.find("{\n").expect("CLI JSON output");
        serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>().next().unwrap().map_err(|e|e.to_string())
    }
}

impl Drop for Fork {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.key);
    }
}

pub async fn rpc(url: &str, method: &str, params: Value) -> Value {
    let body: Value = reqwest::Client::new().post(url).json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send().await.unwrap().json().await.unwrap();
    assert!(body.get("error").is_none(),"local RPC {method} failed: {body}");
    body["result"].clone()
}
