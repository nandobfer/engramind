use engramind::{
    config::Secret,
    core::pepper::{self, PEPPER_CHECK_KEY, PepperCheck, PepperError},
    store::{SqliteStore, Store},
};

use crate::support::TestContext;

#[tokio::test]
async fn migrations_from_scratch_are_idempotent() {
    let context = TestContext::unmigrated();
    let first = context.store.migrate().await.expect("first migrate");
    assert_eq!(first.before, 0);
    assert_eq!(first.after, first.expected);
    assert_eq!(first.newly_applied(), first.expected);

    let second = context.store.migrate().await.expect("second migrate");
    assert_eq!(second.newly_applied(), 0);

    let reopened = SqliteStore::open(&context.dir.path().join("engramind.db"), 1).expect("reopen");
    assert_eq!(
        reopened.migrate().await.expect("migrate").newly_applied(),
        0
    );
    assert!(
        reopened
            .ready()
            .await
            .expect("ready")
            .migrations
            .is_current()
    );
}

#[tokio::test]
async fn diagnose_reports_integrity_extensions_and_wal() {
    let context = TestContext::new().await;
    let report = context.store.diagnose().await.expect("diagnose");
    assert_eq!(report.integrity, "ok");
    assert!(report.migrations.is_current());
    assert!(
        report
            .vec_version
            .is_some_and(|version| version.starts_with('v'))
    );
    assert!(report.fts5);
    assert_eq!(report.journal_mode, "wal");
}

#[tokio::test]
async fn meta_round_trips_and_overwrites() {
    let context = TestContext::new().await;
    assert_eq!(context.store.meta("k").await.expect("meta"), None);
    context.store.set_meta("k", b"um").await.expect("set");
    context
        .store
        .set_meta("k", b"dois")
        .await
        .expect("overwrite");
    assert_eq!(
        context.store.meta("k").await.expect("meta"),
        Some(b"dois".to_vec())
    );
}

#[tokio::test]
async fn a_different_pepper_is_refused() {
    let context = TestContext::new().await;
    let pepper = Secret::new("pepper-original-com-mais-de-32-caracteres".to_owned());
    let other = Secret::new("pepper-diferente-com-mais-de-32-caracteres".to_owned());

    assert_eq!(
        pepper::enforce(&context.store, &pepper).await,
        Ok(PepperCheck::Enrolled)
    );
    let stored = context.store.meta(PEPPER_CHECK_KEY).await.expect("meta");
    assert!(stored.is_some_and(|value| value.len() == 32));
    assert_eq!(
        pepper::enforce(&context.store, &pepper).await,
        Ok(PepperCheck::Verified)
    );
    assert_eq!(
        pepper::enforce(&context.store, &other).await,
        Err(PepperError::Mismatch)
    );
}
