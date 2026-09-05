// ABI-derived V5 proxy reverts for authorization diagnostics.
// Exports: UserProxyV5Errors.
// Deps: alloy sol-types.

use alloy::sol;

sol! {
    #[derive(Debug)]
    interface UserProxyV5Errors {
        error AlreadyInitialized();
        error NotOwner();
        error Reentrancy();
        error SameToken();
        error NativeNotSupportedInOrder();
        error InvalidSignature();
        error NotOrderOwner();
        error RecipientNotOwner();
        error OrderNotYetStarted(uint256 startTime);
        error OrderExpired(uint256 endTime);
        error OrderKilledByOwner(bytes32 id);
        error OwnerNonceUsed(uint256 nonce);
        error DebitExceedsSignedAmount(uint256 debit, uint256 signed);
        error EmptyNonceMask();
        error EmptyOrderList();
        error AgentOrderExpired();
        error AuthorizationExpired(uint64 deadline);
        error AuthorizationExpiresBeforeOrderOpens(uint64 deadline, uint256 startTime);
        error PolicyExpiresBeforeOrderOpens(uint64 expiry, uint256 startTime);
        error AuthOrderMismatch();
        error InvalidAgent();
        error InvalidToken();
        error InvalidExpiry();
        error InvalidEpochLen();
        error LengthMismatch();
        error PolicyInactive();
        error PolicyGenerationMismatch(uint64 signed, uint64 current);
        error ActionNotAllowed();
        error TokenNotAllowed(address token);
        error NonceAlreadyUsed();
        error CapExceeded(uint256 used, uint256 amount, uint256 cap);
        error SlippageExceeded(uint256 received, uint256 minOut);
        error RouterCallFailed(bytes returnData);
        error UnexpectedEthValue(uint256 sent);
        error EthValueMismatch(uint256 sent, uint256 expected);
        error InsufficientOwnerDelivery(uint256 delivered, uint256 required);
        error Permit2AmountOverflow(uint256 amount);
        error MinOutZero();
        error AmountInZero();
        error SpenderNotAllowed();
        error NotSettler(address caller, address expected);
    }
}
