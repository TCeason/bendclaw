//! The one HTTP connection pool for every cloud call.
//!
//! LLM streaming, the model catalog and scheduled tasks all talk to the same
//! host. Sharing the engine's pool means a call after the first rides an
//! open connection instead of paying a TCP + TLS handshake each time, which
//! from a distant region is most of a request's latency. The pool also sets
//! `User-Agent: evot/<version>`, which the server keys compatibility on.

use crate::error::EvotError;
use crate::error::Result;

pub(crate) fn client() -> Result<reqwest::Client> {
    evot_engine::provider::http_client().map_err(|error| EvotError::Conf(error.to_string()))
}
