//! Bounded results reported by the authenticated browser host.
use babel_types::{Error, Hash, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const BROWSER_EXECUTOR: &str = "babel.browser.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrowserInvocationResult {
    ClipboardWrite { written: bool },
    FullscreenEnter { entered: bool },
    Failed { code: BrowserFailureCode },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrowserFailureCode {
    NotAllowed,
    Unavailable,
    ContextLost,
    NativeError,
}

impl BrowserFailureCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotAllowed => "not_allowed",
            Self::Unavailable => "unavailable",
            Self::ContextLost => "context_lost",
            Self::NativeError => "native_error",
        }
    }
}

impl BrowserInvocationResult {
    pub fn validate_for(&self, method: &str) -> Result<()> {
        if matches!(
            (method, self),
            (
                "babel.clipboard.write.v2",
                Self::ClipboardWrite { written: true }
            ) | (
                "babel.fullscreen.enter.v2",
                Self::FullscreenEnter { entered: true }
            ) | (
                "babel.clipboard.write.v2" | "babel.fullscreen.enter.v2",
                Self::Failed { .. }
            )
        ) {
            Ok(())
        } else {
            Err(Error::Conflict(
                "browser acknowledgement result does not match invocation".into(),
            ))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserExecutionTicket {
    pub dispatch_id: Hash,
    pub executor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcknowledgeBrowserInvocationRequest {
    pub dispatch_id: Hash,
    pub result: BrowserInvocationResult,
}
