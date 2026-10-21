//! Measure how long importing npm packages takes: `cargo run --release --example import_timing -- <dir> <pkg>...`
use pi_jsrt::{JsRuntime, RuntimeOptions};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("dir");
    let t0 = std::time::Instant::now();
    let rt = JsRuntime::new(RuntimeOptions { cwd: Some(dir.into()), ..Default::default() }).unwrap();
    println!("startup: {:?}", t0.elapsed());
    for pkg in args {
        let t = std::time::Instant::now();
        let r = rt.import(pkg.clone()).await;
        println!("{pkg}: {:?} {}", t.elapsed(), if r.is_ok() { "ok" } else { "ERR" });
    }
}
