///! Testingg
use git2;
use git2::{Error, Repository};

#[derive(Debug)]
pub struct MetadataOptions {
    pub name: String,
    pub shard_level: u8,
    pub force: bool,
}

impl Default for MetadataOptions {
    fn default() -> Self {
        Self {
            name: "refs/metadata/commits".to_string(),
            shard_level: 4,
            force: false,
        }
    }
}

pub trait MetadataIndex<'a> {
    fn add_metadata(
        &'a self,
        oid: &git2::Oid,
        metadata: &git2::Tree,
        maybe_commit: Option<&git2::Commit>,
        maybe_opts: Option<&MetadataOptions>,
    ) -> Result<git2::Reference<'a>, Error>;
}

impl<'a> MetadataIndex<'a> for Repository {
    fn add_metadata(
        &'a self,
        oid: &git2::Oid,
        metadata: &git2::Tree,
        maybe_commit: Option<&git2::Commit>,
        maybe_opts: Option<&MetadataOptions>,
    ) -> Result<git2::Reference<'a>, Error> {
        let commit = maybe_commit.unwrap_or(&self.head()?.peel_to_commit()?);

        let default_opts = MetadataOptions::default();
        let opts = maybe_opts.unwrap_or(&default_opts);

        let reference = match self.find_reference(&opts.name) {
            Ok(reference) => Some(reference),
            Err(e) if e.code() == git2::ErrorCode::NotFound => None,
            Err(e) => return Err(e),
        };

        todo!()
    }
}
