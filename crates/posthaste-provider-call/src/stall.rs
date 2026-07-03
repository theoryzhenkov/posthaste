//! The blob **stall-deadline** stream adapter (D81/F2).
//!
//! F2's defect was a *total* timeout on a *streaming* body: a 20 MB attachment
//! on a slow-but-alive link fails deterministically forever. The fix is a
//! between-chunks read-deadline — the download is allowed to take as long as it
//! keeps making progress, and fails only when it *stalls* (no bytes for the
//! stall window). This is that deadline expressed as a stream adapter: it wraps
//! any `Stream<Item = Result<T, E>>` and, per item, arms a `stall`-long timer;
//! if an item does not arrive before the timer fires it yields
//! [`StallError::Stalled`] and ends. Being generic over the item/error types
//! keeps it testable with a synthetic in-memory stream under virtual time — no
//! socket, no real clock.

use std::time::Duration;

use async_stream::stream;
use futures_util::{Stream, StreamExt};

/// An error surfaced by [`stall_guard`]: either the wrapped stream stalled
/// (no item within the deadline) or the wrapped stream itself errored.
#[derive(Debug)]
pub enum StallError<E> {
    /// No item arrived within the stall window — the read is dead, not slow.
    Stalled,
    /// The underlying stream yielded an error.
    Inner(E),
}

/// Wrap `inner` in a between-items stall deadline.
///
/// Each pull of the next item is bounded by `stall`; if none arrives in time the
/// returned stream yields [`StallError::Stalled`] once and terminates. A slow
/// stream that keeps delivering items *before* each deadline runs indefinitely —
/// which is exactly the large-blob-on-a-slow-link case F2 broke.
pub fn stall_guard<S, T, E>(
    inner: S,
    stall: Duration,
) -> impl Stream<Item = Result<T, StallError<E>>>
where
    S: Stream<Item = Result<T, E>>,
{
    stream! {
        futures_util::pin_mut!(inner);
        loop {
            match tokio::time::timeout(stall, inner.next()).await {
                // The stall timer fired before the next item: the read is dead.
                Err(_elapsed) => {
                    yield Err(StallError::Stalled);
                    return;
                }
                // The wrapped stream completed cleanly.
                Ok(None) => return,
                // Progress: forward the chunk and re-arm the timer next loop.
                Ok(Some(Ok(item))) => yield Ok(item),
                // The wrapped stream errored: forward once and stop.
                Ok(Some(Err(error))) => {
                    yield Err(StallError::Inner(error));
                    return;
                }
            }
        }
    }
}

/// Drain a byte stream to a `Vec<u8>` under a stall deadline (the blob path's
/// consumer of [`stall_guard`]). Returns `Err(None)` on stall, `Err(Some(e))`
/// on an underlying stream error.
pub(crate) async fn drain_with_stall<S, T, E>(
    inner: S,
    stall: Duration,
) -> Result<Vec<u8>, Option<E>>
where
    S: Stream<Item = Result<T, E>>,
    T: AsRef<[u8]>,
{
    let guarded = stall_guard(inner, stall);
    futures_util::pin_mut!(guarded);
    let mut buffer = Vec::new();
    while let Some(item) = guarded.next().await {
        match item {
            Ok(chunk) => buffer.extend_from_slice(chunk.as_ref()),
            Err(StallError::Stalled) => return Err(None),
            Err(StallError::Inner(error)) => return Err(Some(error)),
        }
    }
    Ok(buffer)
}
