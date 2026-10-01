use babel_api::{config::ServerConfig, serve::serve};

#[tokio::main]
async fn main() -> Result<(), babel_api::serve::ServeError> {
    serve(ServerConfig::from_env()?).await
}
