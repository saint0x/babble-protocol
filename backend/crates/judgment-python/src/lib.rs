//! Persistent Python analysis and ranking. Rust owns resulting durable records.
#![cfg(unix)]

pub mod contract;
mod ranking;
mod temporal;
mod transport;

use babel_judgment::{
    Judgment, JudgmentProvider, JudgmentRegistry, JudgmentRequest, ProviderVersion,
};
use babel_types::{Canonical, Error, JudgmentId, Result, Timestamp};
use contract::{JudgeResult, Request, Response, WorkerResult};
use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard, TryLockError},
    thread,
    time::{Duration, Instant},
};
use transport::Worker;

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub working_directory: Option<PathBuf>,
    /// End-to-end call deadline, including contention and startup after failure.
    pub timeout: Duration,
}

/// One persistent subprocess per provider, serialized without an unbounded queue.
pub struct PythonProvider {
    config: WorkerConfig,
    state: Mutex<State>,
}

struct State {
    worker: Option<Worker>,
    next_id: u64,
}

impl PythonProvider {
    pub fn new(config: WorkerConfig) -> Result<Self> {
        if config.executable.as_os_str().is_empty()
            || config.timeout.is_zero()
            || config.timeout > Duration::from_secs(300)
        {
            return Err(unavailable("invalid worker configuration"));
        }
        let deadline = Instant::now() + config.timeout;
        let mut state = State {
            worker: None,
            next_id: 1,
        };
        state.start(&config, deadline)?;
        Ok(Self {
            config,
            state: Mutex::new(state),
        })
    }

    fn lock(&self, deadline: Instant) -> Result<MutexGuard<'_, State>> {
        loop {
            transport::remaining(deadline)?;
            match self.state.try_lock() {
                Ok(state) => return Ok(state),
                Err(TryLockError::Poisoned(_)) => return Err(unavailable("worker lock poisoned")),
                Err(TryLockError::WouldBlock) => {
                    thread::sleep(Duration::from_millis(1).min(transport::remaining(deadline)?));
                }
            }
        }
    }

    fn judge_with_deadline(
        &self,
        request: &JudgmentRequest,
        deadline: Instant,
    ) -> Result<Judgment> {
        transport::remaining(deadline)?;
        let validated = contract::validate_request(request);
        transport::remaining(deadline)?;
        validated?;
        let input_hash = request.state.canonical_hash()?;
        transport::remaining(deadline)?;
        let mut state = self.lock(deadline)?;
        if state.worker.is_none() {
            state.start(&self.config, deadline)?;
        }
        let result = (|| {
            transport::remaining(deadline)?;
            let id = state.id();
            let response = state.exchange(&Request::judge(id, request.clone()), deadline)?;
            let WorkerResult::Judge(result) = response else {
                return Err(unavailable("unexpected worker result"));
            };
            validate_result(request, &result)?;
            transport::remaining(deadline)?;
            let commitment = (
                request.definition.clone(),
                self.version(),
                input_hash.clone(),
                request.parameters.clone(),
                result.output.clone(),
            );
            let judgment = Judgment {
                id: JudgmentId::from_hash(&commitment.canonical_hash()?),
                definition: request.definition.clone(),
                provider: self.version(),
                input_hash,
                output: result.output,
                confidence: result.confidence,
                created_at: Timestamp::now(),
            };
            transport::remaining(deadline)?;
            Ok(judgment)
        })();
        if result.is_err() {
            state.worker.take();
        }
        result
    }
}

impl JudgmentProvider for PythonProvider {
    fn privacy_policy(&self) -> babel_judgment::JudgmentPrivacyPolicy {
        let mut policy = babel_judgment::JudgmentPrivacyPolicy::local_full();
        policy.allowed_context_keys.extend(
            ["source_agreement", "source_text", "target_text"]
                .into_iter()
                .map(str::to_owned),
        );
        policy
    }

    fn version(&self) -> ProviderVersion {
        contract::provider()
    }

    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        self.judge_with_deadline(request, Instant::now() + self.config.timeout)
    }

    fn judge_before(&self, request: &JudgmentRequest, deadline: Instant) -> Result<Judgment> {
        self.judge_with_deadline(request, deadline.min(Instant::now() + self.config.timeout))
    }
}

impl State {
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = if id == contract::MAX_ID { 1 } else { id + 1 };
        id
    }

    fn start(&mut self, config: &WorkerConfig, deadline: Instant) -> Result<()> {
        self.worker = Some(Worker::spawn(config, deadline)?);
        let id = self.id();
        let result = (|| {
            let WorkerResult::Health(health) = self.exchange(&Request::health(id), deadline)?
            else {
                return Err(unavailable("invalid worker health result"));
            };
            let mut expected = contract::definitions();
            let mut actual = health.supported_definitions;
            expected.sort();
            actual.sort();
            if health.provider != contract::provider()
                || actual != expected
                || health.ranking_provider != contract::ranking_provider()
                || health.temporal_provider != contract::temporal_provider()
            {
                return Err(unavailable("worker health mismatch"));
            }
            transport::remaining(deadline)?;
            Ok(())
        })();
        if result.is_err() {
            self.worker.take();
        }
        result
    }

    fn exchange(&mut self, request: &Request, deadline: Instant) -> Result<WorkerResult> {
        transport::remaining(deadline)?;
        let bytes = contract::encode(request);
        transport::remaining(deadline)?;
        let bytes = bytes?;
        let worker = self
            .worker
            .as_mut()
            .ok_or_else(|| unavailable("worker missing"))?;
        let line = worker.exchange(&bytes, deadline)?;
        transport::remaining(deadline)?;
        let response = contract::decode_for(&line, &request.method);
        transport::remaining(deadline)?;
        let response: Response = response?;
        if response.protocol != contract::Protocol::V1 || response.id != Some(request.id) {
            return Err(unavailable("worker response ID or protocol mismatch"));
        }
        match (response.result, response.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(match error.code {
                contract::ErrorCode::InvalidRequest => invalid("worker rejected request"),
                contract::ErrorCode::UnsupportedDefinition => invalid("worker rejected definition"),
                contract::ErrorCode::AlgorithmFailure => unavailable("worker algorithm failed"),
            }),
            _ => Err(unavailable("invalid worker response envelope")),
        }
    }
}

fn validate_result(request: &JudgmentRequest, result: &JudgeResult) -> Result<()> {
    if result.provider != contract::provider()
        || !result.confidence.is_finite()
        || !(0.0..=1.0).contains(&result.confidence)
        || result
            .output
            .get("confidence")
            .is_some_and(|confidence| confidence.as_f64() != Some(result.confidence))
        || result
            .output
            .get("kind")
            .and_then(serde_json::Value::as_str)
            == Some("cascade")
    {
        return Err(unavailable("invalid worker result"));
    }
    JudgmentRegistry::babel_core()
        .validate_output(&request.definition, &result.output)
        .map_err(|_| unavailable("invalid worker output"))?;
    if request.definition == babel_judgment::DefinitionId::relationship_v1() {
        let expected = request
            .parameters
            .get("relation")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("related");
        if result
            .output
            .get("relation")
            .and_then(serde_json::Value::as_str)
            != Some(expected)
        {
            return Err(unavailable("worker changed requested relation"));
        }
    }
    if request.definition == babel_judgment::DefinitionId::source_agreement_v1() {
        babel_judgment::validate_source_agreement_result(request, &result.output)
            .map_err(|_| unavailable("worker source agreement does not match request"))?;
    }
    Ok(())
}

fn unavailable(message: &'static str) -> Error {
    Error::ProviderUnavailable(format!("Python algorithm worker: {message}"))
}

fn invalid(message: &'static str) -> Error {
    Error::Conflict(format!("Python Judgment request: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_wait_obeys_call_deadline_without_spawning_threads() {
        let provider = PythonProvider {
            config: WorkerConfig {
                executable: "unused".into(),
                args: vec![],
                working_directory: None,
                timeout: Duration::from_millis(30),
            },
            state: Mutex::new(State {
                worker: None,
                next_id: 1,
            }),
        };
        let _held = provider.state.lock().unwrap();
        let start = Instant::now();
        assert!(provider.lock(start + provider.config.timeout).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
