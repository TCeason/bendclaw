//! Resolve outbound delivery through the same registry used for inbound transports.

use super::MessageSink;
use crate::conf::ChannelsConfig;
use crate::error::EvotError;
use crate::error::Result;

pub struct ResolvedDelivery {
    pub sink: Box<dyn MessageSink>,
    pub targets: Vec<String>,
}

pub fn resolve_delivery(
    channels: &ChannelsConfig,
    channel: &str,
    target: &str,
) -> Result<ResolvedDelivery> {
    let registration = super::super::registry::CHANNELS
        .iter()
        .find(|entry| entry.name == channel)
        .ok_or_else(|| EvotError::Run(format!("unsupported delivery channel: {channel}")))?;
    (registration.resolve_delivery)(channels, target)
}
