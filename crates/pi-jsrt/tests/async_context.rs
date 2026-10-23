//! AsyncLocalStorage propagation (promises/await, timers, immediates, nextTick, microtasks,
//! AsyncResource, bind/snapshot, enterWith/exit/disable), async_hooks, and Node's nextTick vs
//! promise job ordering. Expected outputs are Node 22's (scripts run by `pi-jsrt-node`).

use std::process::Command;

const NODE_BIN: &str = env!("CARGO_BIN_EXE_pi-jsrt-node");

fn run(name: &str, script: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, script).unwrap();
    let out = Command::new(NODE_BIN).arg(&path).output().unwrap();
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn async_local_storage_follows_async_continuations() {
    assert_eq!(run("als.js", ALS), ALS_NODE);
}

#[test]
fn next_tick_runs_before_promise_jobs() {
    assert_eq!(run("order.js", ORDER), ORDER_NODE);
}

const ALS: &str = r###"const { AsyncLocalStorage, AsyncResource, executionAsyncId, triggerAsyncId, createHook } = require("async_hooks");
const als = new AsyncLocalStorage();
const other = new AsyncLocalStorage();
const log = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function work(tag) {
  log.push([tag, "start", als.getStore()]);
  await sleep(5);
  log.push([tag, "after await", als.getStore()]);
  await Promise.resolve();
  setTimeout(() => log.push([tag, "timeout", als.getStore()]), 1);
  setImmediate(() => log.push([tag, "immediate", als.getStore()]));
  process.nextTick(() => log.push([tag, "nextTick", als.getStore()]));
  queueMicrotask(() => log.push([tag, "microtask", als.getStore()]));
  other.run("inner", () => log.push([tag, "nested", als.getStore(), other.getStore()]));
  log.push([tag, "after nested", other.getStore()]);
}
log.push(["outside", als.getStore()]);
const p1 = als.run({ id: 1 }, () => work("A"));
const p2 = als.run({ id: 2 }, () => work("B"));
log.push(["sync after run", als.getStore()]);
const r = als.run(7, () => new AsyncResource("X"));
const bound = als.run(8, () => AsyncLocalStorage.bind(() => als.getStore()));
const snap = als.run(9, () => AsyncLocalStorage.snapshot());
Promise.all([p1, p2]).then(async () => {
  await sleep(10);
  log.push(["resource", r.runInAsyncScope(() => als.getStore()), typeof r.asyncId(), r.asyncId() > 1]);
  log.push(["bound", bound(), "snapshot", snap(() => als.getStore())]);
  als.enterWith("entered");
  log.push(["enterWith", als.getStore()]);
  await null;
  log.push(["enterWith after await", als.getStore()]);
  log.push(["exit", als.exit(() => als.getStore()), als.getStore()]);
  als.disable();
  log.push(["disabled", als.getStore()]);
  const ids = [typeof executionAsyncId(), typeof triggerAsyncId()];
  const seen = [];
  const hook = createHook({ init(id, type) { if (type === "Timeout") seen.push("init " + type); }, before() {}, after() {}, destroy() {} }).enable();
  await new Promise((res) => setTimeout(res, 1));
  hook.disable();
  log.push(["hooks", ids, seen]);
  try { createHook({ init: 1 }); } catch (e) { log.push([e.code, e.message]); }
  console.log(JSON.stringify(log));
});
"###;
const ALS_NODE: &str = r###"[["outside",null],["A","start",{"id":1}],["B","start",{"id":2}],["sync after run",null],["A","after await",{"id":1}],["A","nested",{"id":1},"inner"],["A","after nested",null],["A","microtask",{"id":1}],["A","nextTick",{"id":1}],["B","after await",{"id":2}],["B","nested",{"id":2},"inner"],["B","after nested",null],["B","microtask",{"id":2}],["B","nextTick",{"id":2}],["A","immediate",{"id":1}],["B","immediate",{"id":2}],["A","timeout",{"id":1}],["B","timeout",{"id":2}],["resource",7,"number",true],["bound",8,"snapshot",9],["enterWith","entered"],["enterWith after await","entered"],["exit",null,"entered"],["disabled",null],["hooks",["number","number"],["init Timeout"]],["ERR_ASYNC_CALLBACK","hook.init must be a function"]]
"###;
const ORDER: &str = r###"const out = [];
setTimeout(() => out.push("timeout"), 0);
Promise.resolve().then(() => { out.push("promise1"); process.nextTick(() => out.push("tick-in-promise")); }).then(() => out.push("promise2"));
process.nextTick(() => { out.push("tick1"); Promise.resolve().then(() => out.push("promise-in-tick")); process.nextTick(() => out.push("tick-in-tick")); });
queueMicrotask(() => out.push("microtask"));
process.nextTick(() => out.push("tick2"));
setTimeout(() => { out.push("t2"); process.nextTick(() => out.push("t2 tick")); Promise.resolve().then(() => out.push("t2 promise")); }, 5);
setTimeout(() => { out.push("t3"); }, 5);
setTimeout(() => console.log(out.join(",")), 30);
"###;
const ORDER_NODE: &str = r###"tick1,tick2,tick-in-tick,promise1,microtask,promise-in-tick,promise2,tick-in-promise,timeout,t2,t2 tick,t2 promise,t3
"###;
