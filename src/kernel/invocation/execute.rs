//! Invocation execution — Runtime::invoke() and run_once().

use std::sync::Arc;

use super::request::*;
use super::session_route::acquire_session;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::run::prompt::PromptRequestMeta;
use crate::kernel::run::result::RunOutput;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::options::RunOptions;
use crate::kernel::session::session_stream::Stream;

/// Convert ConversationContext + RunOptions into neutral PromptRequestMeta.
/// Lives here in invocation/ — run/prompt/* never imports invocation types.
fn build_prompt_meta(context: &ConversationContext, options: &RunOptions) -> PromptRequestMeta {
    let (channel_type, channel_chat_id) = match context {
        ConversationContext::None => (None, None),
        ConversationContext::Channel(ctx) => {
            (Some(ctx.channel_type.clone()), Some(ctx.chat_id.clone()))
        }
    };
    PromptRequestMeta {
        channel_type,
        channel_chat_id,
        system_overlay: options.system_overlay.clone(),
        skill_overlay: options.skill_overlay.clone(),
    }
}

fn validate(req: &InvocationRequest) -> Result<()> {
    if matches!(
        (&req.source, &req.persistence),
        (ConfigSource::Local, PersistenceMode::Persistent { .. })
    ) {
        return Err(ErrorCode::invalid_input(
            "Local + Persistent is not supported",
        ));
    }
    Ok(())
}

impl Runtime {
    /// Execute an invocation request. Returns a Stream — caller decides transport.
    pub async fn invoke(self: &Arc<Self>, req: InvocationRequest) -> Result<Stream> {
        validate(&req)?;
        let session = acquire_session(self, &req).await?;
        let meta = build_prompt_meta(&req.context, &req.options);
        session.run_with_meta(&req.prompt, meta, req.options).await
    }

    /// Convenience: invoke + collect full output.
    pub async fn run_once_invocation(
        self: &Arc<Self>,
        req: InvocationRequest,
    ) -> Result<RunOutput> {
        let stream = self.invoke(req).await?;
        stream.finish_output().await
    }
}
