//! The JS thread: Boa context, job queue, timers, handle table and event loop.
//!
//! Everything in this module runs on the dedicated runtime thread. The public,
//! thread-safe API lives in [`crate::runtime`] and talks to this thread by
//! posting [`Msg`]s.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use boa_engine::builtins::promise::{OperationType, PromiseState, ResolvingFunctions};
use boa_engine::context::HostHooks;
use boa_engine::job::{GenericJob, Job, JobExecutor, NativeAsyncJob, PromiseJob};
use boa_engine::object::FunctionObjectBuilder;
use boa_engine::object::builtins::{JsFunction, JsPromise};
use boa_engine::{
    Context, JsArgs, JsNativeError, JsObject, JsResult, JsString, JsValue, NativeFunction, Source, js_string,
};
use futures::StreamExt;
use futures::stream::FuturesUnordered;

use crate::error::{self, Error};
use crate::runtime::{Inner, Shared};
use crate::transpile::SourceMapTable;

/// Work item executed on the JS thread.
pub(crate) type Task = Box<dyn FnOnce(&mut Context) + Send + 'static>;

/// Messages processed by the JS thread's event loop.
pub(crate) enum Msg {
    Task(Task),
    Release(u64),
    Shutdown,
}

/// Value delivered to a Rust caller waiting for a JS result.
pub(crate) enum Settled {
    Json(serde_json::Value),
    Handle(u64),
}

/// Per-thread runtime state, reachable from the context via [`state`].
pub(crate) struct State {
    pub(crate) tx: mpsc::Sender<Msg>,
    pub(crate) rt: Weak<Inner>,
    pub(crate) shared: Arc<Shared>,
    pub(crate) tokio: tokio::runtime::Handle,
    handles: RefCell<HashMap<u64, JsValue>>,
    created: RefCell<Option<Vec<u64>>>,
    pub(crate) host_fns: RefCell<HashMap<u64, JsObject>>,
    timers: RefCell<Timers>,
    pending: RefCell<HashMap<u64, ResolvingFunctions>>,
    pub(crate) source_maps: RefCell<HashMap<String, SourceMapTable>>,
    rejections: RefCell<Vec<JsObject>>,
    pub(crate) internal: RefCell<Option<JsObject>>,
    pub(crate) ops: RefCell<Option<JsObject>>,
    pub(crate) cwd: RefCell<PathBuf>,
    stopping: Cell<bool>,
}

impl State {
    pub(crate) fn next_id(&self) -> u64 {
        self.shared.next_id()
    }

    /// Store a value in the handle table. The entry is "unadopted" until Rust
    /// takes a reference (see [`Shared::adopt`]).
    pub(crate) fn new_handle(&self, value: JsValue) -> u64 {
        let id = self.next_id();
        self.handles.borrow_mut().insert(id, value);
        if let Some(created) = self.created.borrow_mut().as_mut() {
            created.push(id);
        }
        id
    }

    /// Store a value and give Rust one reference to it.
    pub(crate) fn new_owned_handle(&self, value: JsValue) -> u64 {
        let id = self.next_id();
        self.handles.borrow_mut().insert(id, value);
        self.shared.adopt(id);
        id
    }

    pub(crate) fn handle_count(&self) -> usize {
        self.handles.borrow().len()
    }

    pub(crate) fn get_handle(&self, id: u64) -> Option<JsValue> {
        self.handles.borrow().get(&id).cloned()
    }

    fn release(&self, id: u64) {
        if self.shared.release(id) {
            self.handles.borrow_mut().remove(&id);
        }
    }

    /// Drop table entries created during a conversion that Rust did not adopt.
    pub(crate) fn sweep_unadopted(&self, ids: &[u64]) {
        let mut handles = self.handles.borrow_mut();
        for id in ids {
            if !self.shared.is_adopted(*id) {
                handles.remove(id);
            }
        }
    }

    /// Post a task to this thread from anywhere (e.g. a tokio task).
    pub(crate) fn poster(&self) -> Poster {
        Poster { tx: self.tx.clone() }
    }
}

/// Cloneable, `Send` handle for posting tasks to the JS thread.
#[derive(Clone)]
pub(crate) struct Poster {
    tx: mpsc::Sender<Msg>,
}

impl Poster {
    pub(crate) fn post(&self, f: impl FnOnce(&mut Context) + Send + 'static) -> bool {
        self.tx.send(Msg::Task(Box::new(f))).is_ok()
    }
}

/// Fetch the thread state from a context.
pub(crate) fn state(ctx: &Context) -> Rc<State> {
    ctx.get_data::<Rc<State>>().expect("pi-jsrt state missing from context").clone()
}

/// The internal JS helper object created by the prelude.
pub(crate) fn internal(ctx: &Context) -> JsObject {
    try_internal(ctx).expect("pi-jsrt prelude not loaded")
}

/// The internal JS helper object, if the prelude has been loaded.
pub(crate) fn try_internal(ctx: &Context) -> Option<JsObject> {
    ctx.get_data::<Rc<State>>()?.internal.borrow().clone()
}

// ---------------------------------------------------------------------------
// Job executor
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct Executor {
    promise_jobs: RefCell<VecDeque<PromiseJob>>,
    async_jobs: RefCell<VecDeque<NativeAsyncJob>>,
    generic_jobs: RefCell<VecDeque<GenericJob>>,
}

impl Executor {
    fn is_empty(&self) -> bool {
        self.promise_jobs.borrow().is_empty() && self.async_jobs.borrow().is_empty() && self.generic_jobs.borrow().is_empty()
    }
}

impl JobExecutor for Executor {
    fn enqueue_job(self: Rc<Self>, job: Job, context: &mut Context) {
        match job {
            Job::PromiseJob(j) => self.promise_jobs.borrow_mut().push_back(j),
            Job::AsyncJob(j) | Job::FinalizationRegistryCleanupJob(j) => self.async_jobs.borrow_mut().push_back(j),
            Job::GenericJob(j) => self.generic_jobs.borrow_mut().push_back(j),
            Job::TimeoutJob(j) => {
                let delay = Duration::from_millis(j.timeout().as_millis());
                let st = state(context);
                st.timers.borrow_mut().add(
                    st.next_id(),
                    TimerCallback::Native(Some(Box::new(move |ctx: &mut Context| {
                        let _ = j.call(ctx);
                    }))),
                    delay,
                    None,
                );
            }
            Job::IntervalJob(_) => {
                // Not produced by any builtin we expose; our own timers implement setInterval.
            }
            _ => {}
        }
    }

    fn run_jobs(self: Rc<Self>, context: &mut Context) -> JsResult<()> {
        // Our async jobs (module loading) complete synchronously, so a spinning poll is
        // enough. Unlike `futures::executor::block_on` this may be re-entered (e.g. by a
        // synchronous `require()` of an ES module from inside a job).
        let cell = RefCell::new(context);
        let fut = self.run_jobs_async(&cell);
        let mut fut = std::pin::pin!(fut);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(r) = fut.as_mut().poll(&mut cx) {
                return r;
            }
            std::thread::yield_now();
        }
    }

    async fn run_jobs_async(self: Rc<Self>, context: &RefCell<&mut Context>) -> JsResult<()> {
        let mut group = FuturesUnordered::new();
        loop {
            for job in std::mem::take(&mut *self.async_jobs.borrow_mut()) {
                group.push(job.call(context));
            }
            if self.is_empty() && group.is_empty() {
                break;
            }
            if !group.is_empty()
                && let Some(Some(Err(err))) = futures::FutureExt::now_or_never(group.next())
            {
                let ctx = &mut **context.borrow_mut();
                report_uncaught(ctx, err);
            }
            let jobs = std::mem::take(&mut *self.promise_jobs.borrow_mut());
            for job in jobs {
                let result = job.call(&mut context.borrow_mut());
                if let Err(err) = result {
                    let ctx = &mut **context.borrow_mut();
                    report_uncaught(ctx, err);
                }
            }
            let jobs = std::mem::take(&mut *self.generic_jobs.borrow_mut());
            for job in jobs {
                let result = job.call(&mut context.borrow_mut());
                if let Err(err) = result {
                    let ctx = &mut **context.borrow_mut();
                    report_uncaught(ctx, err);
                }
            }
            context.borrow_mut().clear_kept_objects();
        }
        Ok(())
    }
}

/// Run all pending microtasks, then report unhandled rejections.
pub(crate) fn run_microtasks(ctx: &mut Context) {
    if let Err(err) = ctx.run_jobs() {
        report_uncaught(ctx, err);
    }
    let st = state(ctx);
    let rejected = std::mem::take(&mut *st.rejections.borrow_mut());
    for promise in rejected {
        let Ok(p) = JsPromise::from_object(promise) else { continue };
        if let PromiseState::Rejected(reason) = p.state() {
            let err = error::from_thrown(&reason, ctx);
            report(ctx, "Unhandled promise rejection", &err);
        }
    }
}

pub(crate) fn report_uncaught(ctx: &mut Context, err: boa_engine::JsError) {
    let err = error::from_boa(err, ctx);
    report(ctx, "Uncaught exception", &err);
}

pub(crate) fn report(ctx: &mut Context, kind: &str, err: &Error) {
    let st = state(ctx);
    if let Some(cb) = &st.shared.on_uncaught {
        cb(kind, err);
        return;
    }
    let text = match err {
        Error::Js(e) => e.stack.clone().unwrap_or_else(|| e.to_string()),
        other => other.to_string(),
    };
    st.shared.write_stderr(format!("{kind}: {text}\n").as_bytes());
}

// ---------------------------------------------------------------------------
// Host hooks (rejection tracking)
// ---------------------------------------------------------------------------

pub(crate) struct Hooks;

impl HostHooks for Hooks {
    fn promise_rejection_tracker(
        &self,
        promise: &JsObject<boa_engine::builtins::promise::Promise>,
        operation: OperationType,
        context: &mut Context,
    ) {
        let Some(st) = context.get_data::<Rc<State>>().cloned() else { return };
        let obj: JsObject = promise.clone().upcast();
        let mut list = st.rejections.borrow_mut();
        match operation {
            OperationType::Reject => list.push(obj),
            OperationType::Handle => list.retain(|p| !JsObject::equals(p, &obj)),
        }
    }
}

// ---------------------------------------------------------------------------
// Timers
// ---------------------------------------------------------------------------

type NativeTimer = Box<dyn FnOnce(&mut Context)>;

pub(crate) enum TimerCallback {
    Js(JsFunction),
    Native(Option<NativeTimer>),
}

struct TimerEntry {
    callback: TimerCallback,
    interval: Option<Duration>,
    key: (Instant, u64),
}

#[derive(Default)]
pub(crate) struct Timers {
    queue: BTreeMap<(Instant, u64), u64>,
    entries: HashMap<u64, TimerEntry>,
    seq: u64,
}

impl Timers {
    pub(crate) fn add(&mut self, id: u64, callback: TimerCallback, delay: Duration, interval: Option<Duration>) {
        self.seq += 1;
        let key = (Instant::now() + delay, self.seq);
        self.queue.insert(key, id);
        self.entries.insert(id, TimerEntry { callback, interval, key });
    }

    pub(crate) fn cancel(&mut self, id: u64) {
        if let Some(e) = self.entries.remove(&id) {
            self.queue.remove(&e.key);
        }
    }

    pub(crate) fn refresh(&mut self, id: u64, delay: Duration) {
        if let Some(e) = self.entries.get_mut(&id) {
            self.queue.remove(&e.key);
            self.seq += 1;
            e.key = (Instant::now() + delay, self.seq);
            self.queue.insert(e.key, id);
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.queue.keys().next().map(|k| k.0)
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

fn run_due_timers(ctx: &mut Context) {
    let st = state(ctx);
    let now = Instant::now();
    loop {
        let due = {
            let mut timers = st.timers.borrow_mut();
            let Some((&key, &id)) = timers.queue.iter().next() else { break };
            if key.0 > now {
                break;
            }
            timers.queue.remove(&key);
            let Some(mut entry) = timers.entries.remove(&id) else { continue };
            match entry.interval {
                Some(interval) => {
                    let cb = match &entry.callback {
                        TimerCallback::Js(f) => Some(f.clone()),
                        TimerCallback::Native(_) => None,
                    };
                    timers.seq += 1;
                    entry.key = (now + interval, timers.seq);
                    timers.queue.insert(entry.key, id);
                    timers.entries.insert(id, entry);
                    cb.map(Due::Js)
                }
                None => match entry.callback {
                    TimerCallback::Js(f) => Some(Due::Js(f)),
                    TimerCallback::Native(mut f) => f.take().map(Due::Native),
                },
            }
        };
        match due {
            Some(Due::Js(f)) => {
                if let Err(e) = f.call(&JsValue::undefined(), &[], ctx) {
                    report_uncaught(ctx, e);
                }
            }
            Some(Due::Native(f)) => f(ctx),
            None => {}
        }
        run_microtasks(ctx);
    }
}

enum Due {
    Js(JsFunction),
    Native(Box<dyn FnOnce(&mut Context)>),
}

// ---------------------------------------------------------------------------
// Async ops: Rust futures whose results resolve JS promises
// ---------------------------------------------------------------------------

/// A boxed conversion run on the JS thread once an async op has completed.
pub(crate) type Finish = Box<dyn FnOnce(&mut Context) -> JsResult<JsValue> + Send>;

/// Spawn `fut` on tokio and return a promise that settles with the value produced
/// by the returned [`Finish`] closure (run on the JS thread).
pub(crate) fn spawn_promise<F>(ctx: &mut Context, fut: F) -> JsValue
where
    F: Future<Output = Finish> + Send + 'static,
{
    let (promise, resolvers) = JsPromise::new_pending(ctx);
    let st = state(ctx);
    let id = st.next_id();
    st.pending.borrow_mut().insert(id, resolvers);
    let poster = st.poster();
    st.tokio.spawn(async move {
        let finish = fut.await;
        poster.post(move |ctx| {
            let result = finish(ctx);
            settle_pending(ctx, id, result);
        });
    });
    promise.into()
}

/// Spawn a future producing `Result<T, Error>`; `convert` turns `T` into a JS value.
pub(crate) fn spawn_result<T, F, C>(ctx: &mut Context, fut: F, convert: C) -> JsValue
where
    T: Send + 'static,
    F: Future<Output = Result<T, Error>> + Send + 'static,
    C: FnOnce(T, &mut Context) -> JsResult<JsValue> + Send + 'static,
{
    spawn_promise(ctx, async move {
        let r = fut.await;
        Box::new(move |ctx: &mut Context| match r {
            Ok(v) => convert(v, ctx),
            Err(e) => Err(error::to_boa(&e, ctx)),
        }) as Finish
    })
}

fn settle_pending(ctx: &mut Context, id: u64, result: JsResult<JsValue>) {
    let st = state(ctx);
    let Some(resolvers) = st.pending.borrow_mut().remove(&id) else { return };
    let r = match result {
        Ok(v) => resolvers.resolve.call(&JsValue::undefined(), &[v], ctx),
        Err(e) => {
            let v = e.into_opaque(ctx).unwrap_or_else(|e| JsValue::from(js_string!(e.to_string().as_str())));
            resolvers.reject.call(&JsValue::undefined(), &[v], ctx)
        }
    };
    if let Err(e) = r {
        report_uncaught(ctx, e);
    }
}

/// Wait for `value` (awaiting it if it is a promise) and invoke `done` with the outcome.
pub(crate) fn when_settled(ctx: &mut Context, value: JsResult<JsValue>, done: impl FnOnce(&mut Context, Result<JsValue, JsValue>) + 'static) {
    let value = match value {
        Ok(v) => v,
        Err(e) => {
            let v = e.into_opaque(ctx).unwrap_or_else(|e| JsValue::from(js_string!(e.to_string().as_str())));
            done(ctx, Err(v));
            return;
        }
    };
    let Some(promise) = value.as_promise() else {
        done(ctx, Ok(value));
        return;
    };
    type Done = Box<dyn FnOnce(&mut Context, Result<JsValue, JsValue>)>;
    let slot: Rc<RefCell<Option<Done>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    let s1 = slot.clone();
    let s2 = slot;
    // SAFETY: the closures capture only an `Rc<RefCell<..>>` holding a Rust closure
    // that does not own GC-traced values (callers pass reply channels / ids only).
    let on_ok = unsafe {
        NativeFunction::from_closure(move |_, args, ctx| {
            if let Some(f) = s1.borrow_mut().take() {
                f(ctx, Ok(args.get_or_undefined(0).clone()));
            }
            Ok(JsValue::undefined())
        })
    };
    let on_err = unsafe {
        NativeFunction::from_closure(move |_, args, ctx| {
            if let Some(f) = s2.borrow_mut().take() {
                f(ctx, Err(args.get_or_undefined(0).clone()));
            }
            Ok(JsValue::undefined())
        })
    };
    let realm = ctx.realm().clone();
    let on_ok = FunctionObjectBuilder::new(&realm, on_ok).build();
    let on_err = FunctionObjectBuilder::new(&realm, on_err).build();
    if let Err(e) = promise.then(Some(on_ok), Some(on_err), ctx) {
        report_uncaught(ctx, e);
    }
}

// ---------------------------------------------------------------------------
// Stack remapping
// ---------------------------------------------------------------------------

/// Rewrite `file:line:col` positions in a stack trace using transpiler source maps.
pub(crate) fn remap_stack(ctx: &Context, stack: &str) -> String {
    let Some(st) = ctx.get_data::<Rc<State>>() else { return stack.to_string() };
    let maps = st.source_maps.borrow();
    if maps.is_empty() {
        return stack.to_string();
    }
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"([^\s()]+):(\d+):(\d+)").unwrap());
    re.replace_all(stack, |c: &regex::Captures<'_>| {
        let file = &c[1];
        let line: u32 = c[2].parse().unwrap_or(0);
        let col: u32 = c[3].parse().unwrap_or(0);
        match maps.get(file).and_then(|m| m.lookup(line, col)) {
            Some((l, cc)) => format!("{file}:{l}:{cc}"),
            None => c[0].to_string(),
        }
    })
    .into_owned()
}

// ---------------------------------------------------------------------------
// Native function helpers
// ---------------------------------------------------------------------------

/// Signature of the internal ops.
pub(crate) type OpFn = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;

/// Create a JS function from a plain function pointer.
pub(crate) fn make_fn(ctx: &mut Context, name: &str, length: usize, f: OpFn) -> JsFunction {
    let realm = ctx.realm().clone();
    FunctionObjectBuilder::new(&realm, NativeFunction::from_fn_ptr(f))
        .name(JsString::from(name))
        .length(length)
        .build()
}

/// Define a set of ops on `target`.
pub(crate) fn define_ops(ctx: &mut Context, target: &JsObject, ops: &[(&str, usize, OpFn)]) -> JsResult<()> {
    for (name, len, f) in ops {
        let func = make_fn(ctx, name, *len, *f);
        target.set(JsString::from(*name), func, true, ctx)?;
    }
    Ok(())
}

/// Throw a TypeError.
pub(crate) fn type_error(msg: impl Into<String>) -> boa_engine::JsError {
    JsNativeError::typ().with_message(msg.into()).into()
}

/// Argument helpers.
pub(crate) fn arg_string(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    Ok(args.get_or_undefined(i).to_string(ctx)?.to_std_string_lossy())
}

pub(crate) fn arg_opt_string(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<Option<String>> {
    let v = args.get_or_undefined(i);
    if v.is_null_or_undefined() {
        Ok(None)
    } else {
        Ok(Some(v.to_string(ctx)?.to_std_string_lossy()))
    }
}

pub(crate) fn arg_f64(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<f64> {
    args.get_or_undefined(i).to_number(ctx)
}

pub(crate) fn js_str(s: &str) -> JsValue {
    JsValue::from(JsString::from(s))
}

// ---------------------------------------------------------------------------
// Core ops (handles, timers, output)
// ---------------------------------------------------------------------------

fn op_handle_new(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = state(ctx).new_handle(args.get_or_undefined(0).clone());
    Ok(JsValue::from(id as f64))
}

fn op_handle_get(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = arg_f64(args, 0, ctx)? as u64;
    state(ctx).get_handle(id).ok_or_else(|| type_error(format!("invalid JS handle {id}")))
}

fn op_host_fn_get(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = arg_f64(args, 0, ctx)? as u64;
    state(ctx)
        .host_fns
        .borrow()
        .get(&id)
        .cloned()
        .map(JsValue::from)
        .ok_or_else(|| type_error(format!("invalid host function {id}")))
}

fn op_timer_start(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(f) = args.get_or_undefined(0).as_function() else {
        return Err(type_error("The \"callback\" argument must be of type function"));
    };
    let ms = args.get_or_undefined(1).to_number(ctx)?;
    let ms = if ms.is_finite() && ms > 0.0 { ms } else { 0.0 };
    let repeat = args.get_or_undefined(2).to_boolean();
    let delay = Duration::from_micros((ms * 1000.0) as u64);
    let st = state(ctx);
    let id = st.next_id();
    st.timers.borrow_mut().add(id, TimerCallback::Js(f), delay, repeat.then_some(delay.max(Duration::from_millis(1))));
    Ok(JsValue::from(id as f64))
}

fn op_timer_clear(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = arg_f64(args, 0, ctx)?;
    if id.is_finite() && id >= 0.0 {
        state(ctx).timers.borrow_mut().cancel(id as u64);
    }
    Ok(JsValue::undefined())
}

fn op_timer_refresh(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = arg_f64(args, 0, ctx)? as u64;
    let ms = arg_f64(args, 1, ctx)?.max(0.0);
    state(ctx).timers.borrow_mut().refresh(id, Duration::from_micros((ms * 1000.0) as u64));
    Ok(JsValue::undefined())
}

fn op_write(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let fd = arg_f64(args, 0, ctx)? as i32;
    let data = crate::ops::bytes_arg(args.get_or_undefined(1), ctx)?;
    let st = state(ctx);
    if fd == 2 {
        st.shared.write_stderr(&data);
    } else {
        st.shared.write_stdout(&data);
    }
    Ok(JsValue::undefined())
}

fn op_report_error(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let kind = arg_string(args, 0, ctx)?;
    let err = error::from_thrown(args.get_or_undefined(1), ctx);
    report(ctx, &kind, &err);
    Ok(JsValue::undefined())
}

fn op_remap_stack(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let s = arg_string(args, 0, ctx)?;
    Ok(js_str(&remap_stack(ctx, &s)))
}

fn op_now(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let st = state(ctx);
    Ok(JsValue::from(st.shared.started.elapsed().as_secs_f64() * 1000.0))
}

fn op_hrtime(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let st = state(ctx);
    Ok(JsValue::from(st.shared.started.elapsed().as_nanos() as f64))
}

fn op_time_origin(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let st = state(ctx);
    Ok(JsValue::from(st.shared.time_origin_ms))
}

pub(crate) const CORE_OPS: &[(&str, usize, OpFn)] = &[
    ("handleNew", 1, op_handle_new),
    ("handleGet", 1, op_handle_get),
    ("hostFnGet", 1, op_host_fn_get),
    ("timerStart", 3, op_timer_start),
    ("timerClear", 1, op_timer_clear),
    ("timerRefresh", 2, op_timer_refresh),
    ("write", 2, op_write),
    ("reportError", 2, op_report_error),
    ("remapStack", 1, op_remap_stack),
    ("now", 0, op_now),
    ("hrtime", 0, op_hrtime),
    ("timeOrigin", 0, op_time_origin),
];

// ---------------------------------------------------------------------------
// Thread bootstrap and event loop
// ---------------------------------------------------------------------------

/// Evaluate a glue script of the form `(function (ops, internal) { ... })`.
pub(crate) fn run_glue(ctx: &mut Context, name: &str, src: &str, ops: &JsObject, internal: &JsObject) -> JsResult<()> {
    let wrapped = format!("(function (ops, internal) {{\"use strict\";\n{src}\n}})");
    let path = PathBuf::from(format!("pi-jsrt:{name}"));
    let f = ctx.eval(Source::from_bytes(wrapped.as_bytes()).with_path(&path))?;
    let f = f.as_function().ok_or_else(|| type_error("glue did not evaluate to a function"))?;
    f.call(&JsValue::undefined(), &[ops.clone().into(), internal.clone().into()], ctx)?;
    Ok(())
}

pub(crate) struct ThreadInit {
    pub(crate) rt: Weak<Inner>,
    pub(crate) shared: Arc<Shared>,
    pub(crate) tokio: tokio::runtime::Handle,
    pub(crate) tx: mpsc::Sender<Msg>,
    pub(crate) rx: mpsc::Receiver<Msg>,
    pub(crate) cwd: PathBuf,
}

/// Build the context and run the event loop until shutdown.
pub(crate) fn thread_main(init: ThreadInit, ready: std::sync::mpsc::SyncSender<Result<(), Error>>) {
    let ThreadInit { rt, shared, tokio, tx, rx, cwd } = init;
    let loader = Rc::new(crate::loader::Loader::new(shared.clone()));
    let mut ctx = match Context::builder()
        .job_executor(Rc::new(Executor::default()))
        .module_loader(loader.clone())
        .host_hooks(Rc::new(Hooks))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(Error::host(format!("failed to create JS context: {e}"))));
            return;
        }
    };
    {
        let limits = ctx.runtime_limits_mut();
        limits.set_recursion_limit(shared.options.recursion_limit);
        limits.set_stack_size_limit(shared.options.stack_size_limit);
        limits.set_backtrace_limit(32);
        if let Some(n) = shared.options.loop_iteration_limit {
            limits.set_loop_iteration_limit(n);
        }
    }
    let st = Rc::new(State {
        tx,
        rt,
        shared: shared.clone(),
        tokio,
        handles: RefCell::default(),
        created: RefCell::default(),
        host_fns: RefCell::default(),
        timers: RefCell::default(),
        pending: RefCell::default(),
        source_maps: RefCell::default(),
        rejections: RefCell::default(),
        internal: RefCell::default(),
        ops: RefCell::default(),
        cwd: RefCell::new(cwd),
        stopping: Cell::new(false),
    });
    ctx.insert_data(st.clone());

    if let Err(e) = crate::ops::bootstrap(&mut ctx) {
        let err = error::from_boa(e, &mut ctx);
        let _ = ready.send(Err(err));
        return;
    }
    run_microtasks(&mut ctx);
    let _ = ready.send(Ok(()));

    event_loop(&mut ctx, &rx);

    // Tear down: drop every GC value we hold before the context goes away.
    st.stopping.set(true);
    st.handles.borrow_mut().clear();
    st.host_fns.borrow_mut().clear();
    *st.timers.borrow_mut() = Timers::default();
    st.pending.borrow_mut().clear();
    st.rejections.borrow_mut().clear();
    *st.internal.borrow_mut() = None;
    *st.ops.borrow_mut() = None;
    loader.clear();
    crate::ops::teardown(&mut ctx);
    drop(ctx);
}

fn event_loop(ctx: &mut Context, rx: &mpsc::Receiver<Msg>) {
    let st = state(ctx);
    loop {
        let next = st.timers.borrow().next_deadline();
        let msg = match next {
            Some(deadline) => {
                let now = Instant::now();
                if deadline <= now {
                    Err(mpsc::RecvTimeoutError::Timeout)
                } else {
                    rx.recv_timeout(deadline - now)
                }
            }
            None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match msg {
            Ok(Msg::Task(task)) => {
                task(ctx);
                run_microtasks(ctx);
            }
            Ok(Msg::Release(id)) => st.release(id),
            Ok(Msg::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        run_due_timers(ctx);
    }
}

/// Collect handle ids created while running `f` (used to sweep unadopted handles).
pub(crate) fn collect_created<R>(ctx: &mut Context, f: impl FnOnce(&mut Context) -> R) -> (R, Vec<u64>) {
    let st = state(ctx);
    let prev = st.created.borrow_mut().replace(Vec::new());
    let r = f(ctx);
    let ids = std::mem::replace(&mut *st.created.borrow_mut(), prev).unwrap_or_default();
    if let Some(outer) = st.created.borrow_mut().as_mut() {
        outer.extend(ids.iter().copied());
    }
    (r, ids)
}

/// Number of live timers (for tests / diagnostics).
pub(crate) fn timer_count(ctx: &Context) -> usize {
    state(ctx).timers.borrow().len()
}
