// Stub bench file. The `[[bench]]` entry in Cargo.toml wires this in;
// `harness = false` means we must provide our own `main`. There are no
// benchmark groups defined yet — keep the file as a no-op main so the
// target compiles and the version-gate's `cargo clippy --all-targets`
// can run. Replace with real `criterion_group! / criterion_main!` when
// benchmarks are added.
fn main() {}
