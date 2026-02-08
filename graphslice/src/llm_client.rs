use anyhow::{Result, anyhow};
use reqwest::Client;
use serde_json::json;
use std::env;

#[derive(Clone)]
pub struct LlmClient {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl LlmClient {
    pub fn new() -> Result<Self> {
        let api_key = env::var("LLM_API_KEY").unwrap_or_else(|_| "dummy".to_string());
        // Default to a local endpoint or standard OpenAI one.
        // Users can override via env vars.
        let base_url = env::var("LLM_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let model = env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

        Ok(Self {
            client: Client::new(),
            api_key,
            base_url,
            model,
        })
    }

    /// Send a prompt to the LLM and get the response text
    pub async fn completion(&self, prompt: &str) -> Result<String> {
        // Mock mode for testing
        if env::var("GRAPHSLICE_TEST_MODE").is_ok() {
            // Return just the content string, as the real implementation extracts this from the JSON response
            return Ok("```json\n{\n  \"calls\": [\"helper\"],\n  \"types\": []\n}\n```".to_string());
        }

        if self.api_key == "dummy" {
            // If no API key is provided, we can't really make a call.
            // For now, return a placeholder or error.
            return Err(anyhow!("LLM_API_KEY not set. Cannot use Fuzzy Slicer."));
        }

        let url = format!("{}/chat/completions", self.base_url);

        // Ensure url doesn't have double slashes if base_url ended with /
        let url = if self.base_url.ends_with('/') {
            format!("{}chat/completions", self.base_url)
        } else {
            url
        };

        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": "You are a Rust expert helping to analyze code dependencies. Output only the requested JSON or code, no markdown fencing unless requested."},
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.1
        });

        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("LLM request failed: {}", error_text));
        }

        let json: serde_json::Value = response.json().await?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Invalid response format from LLM"))?;

        Ok(content.to_string())
    }
}
