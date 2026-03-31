//! Invocation-level session acquisition — routes by source + persistence.

use std::sync::Arc;

use super::request::*;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::assembly::cloud::CloudAssembler;
use crate::kernel::session::assembly::cloud::CloudBuildOptions;
use crate::kernel::session::assembly::contract::SessionAssembly;
use crate::kernel::session::assembly::local::LocalAssembler;
use crate::kernel::session::assembly::local::LocalBuildOptions;
use crate::kernel::session::Session;

/// Acquire a session for the given source + persistence combination.
/// - Noop: always creates a new transient session.
/// - Persistent: delegates to session::factory::acquire_cloud_session.
pub async fn acquire_session(
    runtime: &Arc<Runtime>,
    req: &InvocationRequest,
) -> Result<Arc<Session>> {
    match &req.persistence {
        PersistenceMode::Noop => {
            let session_id = crate::base::id::new_session_id();
            let assembly =
                assemble_source(runtime, &req.source, &session_id, &req.session_options).await?;
            Ok(Arc::new(Session::from_assembly(assembly)))
        }
        PersistenceMode::Persistent { session_id } => {
            let (agent_id, user_id) = match &req.source {
                ConfigSource::Cloud { agent_id, user_id } => (agent_id.as_str(), user_id.as_str()),
                ConfigSource::Local => {
                    return Err(ErrorCode::invalid_input(
                        "Local + Persistent is not supported",
                    ));
                }
            };
            crate::kernel::session::factory::acquire_cloud_session_with_opts(
                runtime,
                agent_id,
                session_id,
                user_id,
                CloudBuildOptions {
                    cwd: req.session_options.cwd.clone(),
                    tool_filter: req.session_options.tool_filter.clone(),
                    llm_override: req.session_options.llm_override.clone(),
                },
            )
            .await
        }
    }
}

async fn assemble_source(
    runtime: &Arc<Runtime>,
    source: &ConfigSource,
    session_id: &str,
    opts: &SessionBuildOptions,
) -> Result<SessionAssembly> {
    match source {
        ConfigSource::Local => {
            LocalAssembler {
                runtime: runtime.clone(),
            }
            .assemble(session_id, LocalBuildOptions {
                cwd: opts.cwd.clone(),
                tool_filter: opts.tool_filter.clone(),
                llm_override: opts.llm_override.clone(),
            })
            .await
        }
        ConfigSource::Cloud { agent_id, user_id } => {
            CloudAssembler {
                runtime: runtime.clone(),
            }
            .assemble(agent_id, session_id, user_id, CloudBuildOptions {
                cwd: opts.cwd.clone(),
                tool_filter: opts.tool_filter.clone(),
                llm_override: opts.llm_override.clone(),
            })
            .await
        }
    }
}
