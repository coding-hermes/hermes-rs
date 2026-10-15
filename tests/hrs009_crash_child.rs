//! HRS-009 crash-recovery child: run as `cargo test --test hrs009_backend_parity
//! crash_child -- --ignored -- -- <db-path> <backend>`. Commits a burst of
//! rows, then idles until the parent SIGKILLs it. The parent's assertion is
//! that committed rows survive and no torn row appears.

use hermes_rs::persistence::{
    AppendMessage, OpenOptions, Patience, SessionStore, StockSqliteBackend, WriteTxn,
};

#[test]
#[ignore = "crash-child harness: driven by p7_kill9 via --exact with args"]
fn crash_child() {
    let args: Vec<String> = std::env::args().collect();
    // test-binary arg shape: <bin> --exact crash_child --ignored -- -- <path> <backend>
    let sep = args.iter().position(|a| a == "--").expect("arg separator");
    let rest = &args[sep + 1..];
    // rest may start with another "--" (libtest passthrough)
    let rest: Vec<&String> = rest.iter().filter(|a| a.as_str() != "--").collect();
    let path = rest[0].clone();
    let backend = rest.get(1).map(|s| s.as_str()).unwrap_or("stock");

    match backend {
        "turso" => {
            #[cfg(feature = "turso")]
            {
                let (mut store, _o) =
                    TursoBackend::open(OpenOptions::read_write(&path)).expect("child open turso");
                store
                    .write(Patience::TRANSCRIPT, &mut |tx: &mut dyn WriteTxn| {
                        for i in 0..200u64 {
                            tx.append_message(
                                "20261015_p7_seed",
                                &AppendMessage {
                                    role: "user".into(),
                                    content: Some(format!("child burst {i}")),
                                    ..Default::default()
                                },
                            )?;
                        }
                        Ok(())
                    })
                    .expect("child writes committed");
                println!("child committed 200 rows via turso");
                store.close().ok();
                // Idling here is fine: the parent kills us whenever.
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            #[cfg(not(feature = "turso"))]
            panic!("turso arm requires --features turso");
        }
        _ => {
            let (mut store, _o) =
                StockSqliteBackend::open(OpenOptions::read_write(&path)).expect("child open stock");
            store
                .write(Patience::TRANSCRIPT, &mut |tx: &mut dyn WriteTxn| {
                    for i in 0..200u64 {
                        tx.append_message(
                            "20261015_p7_seed",
                            &AppendMessage {
                                role: "user".into(),
                                content: Some(format!("child burst {i}")),
                                ..Default::default()
                            },
                        )?;
                    }
                    Ok(())
                })
                .expect("child writes committed");
            println!("child committed 200 rows via stock");
            store.close().ok();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

#[cfg(feature = "turso")]
use hermes_rs::persistence::TursoBackend;
