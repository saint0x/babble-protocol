use crate::{PythonProvider, contract, transport, unavailable};
use babel_discovery::{TemporalProvider, TemporalProviderVersion, TemporalRequest, TemporalResult};
use babel_types::Result;
use std::time::Instant;

impl TemporalProvider for PythonProvider {
    fn version(&self) -> TemporalProviderVersion {
        contract::temporal_provider()
    }

    fn score(&self, request: &TemporalRequest) -> Result<TemporalResult> {
        let deadline = Instant::now() + self.config.timeout;
        request.validate()?;
        let mut state = self.lock(deadline)?;
        if state.worker.is_none() {
            state.start(&self.config, deadline)?;
        }
        let result = (|| {
            let id = state.id();
            let response =
                state.exchange(&contract::Request::temporal(id, request.clone()), deadline)?;
            let contract::WorkerResult::Temporal(result) = response else {
                return Err(unavailable("unexpected temporal result"));
            };
            result
                .validate_for(request, &contract::temporal_provider())
                .map_err(|_| unavailable("invalid temporal result"))?;
            transport::remaining(deadline)?;
            Ok(result)
        })();
        if result.is_err() {
            state.worker.take();
        }
        result
    }
}
