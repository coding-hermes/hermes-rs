# Hermes RS

An independent Rust project with a narrow, implemented Hermes-shaped persistence slice: a stock-SQLite session store and `hermes-rs-session-adapter`, a newline-delimited JSON-RPC stdio boundary for a standard-library Python client. It is not a full implementation of Hermes Agent.

This project is independent and is not an official Nous Research release. Upstream reference: [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent).

## Principles

- Build on explicit compatibility contracts; unknown or untested surfaces remain unimplemented.
- Track upstream releases and changes without inferring support from their existence.
- Keep scope explicit: this repository does not claim full-agent, universal Python, or general parity support.

## Status

The implemented slice is the stock-SQLite session store and the `hermes-rs-session-adapter` stdio boundary that exposes it to a standard-library Python client. It does not include agent orchestration or full Hermes parity. There is no published package or installable artifact yet; use it from a source checkout.

Read the [persistence contract](specs/persistence-shim.md), [Python interoperability boundary](specs/python-interop.md), [usage guide](skills/hermes-rs-usage/SKILL.md), and [dogfood reports](docs/dogfood/) for the tested scope, wire contract, operational guidance, and real-use evidence.

## Development

### Rust

Install rustup using the instructions at https://rustup.rs/; rustup provides the Rust compiler and Cargo. The repository selects the stable toolchain channel and requests the `rustfmt` and `clippy` components through `rust-toolchain.toml`.

From a clean checkout, build both binaries:

```sh
cargo build --bin hermes-rs --bin hermes-rs-session-adapter
cargo test --workspace
```

The test suite includes library unit coverage and integration tests that exercise the built executables. There is no published package or installable artifact yet; build the binaries from this source checkout.

Optional: the test suite runs additional file-compatibility checks against the system `sqlite3` CLI (a different SQLite build than the one the crate links). It is not required to build or to run the core suite, but install it if you want full test coverage — the tests that require it detect its absence and skip with a recorded reason instead of failing:

```sh
# Debian/Ubuntu
sudo apt-get install -y sqlite3
```

Skipped tests are reported on stderr (visible with `cargo test -- --nocapture`) and appended to `hrs010-skipped-tests.log` in the Cargo target directory (default `target/`).

## License

MIT. See `LICENSE`.
