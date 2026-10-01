use crate::{PythonProvider, contract, transport, unavailable};
use babble_lens::{RankingProvider, RankingProviderVersion, RankingRequest, RankingResult};
use babble_types::Result;
use std::time::Instant;

impl RankingProvider for PythonProvider {
    fn version(&self) -> RankingProviderVersion {
        contract::ranking_provider()
    }

    fn rank(&self, request: &RankingRequest) -> Result<RankingResult> {
        let deadline = Instant::now() + self.config.timeout;
        request.validate()?;
        let mut state = self.lock(deadline)?;
        if state.worker.is_none() {
            state.start(&self.config, deadline)?;
        }
        let result = (|| {
            let id = state.id();
            let response =
                state.exchange(&contract::Request::rank(id, request.clone()), deadline)?;
            let contract::WorkerResult::Rank(result) = response else {
                return Err(unavailable("unexpected ranking result"));
            };
            result
                .validate_for(request, &contract::ranking_provider())
                .map_err(|_| unavailable("invalid ranking result"))?;
            transport::remaining(deadline)?;
            Ok(result)
        })();
        if result.is_err() {
            state.worker.take();
        }
        result
    }
}
