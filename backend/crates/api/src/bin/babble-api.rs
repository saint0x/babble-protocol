use babble_api::{config::ServerConfig, serve::serve};

#[tokio::main]
async fn main() -> Result<(), babble_api::serve::ServeError> {
    serve(ServerConfig::from_env()?).await
}
