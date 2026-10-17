# Hermes RS

An independent Rust project establishing a foundation for future work. This repository currently contains only a buildable crate scaffold; it does not implement Hermes Agent functionality and has no installable artifact yet.

This project is independent and is not an official Nous Research release. Upstream reference: [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent).

## Principles

- Build on explicit compatibility contracts; unknown or untested surfaces remain unimplemented.
- Track upstream releases and changes without inferring support from their existence.
- Keep the project foundation small until an implementation task establishes a tested scope.

## Status

Project status: foundation scaffold only. No agent subsystem is implemented, and the repository does not currently provide an installable artifact.

## Development

### Rust

Install rustup using the instructions at https://rustup.rs/; rustup provides the Rust compiler and Cargo. The repository selects the stable toolchain channel and requests the `rustfmt` and `clippy` components through `rust-toolchain.toml`.

From a clean checkout, run:

```sh
cargo build
cargo test
```

The test suite includes library unit coverage and an integration test that runs the built executable. There is no published package or installation command yet; build and run the scaffold from this repository.

Optional: the test suite runs additional file-compatibility checks against the system `sqlite3` CLI (a different SQLite build than the one the crate links). It is not required to build or to run the core suite, but install it if you want full test coverage — the tests that require it detect its absence and skip with a recorded reason instead of failing:

```sh
# Debian/Ubuntu
sudo apt-get install -y sqlite3
```

Skipped tests are reported on stderr (visible with `cargo test -- --nocapture`) and appended to `hrs010-skipped-tests.log` in the Cargo target directory (default `target/`).

## License

MIT. See `LICENSE`.
