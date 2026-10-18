use hermes_rs::persistence::{
    AppendMessage, ModelUsageDelta, OpenOptions, Patience, ReadTxn, SessionStore,
    StockSqliteBackend, WriteTxn,
};

#[test]
fn batch_append_and_usage_round_trip_and_guards() {
    let path = std::env::temp_dir().join(format!("hrs109-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = StockSqliteBackend::open(OpenOptions::read_write(path.clone()))
        .unwrap()
        .0;
    let sid = "hrs109-wire-session";
    store
        .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
            tx.ensure_session(sid, "test", None)?;
            let msgs = (0..3)
                .map(|i| AppendMessage {
                    role: "user".into(),
                    content: Some(format!("message-{i}")),
                    ..Default::default()
                })
                .collect::<Vec<_>>();
            let report = tx.append_messages_batch(sid, &msgs)?;
            assert!(report.first_id > 0);
            assert_eq!(report.appended, 3);
            tx.record_model_usage(
                sid,
                &ModelUsageDelta {
                    model: "test-model".into(),
                    api_call_count: 2,
                    input_tokens: 123,
                    output_tokens: 45,
                    ..Default::default()
                },
            )?;
            let empty = tx.append_messages_batch(sid, &[])?;
            assert_eq!((empty.first_id, empty.appended), (0, 0));
            Ok(())
        })
        .unwrap();
    let rows = store
        .read(&mut |tx: &mut dyn ReadTxn| tx.get_messages(sid, false, false, Some(10), 0))
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|m| m.content.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec!["message-0", "message-1", "message-2"]
    );
    let usage: (i64, i64, i64) = rusqlite::Connection::open(&path).unwrap().query_row("SELECT api_call_count, input_tokens, output_tokens FROM session_model_usage WHERE session_id=?1", [sid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!(usage, (2, 123, 45));
    store
        .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
            tx.end_session(sid, "compression")?;
            Ok(())
        })
        .unwrap();
    let err = store
        .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
            tx.append_messages_batch(sid, &[AppendMessage::default()])
                .map(|_| ())
        })
        .unwrap_err();
    assert!(matches!(
        err,
        hermes_rs::persistence::PersistenceError::CompressionClosed
    ));
    let _ = std::fs::remove_file(path);
}
