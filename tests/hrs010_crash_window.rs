//! HRS-010 crash-window integration: acknowledged rows survive SIGKILL.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hermes_rs::persistence::api::JournalPreference;
use hermes_rs::persistence::{
    AppendMessage, JournalCapability, OpenOptions, Patience, ReadTxn, SessionStore,
    StockSqliteBackend, WriteTxn,
};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hrs010-crash-{}-{}-{tag}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create crash scratch dir");
    dir.join("state.db")
}

#[test]
fn f3_sigkill_child_acknowledged_rows_survive_for_each_journal_preference() {
    use std::os::unix::process::ExitStatusExt;

    let cases = [
        ("default", JournalPreference::Default),
        ("rollback-only", JournalPreference::RollbackOnly),
        ("no-create", JournalPreference::NoCreate),
    ];
    for (label, preference) in cases {
        let path = scratch(label);
        let ready = path.with_extension("db.child-ready");
        let session = format!(
            "hrs010_crash_{}_{}",
            std::process::id(),
            label.replace('-', "_")
        );
        let seed_session = format!("{session}_seed");
        {
            let (store, _) =
                StockSqliteBackend::open(OpenOptions::read_write(&path)).expect("scaffold store");
            store
                .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
                    tx.ensure_session(&seed_session, "cli", None)?;
                    tx.append_message(
                        &seed_session,
                        &AppendMessage {
                            role: "user".into(),
                            content: Some("parent seed".into()),
                            ..Default::default()
                        },
                    )?;
                    Ok(())
                })
                .expect("write parent seed");
        }
        let exe = std::env::current_exe().expect("test exe");
        let path_arg = path.display().to_string();
        let ready_arg = ready.display().to_string();
        let mut child = std::process::Command::new(exe)
            .args([
                "--exact",
                "crash_child",
                "--nocapture",
                "--ignored",
                "--",
                &path_arg,
                &ready_arg,
                &session,
                label,
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("{label}: child spawn failed: {e}"));
        let deadline = Instant::now() + Duration::from_secs(30);
        while !ready.exists() {
            if let Some(status) = child.try_wait().expect("poll crash child") {
                let output = child
                    .wait_with_output()
                    .expect("collect early child output");
                panic!(
                    "{label}: child exited before readiness ({status}); stdout={} stderr={}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            assert!(
                Instant::now() < deadline,
                "{label}: child readiness timed out after 30s"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            unsafe { kill_child(child.id()) },
            "{label}: SIGKILL syscall succeeded"
        );
        let output = child.wait_with_output().expect("wait/reap SIGKILLed child");
        assert_eq!(
            output.status.signal(),
            Some(9),
            "{label}: child must be killed by SIGKILL, got {:?}",
            output.status
        );
        let libtest = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !libtest.contains("running 0 tests") && !libtest.contains("0 passed"),
            "{label}: child test did not execute: {libtest}"
        );
        assert!(
            libtest.contains("running 1 test"),
            "{label}: expected exactly one child test: {libtest}"
        );

        let (store, opened) = StockSqliteBackend::open(options(&path, preference, false))
            .expect("reopen after SIGKILL");
        assert!(
            opened.report.quarantined.is_none(),
            "{label}: healthy post-kill file must not be quarantined"
        );
        let journal = match opened.capabilities.journal {
            JournalCapability::Wal => "wal",
            JournalCapability::RollbackOnly => "delete/rollback",
        };
        println!("HRS-010 crash-window preference={label} resolved_journal={journal}");
        let seed = store
            .read(&mut |tx: &mut dyn ReadTxn| tx.get_messages(&seed_session, false, false, None, 0))
            .expect("read parent seed");
        assert_eq!(seed.len(), 1);
        assert_eq!(seed[0].content.as_deref(), Some("parent seed"));
        let msgs = store
            .read(&mut |tx: &mut dyn ReadTxn| tx.get_messages(&session, false, false, None, 0))
            .expect("read child rows");
        assert!(
            msgs.len() >= 200,
            "{label}: acknowledged child prefix exists; observed {} rows",
            msgs.len()
        );
        for (i, row) in msgs[..200].iter().enumerate() {
            assert_eq!(
                row.content.as_deref(),
                Some(format!("child burst {i}").as_str()),
                "{label}: exact row content"
            );
            if i > 0 {
                assert_eq!(
                    row.id,
                    msgs[0].id + i as i64,
                    "{label}: contiguous ascending ids"
                );
            }
        }
        drop(store);
        std::fs::remove_file(&ready).expect("remove readiness sentinel");
        std::fs::remove_dir_all(path.parent().expect("scratch parent"))
            .expect("remove crash scratch");
    }
}

fn options(path: &PathBuf, preference: JournalPreference, create: bool) -> OpenOptions {
    let mut options = OpenOptions::read_write(path);
    options.journal = preference;
    options.create_if_missing = create;
    options
}

#[test]
#[ignore = "crash child is driven by the parent test with --exact"]
fn crash_child() {
    let args: Vec<String> = std::env::args().collect();
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .expect("libtest argument separator");
    let args: Vec<&str> = args[separator + 1..]
        .iter()
        .filter(|arg| arg.as_str() != "--")
        .map(String::as_str)
        .collect();
    let path = args.first().expect("database path");
    let ready = args.get(1).expect("readiness sentinel");
    let session = args.get(2).expect("session id");
    let preference = match args.get(3).copied().expect("journal preference") {
        "default" => JournalPreference::Default,
        "rollback-only" => JournalPreference::RollbackOnly,
        "no-create" => JournalPreference::NoCreate,
        other => panic!("unknown journal preference {other}"),
    };
    let (store, opened) =
        StockSqliteBackend::open(options(&PathBuf::from(path), preference, false))
            .expect("child opens existing store");
    let journal = match opened.capabilities.journal {
        JournalCapability::Wal => "wal",
        JournalCapability::RollbackOnly => "delete/rollback",
    };
    println!("HRS-010 crash child preference={preference:?} resolved_journal={journal}");
    store
        .write(Patience::TRANSCRIPT, &mut |tx: &mut dyn WriteTxn| {
            tx.ensure_session(session, "cli", None)?;
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
        .expect("commit acknowledged 200-row transaction");
    std::fs::write(ready, b"committed-200").expect("publish readiness sentinel");
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

#[cfg(unix)]
unsafe fn kill_child(pid: u32) -> bool {
    unsafe { libc_kill(pid as i32, 9) == 0 }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}
