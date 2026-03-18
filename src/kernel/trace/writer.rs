//! Background trace writer — async queue for fire-and-forget DB writes.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::storage::dal::trace::record::SpanRecord;
use crate::storage::dal::trace::record::TraceRecord;
use crate::storage::dal::trace::repo::SpanRepo;
use crate::storage::dal::trace::repo::TraceRepo;

const CHANNEL_CAPACITY: usize = 1024;
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const MAX_BATCH_SIZE: usize = 20;

pub enum TraceOp {
    InsertTrace {
        repo: Arc<TraceRepo>,
        record: TraceRecord,
    },
    UpdateTraceCompleted {
        repo: Arc<TraceRepo>,
        trace_id: String,
        duration_ms: u64,
        input_tokens: u64,
        output_tokens: u64,
        total_cost: f64,
    },
    UpdateTraceFailed {
        repo: Arc<TraceRepo>,
        trace_id: String,
        duration_ms: u64,
    },
    AppendSpan {
        repo: Arc<SpanRepo>,
        record: SpanRecord,
    },
}

#[derive(Clone)]
pub struct TraceWriter {
    tx: mpsc::Sender<TraceOp>,
}

impl TraceWriter {
    /// Create a new writer and spawn the background drain task.
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        tokio::spawn(drain_loop(rx));
        Self { tx }
    }

    /// Create a no-op writer for tests without a Tokio runtime.
    pub fn noop() -> Self {
        let (tx, _rx) = mpsc::channel(1);
        Self { tx }
    }

    /// Send an operation to the background queue. Never blocks; drops on full.
    pub fn send(&self, op: TraceOp) {
        if self.tx.try_send(op).is_err() {
            tracing::warn!("trace writer queue full, dropping op");
        }
    }

    /// Graceful shutdown: close the channel and let the drain loop flush.
    pub async fn shutdown(&self) {
        tracing::info!("trace writer shutting down");
        self.tx.closed().await;
    }
}

struct SpanBatch {
    repo: Arc<SpanRepo>,
    records: Vec<SpanRecord>,
}

async fn drain_loop(mut rx: mpsc::Receiver<TraceOp>) {
    let mut span_batches: Vec<SpanBatch> = Vec::new();
    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            op = rx.recv() => {
                match op {
                    Some(op) => {
                        process_op(op, &mut span_batches).await;
                        let total: usize = span_batches.iter().map(|b| b.records.len()).sum();
                        if total >= MAX_BATCH_SIZE {
                            flush_spans(&mut span_batches).await;
                        }
                    }
                    None => {
                        flush_spans(&mut span_batches).await;
                        tracing::info!("trace writer stopped");
                        return;
                    }
                }
            }
            _ = interval.tick() => {
                flush_spans(&mut span_batches).await;
            }
        }
    }
}

async fn process_op(op: TraceOp, span_batches: &mut Vec<SpanBatch>) {
    match op {
        TraceOp::InsertTrace { repo, record } => {
            if let Err(e) = repo.insert(&record).await {
                tracing::warn!(error = %e, "trace writer: failed to insert trace");
            }
        }
        TraceOp::UpdateTraceCompleted {
            repo,
            trace_id,
            duration_ms,
            input_tokens,
            output_tokens,
            total_cost,
        } => {
            if let Err(e) = repo
                .update_completed(
                    &trace_id,
                    duration_ms,
                    input_tokens,
                    output_tokens,
                    total_cost,
                )
                .await
            {
                tracing::warn!(error = %e, "trace writer: failed to complete trace");
            }
        }
        TraceOp::UpdateTraceFailed {
            repo,
            trace_id,
            duration_ms,
        } => {
            if let Err(e) = repo.update_failed(&trace_id, duration_ms).await {
                tracing::warn!(error = %e, "trace writer: failed to fail trace");
            }
        }
        TraceOp::AppendSpan { repo, record } => {
            if let Some(batch) = span_batches
                .iter_mut()
                .find(|b| Arc::ptr_eq(&b.repo, &repo))
            {
                batch.records.push(record);
            } else {
                span_batches.push(SpanBatch {
                    repo,
                    records: vec![record],
                });
            }
        }
    }
}

async fn flush_spans(batches: &mut Vec<SpanBatch>) {
    for batch in batches.drain(..) {
        for record in &batch.records {
            if let Err(e) = batch.repo.append(record).await {
                tracing::warn!(error = %e, "trace writer: failed to append span");
            }
        }
    }
}
