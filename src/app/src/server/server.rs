use std::sync::Arc;

use axum::routing::get;
use axum::routing::post;
use axum::Router;
use bend_base::logx;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;

use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::server::handler;

pub(crate) struct AppState {
    pub(crate) agent: Mutex<Option<bend_agent::Agent>>,
    pub(crate) llm: LlmConfig,
}

pub async fn start(conf: Config) -> Result<()> {
    let llm = conf.active_llm();
    let store_backend = match conf.store.backend {
        crate::conf::StoreBackend::Fs => "fs",
        crate::conf::StoreBackend::Cloud => "cloud",
    };
    let store_target = match conf.store.backend {
        crate::conf::StoreBackend::Fs => conf.store.fs.root_dir.display().to_string(),
        crate::conf::StoreBackend::Cloud => conf.store.cloud.endpoint.clone(),
    };
    let model = llm.model.clone();
    let base_url = llm.base_url.clone().unwrap_or_default();
    let provider = conf.llm.provider.clone();

    let state = Arc::new(AppState {
        agent: Mutex::new(None),
        llm,
    });

    let app = Router::new()
        .route("/", get(handler::index))
        .route("/api/new", post(handler::new_session))
        .route("/api/chat", post(handler::chat))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", conf.server.host, conf.server.port);
    logx!(
        info,
        "server",
        "configured",
        addr = %addr,
        provider = ?provider,
        model = %model,
        base_url = %base_url,
        store_backend = store_backend,
        store_target = %store_target,
    );
    logx!(info, "server", "listening", addr = %addr,);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| BendclawError::Run(format!("failed to bind {addr}: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| BendclawError::Run(format!("server error: {e}")))?;

    Ok(())
}
