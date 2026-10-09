// Entrypoint signer loading and transaction-aware exit status.
// Shared by CLI dispatch and its regression tests.
use crate::{service, signer};
use eyre::Result;
use std::sync::Arc;

/// 1 for anything refused or failed before a transaction was sent; a sent transaction that did
/// not confirm carries its own status. Argument errors exit 2 from clap.
pub(crate) fn exit_code(error: &eyre::Report) -> i32 {
    error
        .downcast_ref::<service::submit::NotConfirmed>()
        .map_or(1, service::submit::NotConfirmed::exit_code)
}

pub(crate) fn signer_from_file(path: Option<&str>) -> Result<Option<Arc<dyn signer::Signer>>> {
    match path {
        Some(path) => {
            let key = signer::local::LocalKey::from_key_file(path)?;
            Ok(Some(Arc::new(key)))
        }
        None => Ok(None),
    }
}

