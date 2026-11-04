//! Ignored crash child included by the HRS-009 parity integration-test target.

use hermes_rs::persistence::{
    AppendMessage, OpenOptions, Patience, SessionStore, StockSqliteBackend, WriteTxn,
};

#[test]
#[ignore = "crash-child harness driven by the parent test via --exact"]
fn crash_child() {
    let args: Vec<String> = std::env::args().collect();
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .expect("libtest argument separator");
    let args = &args[separator + 1..];
    let args: Vec<&str> = args
        .iter()
        .filter(|arg| arg.as_str() != "--")
        .map(String::as_str)
        .collect();
    let path = args.first().expect("database path");
    let backend = args.get(1).copied().unwrap_or("stock");
    let ready = args.get(2).expect("readiness sentinel");
    let session = "20261015_p7_seed";

    match backend {
        "turso" => {
            #[cfg(feature = "turso")]
            {
                use hermes_rs::persistence::TursoBackend;
                let (store, _) = TursoBackend::open(OpenOptions::read_write(path))
                    .expect("open turso child store");
                append_acknowledged_burst(&store, session);
                publish_ready(ready);
                let mut next = 0u64;
                loop {
                    store
                        .write(Patience::TRANSCRIPT, &mut |tx: &mut dyn WriteTxn| {
                            tx.append_message(
                                session,
                                &AppendMessage {
                                    role: "user".into(),
                                    content: Some(format!("unbounded {next}")),
                                    ..Default::default()
                                },
                            )?;
                            Ok(())
                        })
                        .expect("append during kill window");
                    next += 1;
                }
            }
            #[cfg(not(feature = "turso"))]
            panic!("turso child requested without the turso feature");
        }
        "stock" => {
            let (store, _) = StockSqliteBackend::open(OpenOptions::read_write(path))
                .expect("open stock child store");
            append_acknowledged_burst(&store, session);
            publish_ready(ready);
            let mut next = 0u64;
            loop {
                store
                    .write(Patience::TRANSCRIPT, &mut |tx: &mut dyn WriteTxn| {
                        tx.append_message(
                            session,
                            &AppendMessage {
                                role: "user".into(),
                                content: Some(format!("unbounded {next}")),
                                ..Default::default()
                            },
                        )?;
                        Ok(())
                    })
                    .expect("append during kill window");
                next += 1;
            }
        }
        other => panic!("unknown backend {other}"),
    }
}

fn append_acknowledged_burst(store: &impl SessionStore, session: &str) {
    store
        .write(Patience::TRANSCRIPT, &mut |tx: &mut dyn WriteTxn| {
            for i in 0..200u64 {
                tx.append_message(
                    session,
                    &AppendMessage {
                        role: "user".into(),
                        content: Some(format!("child burst {i}")),
                        ..Default::default()
                    },
                )?;
            }
            Ok(())
        })
        .expect("commit acknowledged 200-row burst");
}

fn publish_ready(path: &str) {
    std::fs::write(path, b"committed-200").expect("publish readiness sentinel");
}
