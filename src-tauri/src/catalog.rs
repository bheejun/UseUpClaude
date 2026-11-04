use serde::{Deserialize, Serialize};

const MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_PAGES: usize = 8;

/// Chat API key only. Admin keys are refused and never sent.
pub fn chat_key_for_catalog(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() || crate::secrets::is_admin_api_key(trimmed) {
        return None;
    }
    Some(trimmed.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedModel {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub models: Vec<ListedModel>,
    pub error_code: Option<String>,
}

impl ModelCatalog {
    pub fn no_key() -> Self {
        Self {
            models: Vec::new(),
            error_code: Some("no_key".into()),
        }
    }

    fn error(code: &str) -> Self {
        Self {
            models: Vec::new(),
            error_code: Some(code.to_string()),
        }
    }

    fn ready(models: Vec<ListedModel>) -> Self {
        Self {
            models,
            error_code: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ModelsPage {
    #[serde(default)]
    data: Vec<ModelRow>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    last_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelRow {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
}

pub fn models_from_pages(raw_pages: &[&str]) -> Result<Vec<ListedModel>, ()> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for raw in raw_pages {
        let page: ModelsPage = serde_json::from_str(raw).map_err(|_| ())?;
        for row in page.data {
            push_model(&mut out, &mut seen, row);
        }
    }
    Ok(out)
}

fn push_model(out: &mut Vec<ListedModel>, seen: &mut std::collections::BTreeSet<String>, row: ModelRow) {
    let id = row.id.trim().to_string();
    if id.is_empty() || id.starts_with("sk-ant") || !seen.insert(id.clone()) {
        return;
    }
    let display_name = row
        .display_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.clone());
    out.push(ListedModel { id, display_name });
}

/// GET /v1/models with the chat key. The key is a header only, never the URL or a log line.
pub async fn fetch_models(chat_key: &str) -> ModelCatalog {
    if crate::secrets::is_admin_api_key(chat_key) {
        return ModelCatalog::no_key();
    }
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
    {
        Ok(client) => client,
        Err(_) => return ModelCatalog::error("failed"),
    };

    let mut models = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut after: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let page = match get_page(&client, chat_key, after.as_deref()).await {
            Ok(page) => page,
            Err("unauthorized") => return ModelCatalog::error("unauthorized"),
            Err(_) => return ModelCatalog::error("failed"),
        };
        let more = page.has_more;
        let next = page
            .last_id
            .clone()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty());
        for row in page.data {
            push_model(&mut models, &mut seen, row);
        }
        if !more {
            return ModelCatalog::ready(models);
        }
        let Some(next) = next else {
            return ModelCatalog::error("failed");
        };
        if after.as_ref() == Some(&next) {
            return ModelCatalog::error("failed");
        }
        after = Some(next);
    }
    ModelCatalog::error("failed")
}

async fn get_page(
    client: &reqwest::Client,
    chat_key: &str,
    after_id: Option<&str>,
) -> Result<ModelsPage, &'static str> {
    let mut request = client
        .get(MODELS_URL)
        .header("x-api-key", chat_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .query(&[("limit", "100")]);
    if let Some(after_id) = after_id {
        request = request.query(&[("after_id", after_id)]);
    }
    let response = request.send().await.map_err(|_| "failed")?;
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

    #[test]
    fn admin_key_is_not_used_to_list_models() {
        assert!(chat_key_for_catalog(Some("sk-ant-admin01-example")).is_none());
        assert!(chat_key_for_catalog(Some("  ")).is_none());
        assert!(chat_key_for_catalog(None).is_none());
        assert_eq!(
            chat_key_for_catalog(Some("  sk-ant-api03-example  ")).as_deref(),
            Some("sk-ant-api03-example")
        );
    }

    #[test]
    fn parses_model_list_and_skips_blank_ids() {
        let raw = r#"{"data":[
            {"id":"claude-sonnet-4-5","display_name":"Claude Sonnet 4.5","type":"model"},
            {"id":"  ","display_name":"blank"},
            {"id":"claude-sonnet-4-5","display_name":"again"},
            {"id":"claude-haiku-4-5","display_name":""}
        ],"has_more":false,"last_id":"claude-haiku-4-5"}"#;
        let models = models_from_pages(&[raw]).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "claude-sonnet-4-5");
        assert_eq!(models[0].display_name, "Claude Sonnet 4.5");
        assert_eq!(models[1].display_name, "claude-haiku-4-5");
    }
}
