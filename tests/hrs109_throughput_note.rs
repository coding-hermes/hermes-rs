use hermes_rs::persistence::{
    AppendMessage, OpenOptions, Patience, SessionStore, StockSqliteBackend, WriteTxn,
};
use std::time::Instant;

#[test]
fn hrs109_batch50_throughput_smoke() {
    let path = std::env::temp_dir().join(format!("hrs109-throughput-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = StockSqliteBackend::open(OpenOptions::read_write(&path))
        .unwrap()
        .0;
    store
        .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
            tx.ensure_session("batch", "bench", None)?;
            tx.ensure_session("single", "bench", None)?;
            Ok(())
        })
        .unwrap();
    let messages: Vec<_> = (0..50)
        .map(|i| AppendMessage {
            role: "user".into(),
            content: Some(format!("payload-{i}")),
            ..Default::default()
        })
        .collect();
    let start = Instant::now();
    store
        .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
            tx.append_messages_batch("batch", &messages)?;
            Ok(())
        })
        .unwrap();
    let batch = start.elapsed();
    let start = Instant::now();
    store
        .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
            for msg in &messages {
                tx.append_message("single", msg)?;
            }
            Ok(())
        })
        .unwrap();
    let singles = start.elapsed();
    eprintln!(
        "HRS109_THROUGHPUT batch50_ns={} singles50_ns={} ratio={:.3}",
        batch.as_nanos(),
        singles.as_nanos(),
        batch.as_secs_f64() / singles.as_secs_f64()
    );
    assert!(
        batch.as_secs_f64() <= singles.as_secs_f64() * 2.0,
        "batch exceeded 2x single-append baseline"
    );
    let _ = std::fs::remove_file(path);
}
