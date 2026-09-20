//! Getting the bundled dictionary onto this device, once.
//!
//! The en_US pair is ~860 KB, so it is fetched lazily from the server
//! (`/api/dictionaries/en_US.{aff,dic}`) and then kept in [`rinch_storage`] under a
//! **versioned** key — `dict/en_US/v1/aff` and `.../dic`. Every load after the first
//! is local, which means the spellchecker works offline, and shipping a different
//! word list later is a new version segment rather than a cache invalidation
//! problem.
//!
//! # At most once per run
//!
//! [`speller`] holds the loaded [`Speller`] in a process-wide slot and hands out the
//! same `Rc<RefCell<Speller>>` to everyone. Two callers racing the first load share
//! one fetch: the second parks on the same in-flight request instead of starting a
//! second one, which matters because the loser of that race would otherwise re-parse
//! 79,000 words for nothing.
//!
//! # Why there is a task driver in here
//!
//! `local_store::spawn` is documented as single-poll on native: `FsStore`'s futures
//! resolve on their first poll, so a future that *pends* is dropped rather than
//! driven. That is fine for storage, but the dictionary fetch genuinely pends —
//! `rinch_http::fetch` runs the request on a worker thread and resumes the callback
//! on the UI thread later. So this module carries [`spawn_task`], a minimal
//! main-thread executor that re-polls when woken. On web it is `spawn_local`; on
//! native it is a parked future the waker re-polls in place.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use super::engine::Speller;
use crate::local_store::backend;

/// Cache keys. The `v1` segment is the *dictionary* version, not a schema version:
/// bump it when the bundled word list changes and every device refetches on its own.
const AFF_KEY: &str = "dict/en_US/v1/aff";
const DIC_KEY: &str = "dict/en_US/v1/dic";

const AFF_URL: &str = "/api/dictionaries/en_US.aff";
const DIC_URL: &str = "/api/dictionaries/en_US.dic";

// ── The one-per-run slot ─────────────────────────────────────────────────────

type Shared = Rc<RefCell<Speller>>;
type Waiter = Box<dyn FnOnce(Result<Shared, String>)>;

enum Slot {
    /// A load is in flight; these callbacks are waiting on it.
    Loading(Vec<Waiter>),
    /// Loaded. Everyone shares this handle.
    Ready(Shared),
    /// The load failed. Kept so a retry is a deliberate act rather than something
    /// that happens automatically on every keystroke.
    Failed(String),
}

thread_local! {
    static SLOT: RefCell<Option<Slot>> = const { RefCell::new(None) };
}

/// What [`speller`] has to do once it has looked at the slot.
enum Action {
    /// Answer straight away, outside the slot borrow.
    Deliver(Result<Shared, String>),
    /// Parked behind a load already in flight.
    Parked,
    /// This caller starts the load.
    Start,
}

/// The shared speller, loading it if this is the first ask.
///
/// `on_ready` runs on the UI thread — immediately if the dictionary is already
/// loaded (or already known to have failed), otherwise once the load finishes.
pub fn speller(on_ready: impl FnOnce(Result<Shared, String>) + 'static) {
    // Held in an `Option` so the branch that parks it can take ownership while the
    // branches that answer immediately still have it afterwards.
    let mut waiter: Option<Waiter> = Some(Box::new(on_ready));

    let action = SLOT.with(|s| {
        let mut slot = s.borrow_mut();
        match &mut *slot {
            Some(Slot::Ready(sp)) => Action::Deliver(Ok(sp.clone())),
            Some(Slot::Failed(e)) => Action::Deliver(Err(e.clone())),
            Some(Slot::Loading(waiters)) => {
                waiters.push(waiter.take().expect("waiter"));
                Action::Parked
            }
            None => {
                *slot = Some(Slot::Loading(vec![waiter.take().expect("waiter")]));
                Action::Start
            }
        }
    });

    match action {
        // Outside the borrow: the callback is free to ask for the speller again.
        Action::Deliver(result) => (waiter.take().expect("waiter"))(result),
        Action::Parked => {}
        Action::Start => spawn_task(async move {
            let result = load_speller().await.map(|sp| Rc::new(RefCell::new(sp)));
            let waiters = SLOT.with(|s| {
                let mut slot = s.borrow_mut();
                let waiters = match slot.take() {
                    Some(Slot::Loading(w)) => w,
                    other => {
                        *slot = other;
                        Vec::new()
                    }
                };
                *slot = Some(match &result {
                    Ok(sp) => Slot::Ready(sp.clone()),
                    Err(e) => Slot::Failed(e.clone()),
                });
                waiters
            });
            for w in waiters {
                w(result.clone());
            }
        }),
    }
}

/// Run `f` against the loaded speller, if there is one.
///
/// A no-op when the dictionary has not been loaded yet — which is the right answer
/// rather than a missed update, because whatever `f` would have installed (custom
/// words, entity names) is read from its own source when the load finishes.
pub fn with_loaded_speller(f: impl FnOnce(&mut Speller)) {
    SLOT.with(|s| {
        if let Some(Slot::Ready(sp)) = &*s.borrow() {
            f(&mut sp.borrow_mut());
        }
    });
}

/// Load the dictionary: local cache first, server second, and cache what was
/// fetched so the next run is offline.
///
/// A cache write that fails is logged and ignored — a slow start is better than no
/// spellchecker.
pub async fn load_speller() -> Result<Speller, String> {
    let (aff, dic) = match cached_pair().await {
        Some(pair) => pair,
        None => {
            let aff = fetch_text(AFF_URL).await?;
            let dic = fetch_text(DIC_URL).await?;
            store_pair(&aff, &dic).await;
            (aff, dic)
        }
    };
    Speller::from_hunspell(&aff, &dic)
}

/// Both halves out of local storage, or `None` if either is missing or not UTF-8.
///
/// All-or-nothing on purpose: an affix file without its word list (or a half-written
/// pair from an interrupted first run) is not a dictionary, and refetching is
/// cheaper than reasoning about which half is stale.
async fn cached_pair() -> Option<(String, String)> {
    let store = backend().await.ok()?;
    let aff = store.get(AFF_KEY).await.ok()??;
    let dic = store.get(DIC_KEY).await.ok()??;
    Some((String::from_utf8(aff).ok()?, String::from_utf8(dic).ok()?))
}

/// Cache the pair, writing the word list **before** the affix file.
///
/// The order matters for the same reason the manifest pointer-flip in
/// `local_store` does: [`cached_pair`] requires both keys, and it reads the affix
/// key first, so making that one the last write means an interrupted cache fill
/// never presents itself as a complete cache.
async fn store_pair(aff: &str, dic: &str) {
    let Ok(store) = backend().await else {
        log::warn!("spell: no local store; the dictionary will be refetched next run");
        return;
    };
    if let Err(e) = store.put(DIC_KEY, dic.as_bytes()).await {
        log::warn!("spell: could not cache the word list: {e}");
        return;
    }
    if let Err(e) = store.put(AFF_KEY, aff.as_bytes()).await {
        log::warn!("spell: could not cache the affix file: {e}");
    }
}

/// GET `url` as UTF-8 text through the app's cross-platform HTTP layer.
async fn fetch_text(url: &str) -> Result<String, String> {
    let (send, recv) = oneshot::<Result<Option<Vec<u8>>, crate::api::BinError>>();
    crate::api::get_bytes(url, send);
    let bytes = recv
        .await
        .ok_or_else(|| format!("{url}: the request was dropped"))?
        .map_err(|e| format!("{url}: {} (HTTP {})", e.message, e.status))?
        .ok_or_else(|| format!("{url}: the server returned no content"))?;
    String::from_utf8(bytes).map_err(|_| format!("{url}: not valid UTF-8"))
}

// ── A local oneshot, bridging callback APIs into `.await` ────────────────────

struct OneshotInner<T> {
    value: Option<T>,
    waker: Option<Waker>,
    dropped: bool,
}

/// Receives the value a callback produces. Resolves to `None` if the sender was
/// dropped without being called — which is how rinch reports a callback abandoned
/// because the component that parked it went away.
struct OneshotRecv<T>(Rc<RefCell<OneshotInner<T>>>);

struct OneshotSend<T>(Rc<RefCell<OneshotInner<T>>>);

impl<T> OneshotSend<T> {
    fn send(self, value: T) {
        let mut inner = self.0.borrow_mut();
        inner.value = Some(value);
        let waker = inner.waker.take();
        drop(inner);
        if let Some(w) = waker {
            w.wake();
        }
    }
}

impl<T> Drop for OneshotSend<T> {
    fn drop(&mut self) {
        let mut inner = self.0.borrow_mut();
        if inner.value.is_some() {
            return;
        }
        inner.dropped = true;
        let waker = inner.waker.take();
        drop(inner);
        if let Some(w) = waker {
            w.wake();
        }
    }
}

impl<T> Future for OneshotRecv<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.0.borrow_mut();
        if let Some(v) = inner.value.take() {
            return Poll::Ready(Some(v));
        }
        if inner.dropped {
            return Poll::Ready(None);
        }
        inner.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// A single-use, single-threaded channel: `(send, recv)`.
fn oneshot<T>() -> (impl FnOnce(T), OneshotRecv<T>) {
    let inner = Rc::new(RefCell::new(OneshotInner {
        value: None,
        waker: None,
        dropped: false,
    }));
    let send = OneshotSend(inner.clone());
    (move |v| send.send(v), OneshotRecv(inner))
}

// ── Main-thread task driver ──────────────────────────────────────────────────

/// Run a `!Send` future to completion on the UI thread, **including** across a
/// pend.
///
/// See the module docs for why `local_store::spawn` is not enough here.
#[cfg(target_arch = "wasm32")]
pub fn spawn_task(fut: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(fut);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_task(fut: impl Future<Output = ()> + 'static) {
    native_task::spawn(fut);
}

/// A minimal single-threaded executor: one slot per task, re-polled by its own
/// waker. Everything here runs on the UI thread, so no locking and nothing `Send`.
#[cfg(not(target_arch = "wasm32"))]
mod native_task {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    type Task = Pin<Box<dyn Future<Output = ()>>>;

    thread_local! {
        static TASKS: RefCell<HashMap<u64, Task>> = RefCell::new(HashMap::new());
        static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        /// Tasks woken while they were being polled; drained by the polling loop so
        /// a wake from inside `poll` re-runs rather than being lost.
        static REWAKE: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
        static POLLING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// The waker hands the task id back to this thread. `Wake` demands `Send +
    /// Sync`, which a bare id satisfies; the id is only ever *used* on this thread,
    /// and a wake arriving from elsewhere (there is none today — `rinch_http`
    /// resumes on the UI thread) would simply find no task.
    struct IdWaker(u64);

    impl Wake for IdWaker {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            if POLLING.with(std::cell::Cell::get) {
                REWAKE.with(|r| r.borrow_mut().push(self.0));
            } else {
                poll_task(self.0);
            }
        }
    }

    pub fn spawn(fut: impl Future<Output = ()> + 'static) {
        let id = NEXT_ID.with(|n| {
            let id = n.get();
            n.set(id + 1);
            id
        });
        TASKS.with(|t| t.borrow_mut().insert(id, Box::pin(fut)));
        poll_task(id);
    }

    fn poll_task(id: u64) {
        // Take the future out while polling, so a wake during the poll cannot
        // re-enter it; it is put back (or dropped, if finished) afterwards.
        let Some(mut task) = TASKS.with(|t| t.borrow_mut().remove(&id)) else {
            return;
        };
        let waker = Waker::from(Arc::new(IdWaker(id)));
        let mut cx = Context::from_waker(&waker);

        POLLING.with(|p| p.set(true));
        let done = matches!(task.as_mut().poll(&mut cx), Poll::Ready(()));
        POLLING.with(|p| p.set(false));

        if !done {
            TASKS.with(|t| t.borrow_mut().insert(id, task));
        }

        // Anything woken mid-poll gets its re-poll now.
        let pending = REWAKE.with(|r| std::mem::take(&mut *r.borrow_mut()));
        for woken in pending {
            poll_task(woken);
        }
    }
}
