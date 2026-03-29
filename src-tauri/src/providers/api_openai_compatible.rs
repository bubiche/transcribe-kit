pub const PROVIDER_ID: &str = "openai-compatible";

#[derive(Debug, Clone)]
pub struct ApiCredentials {
    pub api_key: String,
    pub base_url: String,
}

impl ApiCredentials {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.api_key.trim().is_empty() {
            return Err("API key is required.");
        }

        if !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://")) {
            return Err("Base URL must start with http:// or https://");
        }

        Ok(())
    }
}

