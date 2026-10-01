use babble_judgment::{
    Judgment, JudgmentPrivacyPolicy, JudgmentProvider, JudgmentRegistry, JudgmentRequest,
    ProviderRole, ProviderVersion,
};
use babble_types::{Canonical, Error as CoreError, JudgmentId, Result, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct JevProvider<T = ReqwestTransport> {
    config: JevConfig,
    transport: T,
}

#[derive(Clone, Debug)]
pub struct JevConfig {
    pub endpoint: String,
    pub api_key: Option<String>,
    pub model: String,
    pub model_version: String,
    pub timeout: Duration,
}

impl JevConfig {
    pub fn new(
        endpoint: impl Into<String>,
        model: impl Into<String>,
        model_version: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            api_key: None,
            model: model.into(),
            model_version: model_version.into(),
            timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JevRequest {
    pub definition: String,
    pub state: babble_judgment::JudgmentState,
    pub parameters: serde_json::Map<String, Value>,
    pub input_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JevResponse {
    pub output: Value,
    pub confidence: f64,
    pub model: Option<String>,
    pub model_version: Option<String>,
}

pub trait JevTransport: Clone + Send + Sync + 'static {
    fn evaluate(&self, config: &JevConfig, request: &JevRequest) -> Result<JevResponse>;
}

#[derive(Clone, Debug, Default)]
pub struct ReqwestTransport;

impl JevProvider<ReqwestTransport> {
    pub fn new(config: JevConfig) -> Self {
        Self {
            config,
            transport: ReqwestTransport,
        }
    }
}

impl<T: JevTransport> JevProvider<T> {
    pub fn with_transport(config: JevConfig, transport: T) -> Self {
        Self { config, transport }
    }
}

impl<T: JevTransport> JudgmentProvider for JevProvider<T> {
    fn version(&self) -> ProviderVersion {
        ProviderVersion {
            provider: "jev".to_string(),
            model: self.config.model.clone(),
            version: self.config.model_version.clone(),
        }
    }

    fn role(&self) -> ProviderRole {
        ProviderRole::Remote
    }

    fn privacy_policy(&self) -> JudgmentPrivacyPolicy {
        JudgmentPrivacyPolicy::remote_minimized()
    }

    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        JudgmentRegistry::babble_core().validate_request(request)?;
        let input_hash = request.state.canonical_hash()?;
        let jev_request = JevRequest {
            definition: request.definition.as_str().to_string(),
            state: request.state.clone(),
            parameters: request.parameters.clone().into_iter().collect(),
            input_hash: input_hash.to_string(),
        };
        let response = self.transport.evaluate(&self.config, &jev_request)?;
        let provider = ProviderVersion {
            provider: "jev".to_string(),
            model: response
                .model
                .clone()
                .unwrap_or_else(|| self.config.model.clone()),
            version: response
                .model_version
                .clone()
                .unwrap_or_else(|| self.config.model_version.clone()),
        };
        let commitment = (
            request.definition.clone(),
            provider.clone(),
            input_hash.clone(),
            request.parameters.clone(),
            response.output.clone(),
            response.confidence.to_bits(),
        );
        let judgment = Judgment {
            id: JudgmentId::from_hash(&commitment.canonical_hash()?),
            definition: request.definition.clone(),
            provider,
            input_hash,
            output: response.output,
            confidence: response.confidence.clamp(0.0, 1.0),
            created_at: Timestamp::now(),
        };
        JudgmentRegistry::babble_core().validate_output(&judgment.definition, &judgment.output)?;
        Ok(judgment)
    }
}

impl JevTransport for ReqwestTransport {
    fn evaluate(&self, config: &JevConfig, request: &JevRequest) -> Result<JevResponse> {
        let client = reqwest::blocking::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|err| provider_error(format!("build Jev HTTP client: {err}")))?;
        let mut builder = client.post(&config.endpoint).json(request);
        if let Some(api_key) = &config.api_key {
            builder = builder.bearer_auth(api_key);
        }
        let response = builder
            .send()
            .map_err(|err| provider_error(format!("send Jev request: {err}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(provider_error(format!(
                "Jev provider returned HTTP {status}"
            )));
        }
        response
            .json::<JevResponse>()
            .map_err(|err| provider_error(format!("decode Jev response: {err}")))
    }
}

fn provider_error(message: String) -> CoreError {
    CoreError::Conflict(format!("judgment provider error: {message}"))
}
