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

## Amounts
- Every amount in signatures, calldata and machine output is a raw-unit integer string. Amount
  parsing (`parse_amount`, `scale_amount`, the `raw:` prefix) is owned by issue #2 — do not touch it.

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
