use chrono::{DateTime, Datelike, SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};

const COST_REPORT_URL: &str = "https://api.anthropic.com/v1/organizations/cost_report";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_PAGES: usize = 4;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSpend {
    pub state: String,
    pub amount_usd: Option<f64>,
    pub error_code: Option<String>,
}

impl WorkspaceSpend {
    pub fn hidden() -> Self {
        Self {
            state: "hidden".into(),
            amount_usd: None,
            error_code: None,
        }
    }

    fn ok(amount_usd: f64) -> Self {
        Self {
            state: "ok".into(),
            amount_usd: Some(amount_usd),
            error_code: None,
        }
    }

    fn error(error_code: &str) -> Self {
        Self {
            state: "error".into(),
            amount_usd: None,
            error_code: Some(error_code.to_string()),
        }
    }
}

/// UTC calendar month `[start, end)` as RFC3339 timestamps.
pub fn month_bounds(now: DateTime<Utc>) -> (String, String) {
    let start = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("month start");
    let (year, month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    let end = Utc
        .with_ymd_and_hms(year, month, 1, 0, 0, 0)
        .single()
        .expect("next month start");
    (
        start.to_rfc3339_opts(SecondsFormat::Secs, true),
        end.to_rfc3339_opts(SecondsFormat::Secs, true),
    )
}

pub struct CostAmount<'a> {
    pub amount: &'a str,
    pub currency: Option<&'a str>,
    pub workspace_id: Option<&'a str>,
}

/// Amounts are decimal strings in cents. `"123.45"` USD is `$1.2345`.
pub fn sum_amount_usd(rows: &[CostAmount<'_>], workspace_id: &str) -> Result<f64, ()> {
    let mut total = 0.0;
    for row in rows {
        if !row_in_workspace(row.workspace_id, workspace_id) {
            continue;
        }
        if row.currency.unwrap_or("USD") != "USD" {
            return Err(());
        }
        let cents: f64 = row.amount.trim().parse().map_err(|_| ())?;
        if !cents.is_finite() {
            return Err(());
        }
        total += cents / 100.0;
    }
    if !total.is_finite() {
        return Err(());
    }
    Ok(total)
}

fn row_in_workspace(row_workspace: Option<&str>, wanted: &str) -> bool {
    match row_workspace.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => id == wanted,
        None => true,
    }
}

#[derive(Debug, Deserialize)]
struct CostPage {
    #[serde(default)]
    data: Vec<CostBucket>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_page: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CostBucket {
    #[serde(default)]
    results: Vec<CostRow>,
}

#[derive(Debug, Deserialize)]
struct CostRow {
    amount: String,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
}

fn rows_from_pages(pages: &[CostPage]) -> Vec<CostAmount<'_>> {
    pages
        .iter()
        .flat_map(|page| page.data.iter())
        .flat_map(|bucket| bucket.results.iter())
        .map(|row| CostAmount {
            amount: &row.amount,
            currency: row.currency.as_deref(),
            workspace_id: row.workspace_id.as_deref(),
        })
        .collect()
}

pub fn sum_pages_usd(raw_pages: &[&str], workspace_id: &str) -> Result<f64, ()> {
    let mut pages = Vec::new();
    for raw in raw_pages {
        let page: CostPage = serde_json::from_str(raw).map_err(|_| ())?;
        pages.push(page);
    }
    sum_amount_usd(&rows_from_pages(&pages), workspace_id)
}

/// Admin cost report for the current UTC month.
/// The admin key is sent only as the `x-api-key` header, never logged or placed in the URL.
pub async fn fetch_month_spend(admin_key: &str, workspace_id: &str) -> WorkspaceSpend {
    let (start, end) = month_bounds(Utc::now());
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
    {
        Ok(client) => client,
        Err(_) => return WorkspaceSpend::error("failed"),
    };

    let mut pages = Vec::new();
    let mut page_token: Option<String> = None;
    let mut finished = false;
    for _ in 0..MAX_PAGES {
        let page = match fetch_page(&client, admin_key, workspace_id, &start, &end, page_token.as_deref()).await {
            Ok(page) => page,
            Err(code) => return WorkspaceSpend::error(code),
        };
        let more = page.has_more;
        let next = page
            .next_page
            .clone()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());
        pages.push(page);
        if !more {
            finished = true;
            break;
        }
        let Some(next) = next else {
            return WorkspaceSpend::error("failed");
        };
        if page_token.as_ref() == Some(&next) {
            return WorkspaceSpend::error("failed");
        }
        page_token = Some(next);
    }
    if !finished {
        return WorkspaceSpend::error("failed");
    }

    match sum_amount_usd(&rows_from_pages(&pages), workspace_id) {
        Ok(amount) => WorkspaceSpend::ok(amount),
        Err(()) => WorkspaceSpend::error("failed"),
    }
}

async fn fetch_page(
    client: &reqwest::Client,
    admin_key: &str,
    workspace_id: &str,
    start: &str,
    end: &str,
    page_token: Option<&str>,
) -> Result<CostPage, &'static str> {
    let mut query = vec![
        ("starting_at", start.to_string()),
        ("ending_at", end.to_string()),
        ("bucket_width", "1d".to_string()),
        ("limit", "31".to_string()),
    ];
    if let Some(token) = page_token {
        query.push(("page", token.to_string()));
    }

    let response = client
        .get(COST_REPORT_URL)
        .header("x-api-key", admin_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-workspace-id", workspace_id)
        .query(&query)
        .send()
        .await
        .map_err(|_| "failed")?;

    let status = response.status().as_u16();
    if status == 401 {
        return Err("unauthorized");
    }
    if status == 400 {
        return Err("bad_request");
    }
    if !(200..300).contains(&status) {
        return Err("failed");
    }
    response.json().await.map_err(|_| "failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn month_bounds_cover_november_and_roll_december() {
        let november = Utc.with_ymd_and_hms(2026, 11, 4, 15, 30, 0).unwrap();
        assert_eq!(
            month_bounds(november),
            (
                "2026-11-01T00:00:00Z".to_string(),
                "2026-12-01T00:00:00Z".to_string()
            )
        );
        let december = Utc.with_ymd_and_hms(2026, 12, 31, 23, 0, 0).unwrap();
        assert_eq!(
            month_bounds(december),
            (
                "2026-12-01T00:00:00Z".to_string(),
                "2027-01-01T00:00:00Z".to_string()
            )
        );
    }

    #[test]
    fn sums_cents_and_skips_other_workspaces() {
        let raw = r#"{"data":[{"results":[
            {"amount":"123.45","currency":"USD","workspace_id":null},
            {"amount":"100","currency":"USD","workspace_id":"wrkspc_other"},
            {"amount":"50.00","currency":"USD","workspace_id":"wrkspc_mine"}
        ]}],"has_more":false,"next_page":null}"#;
        let total = sum_pages_usd(&[raw], "wrkspc_mine").unwrap();
        assert!((total - 1.7345).abs() < 1e-9);
    }

    #[test]
    fn rejects_non_usd_rows() {
        let raw = r#"{"data":[{"results":[{"amount":"10","currency":"EUR"}]}],"has_more":false}"#;
        assert!(sum_pages_usd(&[raw], "wrkspc_mine").is_err());
    }
}
