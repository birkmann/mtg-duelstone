//! Run an [`crate::Api`] call (or any future) off the render loop and get the result over a channel.
//!
//! Why one helper: Bevy systems poll a `Receiver` each frame either way, but *how* the work runs differs —
//! natively a plain thread drives the (blocking-inside) future to completion, in the browser the future
//! is handed to the page's event loop (there are no threads). Call sites stay identical:
//! `let rx = mtg_client::spawn(async move { api.decks(fmt).await });`

use std::future::Future;

use crossbeam_channel::{Receiver, unbounded};

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn<T, F>(fut: F) -> Receiver<T>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let (tx, rx) = unbounded();
    std::thread::Builder::new()
        .name("mtg-task".into())
        .spawn(move || {
            let _ = tx.send(block_on(fut));
        })
        .expect("spawn task thread");
    rx
}

#[cfg(target_arch = "wasm32")]
pub fn spawn<T, F>(fut: F) -> Receiver<T>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    let (tx, rx) = unbounded();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tx.send(fut.await);
    });
    rx
}

/// Drive a future on the current thread (native only — the browser can't block).
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<F: Future>(fut: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread::{self, Thread};

    struct Unpark(Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => thread::park(),
        }
    }
}

/// Milliseconds since the Unix epoch on both platforms (`SystemTime::now` panics in the browser).
pub fn now_millis() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
}
