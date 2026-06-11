//! PgAuditSink (T4.1 requirement 2): the daemon's bridge from the
//! runner's SYNC `AuditSink` to the async Postgres `AuditWriter`, with
//! FAIL-SYNCHRONOUS semantics — a failed write surfaces in the SAME
//! `append` call, because "no audit, no trading" (I5) only works if the
//! failure is seen before the next order, never a tick later.
//!
//! Mechanism: ONE dedicated writer thread owning a current-thread tokio
//! runtime and the `AuditWriter`; `append` sends the job and BLOCKS on
//! the reply channel. Why not `block_in_place`/`Handle::block_on` inside
//! `append`: those panic or refuse depending on the caller's runtime
//! flavor (and the journal-generic constructor deadlock earlier tonight
//! was exactly this class of bug). A plain channel round-trip to a
//! thread that owns its own runtime is flavor-proof, preserves write
//! ORDER (single consumer), and contains zero panic paths.
//!
//! Failure posture is fail-closed at every seam: thread-spawn failure,
//! runtime-build failure, a dead thread, or a Postgres error all surface
//! as `RunnerError::AuditFailed`, which the runner answers with a global
//! halt.

use fortuna_core::clock::Clock;
use fortuna_ledger::AuditWriter;
use fortuna_runner::{AuditSink, RunnerError};
use std::sync::mpsc;
use std::sync::Arc;

enum Job {
    Append {
        kind: String,
        ref_id: Option<String>,
        payload: serde_json::Value,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Shutdown,
}

/// Sync facade over the Postgres audit writer. Construct with `spawn`.
pub struct PgAuditSink {
    tx: mpsc::Sender<Job>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl PgAuditSink {
    /// Start the writer thread. If the thread, its runtime, or its pool
    /// cannot be built, the sink still constructs — and every `append`
    /// fails closed (the runner halts on the first audit attempt), which
    /// is the correct posture for a daemon that cannot persist its trail.
    ///
    /// The writer builds its OWN single-connection pool from the caller
    /// pool's connect options: tokio connections are bound to the
    /// reactor that registered them, so REUSING the caller's pool from
    /// this thread's runtime hangs until the acquire timeout (observed
    /// red-first: "pool timed out while waiting for an open connection").
    /// A private pool is also the conservative isolation posture — audit
    /// writes can never queue behind anyone else's connection use.
    pub fn spawn(pool: sqlx::PgPool, clock: Arc<dyn Clock>, id_seed: u64) -> PgAuditSink {
        let connect = (*pool.connect_options()).clone();
        let (tx, rx) = mpsc::channel::<Job>();
        let worker = std::thread::Builder::new()
            .name("pg-audit-writer".to_string())
            .spawn(move || {
                let refuse_all = |rx: mpsc::Receiver<Job>, reason: String| {
                    while let Ok(job) = rx.recv() {
                        match job {
                            Job::Append { reply, .. } => {
                                let _ = reply.send(Err(reason.clone()));
                            }
                            Job::Shutdown => break,
                        }
                    }
                };
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        refuse_all(rx, format!("audit writer runtime failed to build: {e}"));
                        return;
                    }
                };
                let own_pool = match rt.block_on(
                    sqlx::postgres::PgPoolOptions::new()
                        .max_connections(1)
                        .connect_with(connect),
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        refuse_all(rx, format!("audit writer pool failed to connect: {e}"));
                        return;
                    }
                };
                let writer = AuditWriter::new(own_pool, clock, id_seed);
                while let Ok(job) = rx.recv() {
                    match job {
                        Job::Append {
                            kind,
                            ref_id,
                            payload,
                            reply,
                        } => {
                            let result = rt
                                .block_on(writer.append(&kind, None, ref_id.as_deref(), payload))
                                .map(|_| ())
                                .map_err(|e| e.to_string());
                            let _ = reply.send(result);
                        }
                        Job::Shutdown => break,
                    }
                }
            })
            .ok();
        // A failed spawn leaves `worker` None and the receiver dropped:
        // every send errs, every append fails closed.
        PgAuditSink { tx, worker }
    }
}

impl AuditSink for PgAuditSink {
    fn append(
        &mut self,
        kind: &str,
        ref_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<(), RunnerError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(Job::Append {
                kind: kind.to_string(),
                ref_id: ref_id.map(str::to_string),
                payload,
                reply: reply_tx,
            })
            .map_err(|_| RunnerError::AuditFailed {
                reason: "audit writer thread is gone".to_string(),
            })?;
        match reply_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(reason)) => Err(RunnerError::AuditFailed { reason }),
            Err(_) => Err(RunnerError::AuditFailed {
                reason: "audit writer thread died mid-write".to_string(),
            }),
        }
    }
}

impl Drop for PgAuditSink {
    fn drop(&mut self) {
        let _ = self.tx.send(Job::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
