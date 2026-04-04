//! Plumbing commands for tracking non-text objects.

/// Plumbing functionality for non-text object storage.
pub struct Store {
    repo: git2::Repository,
}

impl Store {
    /// Returns a reference to the underlying repository.
    #[must_use]
    pub fn repo(&self) -> &git2::Repository {
        &self.repo
    }
}
