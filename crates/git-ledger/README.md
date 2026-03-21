# git-ledger

Git-native record storage.
Each record lives under its own ref with typed fields stored as blobs in a tree.

## Usage

```sh
git ledger create refs/issues --set title="Bug report" --set status=open
git ledger read refs/issues/1
git ledger update refs/issues/1 --set status=closed
git ledger list refs/issues
```

## License

MIT OR Apache-2.0
