// Batch-sell plans create review requests; runs consume only confirmed requests.
// Execution reuses the grant-bounded sale engine and its signing safeguards.
mod models;
mod plan;
mod request;
mod run;
mod received;
mod report;
pub use models::{PlanInput, PlanOutput, RunInput, Via};
pub use plan::plan;
pub use run::{run, load};
pub use report::{ReportInput, ReportOutput, report, text as report_text};
pub(crate) use run::raw_cap;
#[cfg(test)]
mod tests;
