use std::path::Path;

use anyhow::{anyhow, Context, Result};
use job_agent_storage::{
    add_service_provider, delete_service_provider, list_service_providers, NewServiceProvider,
    ServiceProvider,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub async fn view_service_providers<P: AsRef<Path>>(db_path: P) -> Result<Vec<ServiceProvider>> {
    let services = list_service_providers(db_path).await?;
    Ok(services)
}

pub async fn add_service_provider_via_api<P: AsRef<Path>>(
    db_path: P,
    input: NewServiceProvider,
) -> Result<i64> {
    let id = add_service_provider(db_path, input).await?;
    Ok(id)
}

pub async fn delete_service_provider_via_api<P: AsRef<Path>>(
    db_path: P,
    id: i64,
) -> Result<bool> {
    let deleted = delete_service_provider(db_path, id).await?;
    Ok(deleted)
}

pub async fn get_service_provider_via_api<P: AsRef<Path>>(
    db_path: P,
    provider_name: &str,
) -> Result<ServiceProvider> {
    let services = view_service_providers(db_path).await?;
    services
        .into_iter()
        .find(|s| s.provider_name.eq_ignore_ascii_case(provider_name))
        .ok_or_else(|| anyhow!("service provider not found: {provider_name}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiliconFlowChatRequest {
    pub model: String,
    pub messages: Vec<LlmMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SiliconFlowChatResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<SiliconFlowChoice>,
    pub usage: Option<SiliconFlowUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SiliconFlowChoice {
    pub index: u32,
    pub message: SiliconFlowAssistantMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SiliconFlowAssistantMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SiliconFlowUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct SiliconFlowCallInput {
    pub provider_name: String,
    pub messages: Vec<LlmMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

pub async fn call_siliconflow_chat_completion<P: AsRef<Path>>(
    db_path: P,
    input: SiliconFlowCallInput,
) -> Result<SiliconFlowChatResponse> {
    let service = get_service_provider_via_api(db_path, &input.provider_name).await?;

    if service.api_key.trim().is_empty() {
        return Err(anyhow!("service api_key is empty: {}", service.provider_name));
    }
    if service.model_name.trim().is_empty() {
        return Err(anyhow!("service model_name is empty: {}", service.provider_name));
    }

    let endpoint = siliconflow_chat_endpoint(&service.api_url);

    let req = SiliconFlowChatRequest {
        model: service.model_name,
        messages: input.messages,
        temperature: input.temperature,
        max_tokens: input.max_tokens,
        stream: Some(false),
    };

    let client = Client::new();
    let response = client
        .post(&endpoint)
        .bearer_auth(service.api_key)
        .json(&req)
        .send()
        .await
        .with_context(|| format!("failed calling siliconflow endpoint: {endpoint}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        return Err(anyhow!("siliconflow call failed: status={status}, body={body}"));
    }

    let parsed = response
        .json::<SiliconFlowChatResponse>()
        .await
        .context("failed to parse siliconflow response json")?;

    Ok(parsed)
}

fn siliconflow_chat_endpoint(base: &str) -> String {
    let base = base.trim();
    if base.ends_with("/chat/completions") {
        return base.to_string();
    }

    if base.ends_with("/v1") {
        return format!("{base}/chat/completions");
    }

    if base.ends_with('/') {
        format!("{}v1/chat/completions", base)
    } else {
        format!("{base}/v1/chat/completions")
    }
}

#[cfg(test)]
mod tests {
    use super::siliconflow_chat_endpoint;

    #[test]
    fn endpoint_builder_supports_multiple_base_formats() {
        assert_eq!(
            siliconflow_chat_endpoint("https://api.siliconflow.cn"),
            "https://api.siliconflow.cn/v1/chat/completions"
        );
        assert_eq!(
            siliconflow_chat_endpoint("https://api.siliconflow.cn/v1"),
            "https://api.siliconflow.cn/v1/chat/completions"
        );
        assert_eq!(
            siliconflow_chat_endpoint("https://api.siliconflow.cn/v1/chat/completions"),
            "https://api.siliconflow.cn/v1/chat/completions"
        );
    }
}
