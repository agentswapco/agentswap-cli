// App-owned grant request transport; arbitrary URLs never select the API origin.
// Redirects and payment signing are disabled for review request reads and writes.
use super::models::{GrantRequest, Record, ShortLink};
use eyre::{Result, ensure};

pub(super) fn id(value: &str) -> Result<String> {
    let id = if value.contains("://") {
        let url = reqwest::Url::parse(value)?;
        ensure!(url.origin() == reqwest::Url::parse(crate::routes::APP_ORIGIN)?.origin()
            && url.username().is_empty() && url.password().is_none()
            && url.query().is_none() && url.fragment().is_none(), "request URL must be an app grant URL");
        url.path().strip_prefix("/grant/r/").ok_or_else(|| eyre::eyre!("invalid grant request path"))?.to_owned()
    } else { value.to_owned() };
    ensure!(id.len() >= 22 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'), "invalid grant request id");
    Ok(id)
}

pub(super) fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30)).build()?)
}

pub(super) async fn get(value: &str) -> Result<Record> {
    let id = id(value)?;
    let response = client()?.get(format!("{}/api/grant-requests/{id}", crate::routes::app_origin())).send().await?;
    ensure!(response.status().is_success(), "grant request GET HTTP {}: {}", response.status(), response.text().await?);
    let record: Record = response.json().await?;
    ensure!(record.id == id, "grant request id mismatch");
    Ok(record)
}

pub(super) async fn post(body: &GrantRequest) -> Result<ShortLink> {
    ensure!(body.tokens.len() <= 20, "too_many_tokens: limit 20");
    let response = client()?.post(format!("{}/api/grant-requests", crate::routes::app_origin())).json(body).send().await?;
    ensure!(response.status() == reqwest::StatusCode::CREATED, "grant request POST HTTP {}: {}", response.status(), response.text().await?);
    let link: ShortLink = response.json().await?;
    ensure!(id(&link.url)? == link.id, "grant request URL/id mismatch");
    Ok(link)
}
