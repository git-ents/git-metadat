//! Property and integration tests for the `git-store` crate.

use proptest::prelude::*;
use tempfile::TempDir;

use crate::git::GitStore;
use crate::store::Store;
use crate::{ContentAddressable, Ref, Transaction};

fn fresh_store() -> (TempDir, GitStore) {
    let dir = TempDir::new().expect("tempdir");
    let store = GitStore::init(dir.path()).expect("init");
    (dir, store)
}

fn store_blob(store: &GitStore, data: &[u8]) -> gix::ObjectId {
    store.store(&data.to_vec()).expect("store")
}

// ── ContentAddressable laws ───────────────────────────────────────────────────

proptest! {
    /// Law 1: determinism; same input always produces the same hash.
    #[test]
    fn ca_determinism(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let (_dir, store) = fresh_store();
        let h1 = store_blob(&store, &bytes);
        let h2 = store_blob(&store, &bytes);
        prop_assert_eq!(h1, h2);
    }

    /// Law 2: round-trip; retrieve(store(v)) == v.
    #[test]
    fn ca_round_trip(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let (_dir, store) = fresh_store();
        let hash = store_blob(&store, &bytes);
        let retrieved = store.retrieve(&hash).expect("retrieve").expect("present");
        prop_assert_eq!(retrieved, bytes);
    }

    /// Law 3: referential transparency; store(a) == store(b) iff a == b.
    #[test]
    fn ca_referential_transparency(
        a in proptest::collection::vec(any::<u8>(), 0..256),
        b in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let (_dir, store) = fresh_store();
        let ha = store_blob(&store, &a);
        let hb = store_blob(&store, &b);
        prop_assert_eq!(ha == hb, a == b);
    }
}

// ── Pointer (Ref + Transaction) laws ─────────────────────────────────────────

/// Law 1: atomicity; a committed transaction is visible; an uncommitted one is not.
#[test]
fn pointer_atomicity() {
    let (_dir, store) = fresh_store();
    let data = b"atomicity test";
    let oid = store_blob(&store, data);

    let r = store.git_ref("refs/store/test").expect("git_ref");

    // Before commit, the ref does not exist.
    assert_eq!(r.read().expect("read"), None);

    let mut tx = store.transaction();
    tx.stage(&r, None, Some(oid));
    tx.commit().expect("commit");

    // After commit, the ref is visible.
    assert_eq!(r.read().expect("read"), Some(oid));
}

/// Law 2: linearizability; CAS fails when the expected value does not match.
#[test]
fn pointer_linearizability() {
    let (_dir, store) = fresh_store();
    let oid_a = store_blob(&store, b"value-a");
    let oid_b = store_blob(&store, b"value-b");
    let oid_c = store_blob(&store, b"value-c");

    let r = store.git_ref("refs/store/linear").expect("git_ref");

    // Create the ref pointing at oid_a.
    let mut tx = store.transaction();
    tx.stage(&r, None, Some(oid_a));
    tx.commit().expect("create");

    // CAS with wrong expected value must fail.
    let mut tx_bad = store.transaction();
    tx_bad.stage(&r, Some(oid_b), Some(oid_c)); // expected oid_b, but actual is oid_a
    assert!(
        tx_bad.commit().is_err(),
        "CAS with wrong expected must fail"
    );

    // Ref still points at oid_a.
    assert_eq!(r.read().expect("read"), Some(oid_a));
}

/// Law 3: consistency; after a successful CAS, read returns the new value.
#[test]
fn pointer_consistency() {
    let (_dir, store) = fresh_store();
    let oid_a = store_blob(&store, b"before");
    let oid_b = store_blob(&store, b"after");

    let r = store.git_ref("refs/store/consist").expect("git_ref");

    let mut tx = store.transaction();
    tx.stage(&r, None, Some(oid_a));
    tx.commit().expect("create");

    let mut tx2 = store.transaction();
    tx2.stage(&r, Some(oid_a), Some(oid_b));
    tx2.commit().expect("update");

    assert_eq!(r.read().expect("read"), Some(oid_b));
}

// ── Store / Tx integration tests ─────────────────────────────────────────────

fn fresh_db_store(n: u64) -> (TempDir, Store) {
    let dir = TempDir::new().expect("tempdir");
    let s = Store::init(dir.path(), n).expect("init");
    (dir, s)
}

/// Round-trip: write 10 keys across 3 subtree levels, commit, re-read all.
#[test]
fn tx_round_trip() {
    let (_dir, store) = fresh_db_store(0);

    let entries: &[(&[&str], &[u8])] = &[
        (&["a", "x", "one"], b"1"),
        (&["a", "x", "two"], b"2"),
        (&["a", "x", "three"], b"3"),
        (&["a", "y", "four"], b"4"),
        (&["a", "y", "five"], b"5"),
        (&["b", "p", "six"], b"6"),
        (&["b", "p", "seven"], b"7"),
        (&["b", "q", "eight"], b"8"),
        (&["c", "nine"], b"9"),
        (&["c", "ten"], b"10"),
    ];

    // Write all entries in one transaction.
    let mut tx = store.begin().expect("begin");
    for &(path, val) in entries {
        tx.put(path, val.to_vec()).expect("put");
    }
    tx.commit().expect("commit");

    // Read them all back in a new transaction.
    let tx2 = store.begin().expect("begin2");
    for &(path, expected) in entries {
        let got = tx2.get(path).expect("get").expect("present");
        assert_eq!(got, expected, "mismatch at {path:?}");
    }

    // Deleted key is absent.
    let mut tx3 = store.begin().expect("begin3");
    tx3.delete(&["a", "x", "one"]).expect("delete");
    tx3.commit().expect("commit3");

    let tx4 = store.begin().expect("begin4");
    assert_eq!(tx4.get(&["a", "x", "one"]).expect("get"), None);
    assert_eq!(
        tx4.get(&["a", "x", "two"]).expect("get").as_deref(),
        Some(b"2" as &[u8])
    );
}

/// [`list`](crate::Tx::list) returns the correct immediate children.
#[test]
fn tx_list() {
    let (_dir, store) = fresh_db_store(1);

    let mut tx = store.begin().expect("begin");
    tx.put(&["ns", "a", "k1"], b"v1".to_vec()).expect("put");
    tx.put(&["ns", "a", "k2"], b"v2".to_vec()).expect("put");
    tx.put(&["ns", "b", "k3"], b"v3".to_vec()).expect("put");
    tx.commit().expect("commit");

    let tx2 = store.begin().expect("begin2");
    let mut children = tx2.list(&["ns"]).expect("list ns");
    children.sort();
    assert_eq!(children, vec!["a", "b"]);

    let mut leaf_children = tx2.list(&["ns", "a"]).expect("list ns/a");
    leaf_children.sort();
    assert_eq!(leaf_children, vec!["k1", "k2"]);
}

/// Concurrent transactions: one wins, the other retries and succeeds.
#[test]
fn tx_concurrent_retry() {
    let (_dir, store) = fresh_db_store(2);

    // Seed an initial value.
    let mut seed = store.begin().expect("begin");
    seed.put(&["counter"], b"0".to_vec()).expect("put");
    seed.commit().expect("seed commit");

    // Snapshot both transactions at the same point.
    let mut tx_a = store.begin().expect("tx_a begin");
    let mut tx_b = store.begin().expect("tx_b begin");

    tx_a.put(&["counter"], b"a".to_vec()).expect("put a");
    tx_b.put(&["counter"], b"b".to_vec()).expect("put b");

    // Commit tx_a first; tx_b must retry.
    tx_a.commit().expect("tx_a commit");
    tx_b.with_max_retries(3)
        .commit()
        .expect("tx_b commit after retry");

    // The last committed value wins.
    let tx_read = store.begin().expect("read tx");
    let val = tx_read.get(&["counter"]).expect("get").expect("present");
    assert!(val == b"a" || val == b"b", "unexpected value: {val:?}");
}
