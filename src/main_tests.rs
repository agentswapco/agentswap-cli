// Regression tests for main behavior.
// Uses the parent module and existing fixtures.
use super::*;
    use service::submit::{NotConfirmed, TxStatus, EXIT_REVERTED, EXIT_UNKNOWN};

    #[test]
    fn exit_status_tells_refused_from_reverted_from_unknown() {
        assert_eq!(exit_code(&eyre::eyre!("refused before signing")), 1);
        for (status, code) in [(TxStatus::Reverted, EXIT_REVERTED), (TxStatus::Unknown, EXIT_UNKNOWN)] {
            let failure = NotConfirmed::check(Some("0xab"), Some(status), None).unwrap();
            assert_eq!(exit_code(&eyre::Report::new(failure)), code);
        }
    }
