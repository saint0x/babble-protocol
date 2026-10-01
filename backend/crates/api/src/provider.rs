use babble_discovery::{
    NativeTemporalScorer, TemporalProvider, TemporalProviderVersion, TemporalRequest,
    TemporalResult,
};
use babble_judgment::{Judgment, JudgmentProvider, JudgmentRequest, ProviderVersion};
use babble_judgment_local::LocalProvider;
use babble_judgment_python::{PythonProvider, WorkerConfig};
use babble_lens::{
    NativeRanker, RankingProvider, RankingProviderVersion, RankingRequest, RankingResult,
};
use babble_types::Result;
use std::{env, path::PathBuf, sync::Arc, time::Duration};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JudgmentConfig {
    Python {
        executable: PathBuf,
        directory: PathBuf,
        timeout_ms: u64,
    },
    RustLocal,
}

impl Default for JudgmentConfig {
    fn default() -> Self {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../algorithms");
        Self::Python {
            executable: directory.join(".venv/bin/python"),
            directory,
            timeout_ms: 5_000,
        }
    }
}

impl JudgmentConfig {
    pub fn from_env() -> std::result::Result<Self, crate::config::ConfigError> {
        Self::from_values(
            env::var("BABBLE_JUDGMENT_PROVIDER").ok().as_deref(),
            env::var_os("BABBLE_ALGORITHMS_DIR").map(PathBuf::from),
            env::var_os("BABBLE_PYTHON_EXECUTABLE").map(PathBuf::from),
            env::var("BABBLE_ALGORITHM_TIMEOUT_MS").ok().as_deref(),
        )
    }

    fn from_values(
        provider: Option<&str>,
        directory: Option<PathBuf>,
        executable: Option<PathBuf>,
        timeout: Option<&str>,
    ) -> std::result::Result<Self, crate::config::ConfigError> {
        let invalid = |name: &'static str| crate::config::ConfigError::InvalidProviderSetting(name);
        match provider.unwrap_or("python") {
            "rust-local" => {
                if directory.is_some() || executable.is_some() || timeout.is_some() {
                    return Err(invalid(
                        "Python settings require BABBLE_JUDGMENT_PROVIDER=python",
                    ));
                }
                Ok(Self::RustLocal)
            }
            "python" => {
                let directory = directory.unwrap_or_else(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../algorithms")
                });
                if directory.as_os_str().is_empty() {
                    return Err(invalid("BABBLE_ALGORITHMS_DIR"));
                }
                let directory = if directory.is_absolute() {
                    directory
                } else {
                    env::current_dir()
                        .map_err(|_| invalid("working directory"))?
                        .join(directory)
                };
                let executable = executable.unwrap_or_else(|| directory.join(".venv/bin/python"));
                if executable.as_os_str().is_empty() {
                    return Err(invalid("BABBLE_PYTHON_EXECUTABLE"));
                }
                let timeout_ms = match timeout {
                    None => 5_000,
                    Some(value) => value
                        .parse::<u64>()
                        .map_err(|_| invalid("BABBLE_ALGORITHM_TIMEOUT_MS"))?,
                };
                if !(1..=120_000).contains(&timeout_ms) {
                    return Err(invalid("BABBLE_ALGORITHM_TIMEOUT_MS"));
                }
                Ok(Self::Python {
                    executable,
                    directory,
                    timeout_ms,
                })
            }
            _ => Err(invalid("BABBLE_JUDGMENT_PROVIDER")),
        }
    }

    pub fn start(&self) -> Result<ServerProvider> {
        match self {
            Self::RustLocal => Ok(ServerProvider::RustLocal(LocalProvider::default())),
            Self::Python {
                executable,
                directory,
                timeout_ms,
            } => {
                let provider = PythonProvider::new(WorkerConfig {
                    executable: executable.clone(),
                    args: vec!["-I".into(), "-m".into(), "babble_algorithms.worker".into()],
                    working_directory: Some(directory.clone()),
                    timeout: Duration::from_millis(*timeout_ms),
                })?;
                Ok(ServerProvider::Python(Arc::new(provider)))
            }
        }
    }
}

#[derive(Clone)]
pub enum ServerProvider {
    Python(Arc<PythonProvider>),
    RustLocal(LocalProvider),
}

impl JudgmentProvider for ServerProvider {
    fn version(&self) -> ProviderVersion {
        match self {
            Self::Python(provider) => JudgmentProvider::version(provider.as_ref()),
            Self::RustLocal(provider) => provider.version(),
        }
    }

    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        match self {
            Self::Python(provider) => provider.judge(request),
            Self::RustLocal(provider) => provider.judge(request),
        }
    }

    fn judge_before(
        &self,
        request: &JudgmentRequest,
        deadline: std::time::Instant,
    ) -> Result<Judgment> {
        match self {
            Self::Python(provider) => provider.judge_before(request, deadline),
            Self::RustLocal(provider) => provider.judge_before(request, deadline),
        }
    }

    fn supported_definitions(&self) -> Vec<babble_judgment::DefinitionId> {
        match self {
            Self::Python(provider) => provider.supported_definitions(),
            Self::RustLocal(provider) => provider.supported_definitions(),
        }
    }

    fn privacy_policy(&self) -> babble_judgment::JudgmentPrivacyPolicy {
        match self {
            Self::Python(provider) => provider.privacy_policy(),
            Self::RustLocal(provider) => provider.privacy_policy(),
        }
    }
}

impl RankingProvider for ServerProvider {
    fn version(&self) -> RankingProviderVersion {
        match self {
            Self::Python(provider) => RankingProvider::version(provider.as_ref()),
            Self::RustLocal(_) => RankingProvider::version(&NativeRanker),
        }
    }

    fn rank(&self, request: &RankingRequest) -> Result<RankingResult> {
        match self {
            Self::Python(provider) => provider.rank(request),
            Self::RustLocal(_) => NativeRanker.rank(request),
        }
    }
}

impl TemporalProvider for ServerProvider {
    fn version(&self) -> TemporalProviderVersion {
        match self {
            Self::Python(provider) => TemporalProvider::version(provider.as_ref()),
            Self::RustLocal(_) => TemporalProvider::version(&NativeTemporalScorer),
        }
    }

    fn score(&self, request: &TemporalRequest) -> Result<TemporalResult> {
        match self {
            Self::Python(provider) => provider.score(request),
            Self::RustLocal(_) => NativeTemporalScorer.score(request),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_is_default_and_uses_an_installed_isolated_environment() {
        let config = JudgmentConfig::from_values(None, None, None, None).unwrap();
        let JudgmentConfig::Python {
            executable,
            directory,
            timeout_ms,
        } = config
        else {
            panic!("wrong default")
        };
        assert!(executable.ends_with("algorithms/.venv/bin/python"));
        assert!(directory.is_absolute());
        assert_eq!(timeout_ms, 5_000);
    }

    #[test]
    fn explicit_configuration_is_validated_without_silent_fallback() {
        for timeout in ["0", "120001", "-1", "not-a-number"] {
            assert!(JudgmentConfig::from_values(None, None, None, Some(timeout)).is_err());
        }
        assert!(JudgmentConfig::from_values(Some("unknown"), None, None, None).is_err());
        assert!(JudgmentConfig::from_values(Some("rust-local"), None, None, Some("5000")).is_err());
        assert!(JudgmentConfig::from_values(None, Some(PathBuf::new()), None, None).is_err());
        assert!(JudgmentConfig::from_values(None, None, Some(PathBuf::new()), None).is_err());
        assert_eq!(
            JudgmentConfig::from_values(Some("rust-local"), None, None, None).unwrap(),
            JudgmentConfig::RustLocal
        );
        let config = JudgmentConfig::from_values(
            None,
            Some(PathBuf::from("custom")),
            Some(PathBuf::from("python3")),
            Some("2000"),
        )
        .unwrap();
        let JudgmentConfig::Python {
            executable,
            directory,
            timeout_ms,
        } = config
        else {
            panic!("wrong provider")
        };
        assert_eq!(executable, PathBuf::from("python3"));
        assert!(directory.ends_with("custom"));
        assert!(directory.is_absolute());
        assert_eq!(timeout_ms, 2000);
    }
}
