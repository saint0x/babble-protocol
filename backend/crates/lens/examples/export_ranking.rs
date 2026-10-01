//! Generate fixtures/protocol/v1/ranking.json via stdout; never hand-edit goldens.
#[path = "../tests/support/mod.rs"]
mod support;

use babel_lens::{NativeRanker, RankingProvider, RankingRequest, RankingResult};
use serde::Serialize;
use std::io::{self, Write};

#[derive(Serialize)]
struct Case {
    name: String,
    request: RankingRequest,
    result: RankingResult,
}

#[derive(Serialize)]
struct Fixtures {
    version: u32,
    cases: Vec<Case>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cases = support::fixtures()
        .into_iter()
        .map(|(name, request)| {
            let result = NativeRanker.rank(&request)?;
            Ok(Case {
                name,
                request,
                result,
            })
        })
        .collect::<babel_types::Result<Vec<_>>>()?;
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    serde_json::to_writer_pretty(&mut stdout, &Fixtures { version: 1, cases })?;
    writeln!(stdout)?;
    Ok(())
}
