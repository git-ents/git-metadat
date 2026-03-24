# git-ledger

Git-native record storage.
Each record lives under its own ref with typed fields stored as blobs in a tree.

## Usage

### Create a record

```sh
git ledger create refs/issues --set title="Bug report" --set status=open
```

With an explicit ID:

```sh
git ledger create refs/issues my-id --set title="Named record"
```

With a content-addressed ID (hashes stdin):

```sh
git ledger create refs/issues --content-hash --set title="Deduplicated record"
```

With a custom commit message:

```sh
git ledger create refs/issues --set title="Bug" --set status=open -m "open issue: bug"
```

### Read a record

```sh
git ledger read refs/issues/1
```

### Update a record

Set a field:

```sh
git ledger update refs/issues/1 --set status=closed
```

Set multiple fields at once:

```sh
git ledger update refs/issues/1 --set status=closed --set resolution=fixed
```

Delete a field:

```sh
git ledger update refs/issues/1 --delete resolution
```

Mix sets and deletes:

```sh
git ledger update refs/issues/1 --set status=wontfix --delete assignee
```

### List records

```sh
git ledger list refs/issues
```

### Show history for a record

```sh
git ledger log refs/issues/1
```

### Use a different repository

```sh
git ledger -C /path/to/repo list refs/issues
```

## Design

A ledger record is a Git ref (e.g. `refs/issues/1`).
Each field is a blob stored in the commit's tree under the field name.
`create` and `update` both advance the ref with a new commit, so the full field history is preserved in the commit log.

IDs are assigned sequentially by default (1, 2, 3, …).
Pass an explicit ID or `--content-hash` to override.

## License

MIT OR Apache-2.0
