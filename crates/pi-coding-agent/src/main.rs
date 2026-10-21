//! The `pi` binary (port of `packages/coding-agent/src/cli.ts`).

fn main() {
    pi_coding_agent::cli::setup::setup_cli();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    if let Err(err) = runtime.block_on(pi_coding_agent::main(&args, None)) {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
