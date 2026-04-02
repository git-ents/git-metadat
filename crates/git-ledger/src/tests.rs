use super::*;
use git2::Repository;

fn init_repo() -> (tempfile::TempDir, Repository) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    let mut config = repo.config().unwrap();
    config.set_str("user.name", "test").unwrap();
    config.set_str("user.email", "test@test").unwrap();

    (dir, repo)
}

const PREFIX: &str = "refs/test/records";

#[test]
fn create_sequential() {
    let (_dir, repo) = init_repo();

    let entry = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[
                Mutation::Set("title", b"hello"),
                Mutation::Set("status", b"open"),
            ],
            "create record",
            None,
        )
        .unwrap();

    assert_eq!(entry.id, "1");
    assert_eq!(entry.ref_, format!("{}/1", PREFIX));
    assert_eq!(entry.fields.len(), 2);
}

#[test]
fn create_sequential_increments() {
    let (_dir, repo) = init_repo();

    let e1 = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[Mutation::Set("a", b"1")],
            "first",
            None,
        )
        .unwrap();
    let e2 = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[Mutation::Set("a", b"2")],
            "second",
            None,
        )
        .unwrap();

    assert_eq!(e1.id, "1");
    assert_eq!(e2.id, "2");
}

#[test]
fn create_caller_provided() {
    let (_dir, repo) = init_repo();

    let entry = repo
        .create(
            PREFIX,
            &IdStrategy::CallerProvided("my-record"),
            &[Mutation::Set("title", b"test")],
            "create",
            None,
        )
        .unwrap();

    assert_eq!(entry.id, "my-record");
}

#[test]
fn create_content_addressed() {
    let (_dir, repo) = init_repo();

    let entry = repo
        .create(
            PREFIX,
            &IdStrategy::ContentAddressed(b"some content"),
            &[Mutation::Set("data", b"value")],
            "create",
            None,
        )
        .unwrap();

    // ID should be a hex OID
    assert_eq!(entry.id.len(), 40);
}

#[test]
fn create_duplicate_errors() {
    let (_dir, repo) = init_repo();

    repo.create(
        PREFIX,
        &IdStrategy::CallerProvided("dup"),
        &[Mutation::Set("a", b"1")],
        "first",
        None,
    )
    .unwrap();
    let result = repo.create(
        PREFIX,
        &IdStrategy::CallerProvided("dup"),
        &[Mutation::Set("a", b"2")],
        "second",
        None,
    );
    assert!(result.is_err());
}

#[test]
fn read_record() {
    let (_dir, repo) = init_repo();

    let created = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[
                Mutation::Set("title", b"hello"),
                Mutation::Set("status", b"open"),
            ],
            "create",
            None,
        )
        .unwrap();

    let read = repo.read(&created.ref_).unwrap();
    assert_eq!(read.id, "1");
    assert_eq!(read.fields.len(), 2);

    let title = read.fields.iter().find(|(k, _)| k == "title").unwrap();
    assert_eq!(title.1, b"hello");
}

#[test]
fn read_missing_errors() {
    let (_dir, repo) = init_repo();
    let result = repo.read("refs/test/nonexistent");
    assert!(result.is_err());
}

#[test]
fn update_record() {
    let (_dir, repo) = init_repo();

    let created = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[
                Mutation::Set("title", b"hello"),
                Mutation::Set("status", b"open"),
            ],
            "create",
            None,
        )
        .unwrap();

    let updated = repo
        .update(
            &created.ref_,
            &[Mutation::Set("status", b"closed")],
            "close record",
        )
        .unwrap();

    let status = updated.fields.iter().find(|(k, _)| k == "status").unwrap();
    assert_eq!(status.1, b"closed");
    assert_ne!(updated.commit, created.commit);
}

#[test]
fn update_delete_field() {
    let (_dir, repo) = init_repo();

    let created = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[
                Mutation::Set("title", b"hello"),
                Mutation::Set("status", b"open"),
            ],
            "create",
            None,
        )
        .unwrap();

    let updated = repo
        .update(
            &created.ref_,
            &[Mutation::Delete("status")],
            "remove status",
        )
        .unwrap();

    assert!(!updated.fields.iter().any(|(k, _)| k == "status"));
    assert!(updated.fields.iter().any(|(k, _)| k == "title"));
}

#[test]
fn update_add_field() {
    let (_dir, repo) = init_repo();

    let created = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[Mutation::Set("title", b"hello")],
            "create",
            None,
        )
        .unwrap();

    let updated = repo
        .update(
            &created.ref_,
            &[Mutation::Set("priority", b"high")],
            "add priority",
        )
        .unwrap();

    assert_eq!(updated.fields.len(), 2);
}

#[test]
fn list_records() {
    let (_dir, repo) = init_repo();

    repo.create(
        PREFIX,
        &IdStrategy::Sequential,
        &[Mutation::Set("a", b"1")],
        "first",
        None,
    )
    .unwrap();
    repo.create(
        PREFIX,
        &IdStrategy::Sequential,
        &[Mutation::Set("a", b"2")],
        "second",
        None,
    )
    .unwrap();
    repo.create(
        PREFIX,
        &IdStrategy::Sequential,
        &[Mutation::Set("a", b"3")],
        "third",
        None,
    )
    .unwrap();

    let ids = repo.list(PREFIX).unwrap();
    assert_eq!(ids, vec!["1", "2", "3"]);
}

#[test]
fn list_empty() {
    let (_dir, repo) = init_repo();
    let ids = repo.list(PREFIX).unwrap();
    assert!(ids.is_empty());
}

#[test]
fn history_tracks_updates() {
    let (_dir, repo) = init_repo();

    let created = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[Mutation::Set("status", b"open")],
            "create",
            None,
        )
        .unwrap();

    repo.update(
        &created.ref_,
        &[Mutation::Set("status", b"in-progress")],
        "update 1",
    )
    .unwrap();
    repo.update(
        &created.ref_,
        &[Mutation::Set("status", b"closed")],
        "update 2",
    )
    .unwrap();

    let history = repo.history(&created.ref_).unwrap();
    assert_eq!(history.len(), 3);
}

#[test]
fn create_with_nested_fields() {
    let (_dir, repo) = init_repo();

    let entry = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[
                Mutation::Set("meta/priority", b"high"),
                Mutation::Set("title", b"hello"),
            ],
            "create",
            None,
        )
        .unwrap();

    // Fields should be read back from the tree, matching what read() returns
    let read_entry = repo.read(&entry.ref_).unwrap();
    assert_eq!(entry.fields, read_entry.fields);
}

#[test]
fn delete_nested_field() {
    let (_dir, repo) = init_repo();

    let created = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[
                Mutation::Set("meta/priority", b"high"),
                Mutation::Set("title", b"hello"),
            ],
            "create",
            None,
        )
        .unwrap();

    let updated = repo
        .update(
            &created.ref_,
            &[Mutation::Delete("meta/priority")],
            "remove nested",
        )
        .unwrap();

    // The nested field should be gone
    assert!(!updated.fields.iter().any(|(k, _)| k == "meta/priority"));
    // The non-nested field should remain
    assert!(updated.fields.iter().any(|(k, _)| k == "title"));
}

#[test]
fn delete_nested_field_removes_empty_parent() {
    let (_dir, repo) = init_repo();

    let created = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[Mutation::Set("meta/priority", b"high")],
            "create",
            None,
        )
        .unwrap();

    let updated = repo
        .update(
            &created.ref_,
            &[Mutation::Delete("meta/priority")],
            "remove nested",
        )
        .unwrap();

    // Both the nested field and its now-empty parent should be gone
    assert!(updated.fields.is_empty());
}

#[test]
fn create_commit_oid() {
    let (_dir, repo) = init_repo();

    let entry = repo
        .create(
            PREFIX,
            &IdStrategy::CommitOid,
            &[
                Mutation::Set("title", b"hello"),
                Mutation::Set("status", b"open"),
            ],
            "create record",
            None,
        )
        .unwrap();

    // ID should be the commit OID (40 hex chars)
    assert_eq!(entry.id.len(), 40);
    assert_eq!(entry.id, entry.commit.to_string());
    // Ref should contain the commit OID
    assert_eq!(entry.ref_, format!("{}/{}", PREFIX, entry.id));
    // Should be readable back
    let read = repo.read(&entry.ref_).unwrap();
    assert_eq!(read.fields, entry.fields);
}

#[test]
fn pin_blob_in_create() {
    let (_dir, repo) = init_repo();

    let existing_blob = repo.blob(b"pinned content").unwrap();

    let entry = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[
                Mutation::Set("title", b"hello"),
                Mutation::Pin("objects/pinned", existing_blob, FileMode::Blob),
            ],
            "create with pin",
            None,
        )
        .unwrap();

    // The tree entry for "objects/pinned" must point at the pre-existing blob OID.
    let commit = repo.find_commit(entry.commit).unwrap();
    let tree = commit.tree().unwrap();
    let pinned = tree
        .get_path(std::path::Path::new("objects/pinned"))
        .unwrap();
    assert_eq!(pinned.id(), existing_blob);
    assert_eq!(pinned.filemode(), 0o100644);
}

#[test]
fn pin_gitlink_in_update() {
    let (_dir, repo) = init_repo();

    let entry = repo
        .create(
            PREFIX,
            &IdStrategy::Sequential,
            &[Mutation::Set("title", b"hello")],
            "create",
            None,
        )
        .unwrap();

    // Use the record's own commit OID as the gitlink target.
    let target_oid = entry.commit;

    let updated = repo
        .update(
            &entry.ref_,
            &[Mutation::Pin(
                "objects/target",
                target_oid,
                FileMode::Commit,
            )],
            "pin gitlink",
        )
        .unwrap();

    let commit = repo.find_commit(updated.commit).unwrap();
    let tree = commit.tree().unwrap();
    let pinned = tree
        .get_path(std::path::Path::new("objects/target"))
        .unwrap();
    assert_eq!(pinned.id(), target_oid);
    assert_eq!(pinned.filemode(), 0o160000);
}
