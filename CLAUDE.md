# agentswap-cli — rules for anyone editing this repo

## Protocol binding (V6, since 2026-09-06)
- The CLI targets one V6 address set on Base 8453, Arbitrum 42161, BSC 56 and Robinhood 4663:
  `UserProxyFactoryV6`, `IntentSettlerV3`, `IntentLensV3` (`src/evm.rs`). Addresses are edited
  only by the maintainer; never type a hex address into any other file.
- EIP-712 agent domain is `("AgentSwap UserProxy", "5")` with the owner's V6 proxy as
  `verifyingContract`. `UserProxyV6` inherits `ProxyCoreV5`, which declares that domain; do not
  bump the version string. The deployed-contract parity test proves it.
- `IntentLensV3.preview` returns nineteen static words; the layout is published in
  agentswap-protocol `docs/lens-preview-layouts.md`. Read `PREVIEW_LAYOUT()` once per command and
  refuse to decode against any value other than 3. Never carry V2 word indices forward.
- `IntentFilled` carries `fee` after `requiredOut`; `requiredOut` is gross (floor + fee), never
  user proceeds.

## Identity
- Every commit, tag, issue, PR and release on this repository is made by the project identity
  `agentswapco <274458467+agentswapco@users.noreply.github.com>`, never by an individual. Do not
  add `authors` to `Cargo.toml`, and do not put a person's name, email or machine path in any file.
- Agents never push; the maintainer pushes from the project account.
- Commit messages carry no URL that identifies a person, an account or a chat session.

## Amounts
- Every monetary input, on the CLI and over MCP, is an unsigned decimal integer in the asset's
  smallest unit: `1000000` is 1 USDC, `1000000000000000000` is 1 ETH. No input is ever scaled by
  token decimals, and there is no second input format.
- Token decimals may be used only to render a supplementary display value beside the raw one.
  A rendered value is never parsed back as an amount, and no amount passes through a float.

## Published copy states technical facts only
Everything a reader meets outside the code — `--help` strings, `after_help`, MCP tool descriptions
and input-schema descriptions, runtime messages and tables, `README.md`, `CHANGELOG.md` and the
`Cargo.toml` description — states technical facts only:
- State every precondition the code enforces where the reader meets the command, not only in the
  code: `--min-out` for a live trade, `--allow-trade` for `trade`, `intent place` and both MCP
  signing tools, a positive `--x402-max-amount` for `--x402`, and the chains a command is deployed
  on.
- No tuning constants, budgets, thresholds, gas figures or bps the code owns internally; print the
  chain-derived value instead of restating it. No first person about the team, no operational
  status, no dates or version history, no internal document, service or repository names. A
  limitation an integrator must know stays, phrased as a property of the code.
- One product one-liner, shared by `Cargo.toml`, the clap `about` and `README.md`.
- The chain-selector sentence lives in `crate::tokens::CHAIN_ID_HELP` and the V6 chain list in
  `crate::tokens::V6_CHAINS_NOTE`. Refer to them; never paste them.
- One name per thing: the executor is a solver ("filler" only when naming `IIntentFiller`), the
  signed object is an intent ("order" only when naming the EIP-712 type), chain 56 is BNB Smart
  Chain, chain 42161 is Arbitrum One wherever ids are listed, and the wallet that owns the proxy is
  the owner.
- `health` prints status, version and uptime; the rest stays in `--json`. `register` prints the key
  once. One error gets one remediation block.
- Copy that renames or removes a flag, subcommand, env var, default or MCP tool is a breaking
  release, not a copy fix: correct the help text to describe current behaviour and record the
  rename for the next breaking release.

## Code style
- File ≤ 300 lines, function ≤ 50 lines, every file opens with a 2–4 line header comment.
- No compat shims, feature flags or dead code; change the API directly. No `any`-style escape
  hatches at boundaries. Every new function gets a test.
- Working language is English for everything on disk.

## Build and test
- Use `cargo check` for edit loops and `cargo test` before reporting done; report test output
  verbatim, never a summary of it. The RPC-gated parity test is skipped without its env vars —
  say so rather than counting it as passed.

<!-- aid:start -->
## aid orchestration

This project uses [aid](https://github.com/agent-tools-org/ai-dispatch) as the primary development method.
Use `aid run` to dispatch coding tasks to AI agents instead of writing code directly.

- **Project**: agentswap-cli
- **Profile**: standard
- **Language**: rust
- **Budget**: $20/day
- **Verify**: cargo test

### Rules
- All new functions must have at least one test

### Usage
- Dispatch work: `aid run <agent> "<prompt>" --dir .`
- Review output: `aid show <id> --diff`
- Batch dispatch: `aid batch <file> --parallel`
- Project config: `.aid/project.toml`

<!-- aid:end -->
