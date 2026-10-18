DF-HRS-109 throughput evidence

Measured on this host by `cargo test --test hrs109_throughput_note -- --nocapture` using fresh stock-SQLite state, the same 50 messages, and one `store.write` transaction per arm. Batch arm: append_messages_batch(50). Comparison arm: 50 calls to append_message in one write closure. This isolates per-row session counter updates while preserving the enclosing transaction.

Observed run: batch = 17,036,757 ns; 50×append_message = 18,907,933 ns; ratio = 0.901× (batch / append). Test passed its <=2.0× regression bound. This is a small single-run smoke measurement, not a general performance guarantee. Message inserts are still per-row; stock SQLite consolidates session counter updates into one UPDATE at batch end.
