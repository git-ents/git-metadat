//! High-level transactional store: [`Store`] and [`Tx`].

use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use gix::ObjectId;
use gix::bstr::ByteSlice as _;

use crate::store::{Error as StoreError, GitStore};
use crate::{Ref as _, Transaction as _};

/// Errors returned by [`Store`] and [`Tx`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A git-level operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A git object could not be decoded.
    #[error("object decode failed: {0}")]
    Decode(#[from] gix::objs::decode::Error),
    /// A tree operation (edit or write) failed.
    #[error("tree operation failed: {0}")]
    Tree(String),
    /// Writing the store commit failed.
    #[error("commit write failed: {0}")]
    Commit(String),
    /// All retry attempts were exhausted due to concurrent modifications.
    #[error("concurrent modification: max retries ({0}) exceeded")]
    Conflict(usize),
    /// The path has no components.
    #[error("path must have at least one component")]
    EmptyPath,
    /// A path component is invalid.
    #[error("invalid path component {0:?}: must be non-empty and not contain '/' or null bytes")]
    InvalidComponent(String),
}

/// A transactional key-value store backed by a git ref under `refs/db/<n>`.
///
/// Each committed [`Tx`] produces a git commit so the full history is preserved.
pub struct Store {
    pub(crate) git: GitStore,
    ref_name: String,
}

impl Store {
    /// Open an existing repository at `path` and bind to `refs/db/<n>`.
    ///
    /// The ref need not exist yet; it is created on the first committed transaction.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository cannot be opened.
    pub fn open(path: impl AsRef<std::path::Path>, n: u64) -> Result<Self, Error> {
        Ok(Self {
            git: GitStore::open(path)?,
            ref_name: format!("refs/db/{n}"),
        })
    }

    /// Initialize a new git repository at `path` and bind to `refs/db/<n>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository cannot be initialized.
    pub fn init(path: impl AsRef<std::path::Path>, n: u64) -> Result<Self, Error> {
        Ok(Self {
            git: GitStore::init(path)?,
            ref_name: format!("refs/db/{n}"),
        })
    }

    /// Begin a new transaction, snapshotting the current state of the store.
    ///
    /// # Errors
    ///
    /// Returns an error if the store ref cannot be read.
    pub fn begin(&self) -> Result<Tx<'_>, Error> {
        let snapshot_commit = self.git.git_ref(&self.ref_name)?.read()?;
        Ok(Tx {
            store: self,
            snapshot_commit,
            mutations: HashMap::new(),
            max_retries: 3,
        })
    }
}

/// An in-progress transaction against a [`Store`].
///
/// Mutations are buffered in memory and written atomically on [`commit`](Tx::commit) via
/// compare-and-swap against the store ref.  On CAS conflict the transaction re-snapshots and
/// retries up to `max_retries` times.
pub struct Tx<'a> {
    store: &'a Store,
    snapshot_commit: Option<ObjectId>,
    /// path → Some(bytes) for put, None for delete
    mutations: HashMap<Vec<String>, Option<Vec<u8>>>,
    max_retries: usize,
}

impl Tx<'_> {
    /// Override the maximum number of CAS retry attempts (default: 3).
    #[must_use]
    pub fn with_max_retries(mut self, n: usize) -> Self {
        self.max_retries = n;
        self
    }

    /// Retrieve the value stored at `path`.
    ///
    /// Staged mutations take precedence over the snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if a path component is invalid or the snapshot cannot be read.
    pub fn get(&self, path: &[&str]) -> Result<Option<Vec<u8>>, Error> {
        validate_path(path)?;
        let key = to_key(path);
        if let Some(val) = self.mutations.get(&key) {
            return Ok(val.clone());
        }
        let Some(commit_oid) = self.snapshot_commit else {
            return Ok(None);
        };
        let tree_oid = tree_of_commit(&self.store.git.repo, commit_oid)?;
        get_blob(&self.store.git.repo, tree_oid, &key)
    }

    /// Stage a write of `value` at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if a path component is invalid.
    pub fn put(&mut self, path: &[&str], value: Vec<u8>) -> Result<(), Error> {
        validate_path(path)?;
        self.mutations.insert(to_key(path), Some(value));
        Ok(())
    }

    /// Stage a deletion of the value at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if a path component is invalid.
    pub fn delete(&mut self, path: &[&str]) -> Result<(), Error> {
        validate_path(path)?;
        self.mutations.insert(to_key(path), None);
        Ok(())
    }

    /// List the immediate children of `path`.
    ///
    /// Combines snapshot tree entries with staged mutations: puts add keys, direct deletes
    /// remove them.  A put at a sub-path (depth > 1 below `path`) re-instates the parent key
    /// even if a direct delete was staged.
    ///
    /// # Errors
    ///
    /// Returns an error if a path component is invalid or the snapshot cannot be read.
    pub fn list(&self, path: &[&str]) -> Result<Vec<String>, Error> {
        validate_path(path)?;
        let prefix = to_key(path);
        let mut keys: BTreeSet<String> = BTreeSet::new();

        if let Some(commit_oid) = self.snapshot_commit {
            let tree_oid = tree_of_commit(&self.store.git.repo, commit_oid)?;
            if let Some(sub) = subtree(&self.store.git.repo, tree_oid, &prefix)? {
                list_entries(&self.store.git.repo, sub, &mut keys)?;
            }
        }

        // Collect direct deletes and adds from staged mutations.
        let mut direct_deletes: BTreeSet<String> = BTreeSet::new();
        let mut adds: BTreeSet<String> = BTreeSet::new();
        for (mut_path, value) in &self.mutations {
            if mut_path.len() > prefix.len() && mut_path.starts_with(&prefix) {
                let child = mut_path[prefix.len()].clone();
                match value {
                    Some(_) => {
                        adds.insert(child);
                    }
                    None if mut_path.len() == prefix.len() + 1 => {
                        direct_deletes.insert(child);
                    }
                    None => {}
                }
            }
        }
        // Adds win over direct deletes (a put at any sub-path revives the parent key).
        for key in direct_deletes {
            if !adds.contains(&key) {
                keys.remove(&key);
            }
        }
        for key in adds {
            keys.insert(key);
        }

        Ok(keys.into_iter().collect())
    }

    /// Commit all staged mutations to the store.
    ///
    /// Writes new tree objects (structural sharing via gix's tree editor), creates a commit,
    /// and CAS-updates the store ref.  Retries on conflict up to `max_retries` times.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Conflict`] if all retries are exhausted.
    pub fn commit(self) -> Result<(), Error> {
        let Tx {
            store,
            snapshot_commit: initial,
            mutations,
            max_retries,
        } = self;
        let mut snapshot = initial;
        let mut remaining = max_retries;

        loop {
            let snapshot_tree: gix::Tree<'_> = match snapshot {
                None => store.git.repo.empty_tree(),
                Some(c) => {
                    let tree_oid = tree_of_commit(&store.git.repo, c)?;
                    store
                        .git
                        .repo
                        .find_object(tree_oid)
                        .map_err(StoreError::FindObject)?
                        .into_tree()
                }
            };

            let new_tree_oid = apply_mutations(&snapshot_tree, &mutations)?;
            let new_commit_oid = write_store_commit(&store.git.repo, new_tree_oid, snapshot)?;

            let git_ref = store.git.git_ref(&store.ref_name)?;
            let mut tx = store.git.transaction();
            tx.stage(&git_ref, snapshot, Some(new_commit_oid));
            match tx.commit() {
                Ok(()) => return Ok(()),
                Err(_) if remaining > 0 => {
                    remaining -= 1;
                    snapshot = git_ref.read()?;
                }
                Err(_) => return Err(Error::Conflict(max_retries)),
            }
        }
    }
}

// ── Private helpers ──────────────────────────────────────────────────────────

fn validate_path(path: &[&str]) -> Result<(), Error> {
    if path.is_empty() {
        return Err(Error::EmptyPath);
    }
    for &component in path {
        if component.is_empty() || component.contains('/') || component.contains('\0') {
            return Err(Error::InvalidComponent(component.to_owned()));
        }
    }
    Ok(())
}

fn to_key(path: &[&str]) -> Vec<String> {
    path.iter().map(ToString::to_string).collect()
}

/// Return the root tree OID of a commit.
fn tree_of_commit(repo: &gix::Repository, commit_oid: ObjectId) -> Result<ObjectId, Error> {
    Ok(repo
        .find_object(commit_oid)
        .map_err(StoreError::FindObject)?
        .into_commit()
        .tree_id()?
        .detach())
}

/// Traverse `tree_oid` along `path` components, returning the subtree OID or `None`.
fn subtree(
    repo: &gix::Repository,
    tree_oid: ObjectId,
    path: &[String],
) -> Result<Option<ObjectId>, Error> {
    let mut current = tree_oid;
    for component in path {
        let obj = repo.find_object(current).map_err(StoreError::FindObject)?;
        let tree = obj.into_tree();
        let decoded = tree.decode()?;
        let Some(entry) = decoded
            .entries
            .iter()
            .find(|e| e.filename == component.as_bytes() && e.mode.is_tree())
        else {
            return Ok(None);
        };
        current = ObjectId::from(entry.oid);
    }
    Ok(Some(current))
}

/// Read the blob at `path` within `tree_oid`, returning `None` if absent.
fn get_blob(
    repo: &gix::Repository,
    tree_oid: ObjectId,
    path: &[String],
) -> Result<Option<Vec<u8>>, Error> {
    if path.is_empty() {
        return Ok(None);
    }
    let Some(sub) = subtree(repo, tree_oid, &path[..path.len() - 1])? else {
        return Ok(None);
    };
    let key = &path[path.len() - 1];

    let obj = repo.find_object(sub).map_err(StoreError::FindObject)?;
    let tree = obj.into_tree();
    let decoded = tree.decode()?;
    let Some(entry) = decoded
        .entries
        .iter()
        .find(|e| e.filename == key.as_bytes() && !e.mode.is_tree())
    else {
        return Ok(None);
    };
    let blob = repo
        .find_object(entry.oid)
        .map_err(StoreError::FindObject)?;
    Ok(Some(blob.data.clone()))
}

/// Populate `keys` with all entry names in `tree_oid`.
fn list_entries(
    repo: &gix::Repository,
    tree_oid: ObjectId,
    keys: &mut BTreeSet<String>,
) -> Result<(), Error> {
    let obj = repo.find_object(tree_oid).map_err(StoreError::FindObject)?;
    let tree = obj.into_tree();
    let decoded = tree.decode()?;
    for entry in &decoded.entries {
        let name = String::from_utf8_lossy(entry.filename).into_owned();
        keys.insert(name);
    }
    Ok(())
}

/// Apply `mutations` on top of `base_tree`, returning the new root tree OID.
///
/// Uses gix's tree editor for structural sharing: only modified subtree paths produce new
/// tree objects.
fn apply_mutations(
    base_tree: &gix::Tree<'_>,
    mutations: &HashMap<Vec<String>, Option<Vec<u8>>>,
) -> Result<ObjectId, Error> {
    let repo = base_tree.repo;
    let mut editor = base_tree
        .edit()
        .map_err(|e: gix::object::tree::editor::init::Error| Error::Tree(e.to_string()))?;

    for (path, value) in mutations {
        let real_path = path.join("/");
        match value {
            Some(bytes) => {
                let blob_oid = repo
                    .write_blob(bytes.as_slice())
                    .map_err(StoreError::WriteObject)?
                    .detach();
                editor
                    .upsert(
                        real_path.as_str(),
                        gix::object::tree::EntryKind::Blob,
                        blob_oid,
                    )
                    .map_err(|e: gix::objs::tree::editor::Error| Error::Tree(e.to_string()))?;
            }
            None => {
                editor
                    .remove(real_path.as_str())
                    .map_err(|e: gix::objs::tree::editor::Error| Error::Tree(e.to_string()))?;
            }
        }
    }

    editor
        .write()
        .map(|id: gix::Id<'_>| id.detach())
        .map_err(|e: gix::object::tree::editor::write::Error| Error::Tree(e.to_string()))
}

/// Write a commit object pointing to `tree_oid` with optional `parent`.
fn write_store_commit(
    repo: &gix::Repository,
    tree_oid: ObjectId,
    parent: Option<ObjectId>,
) -> Result<ObjectId, Error> {
    use gix::bstr::ByteSlice as _;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let time_str = format!("{secs} +0000");
    let sig = gix::actor::SignatureRef {
        name: b"git-store".as_bstr(),
        email: b"git-store@localhost".as_bstr(),
        time: &time_str,
    };
    let parents: Vec<ObjectId> = parent.into_iter().collect();
    repo.new_commit_as(sig, sig, "store transaction", tree_oid, parents)
        .map(|c| c.id)
        .map_err(|e| Error::Commit(e.to_string()))
}
