//! Property and integration tests for the `git-store` crate.

use proptest::prelude::*;
use tempfile::TempDir;

use crate::store::GitStore;
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
