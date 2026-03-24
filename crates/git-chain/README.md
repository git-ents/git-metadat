# git-chain

Append-only event chains stored as Git commit history.
Each commit is an event; the commit chain provides ordering.
Each commit's tree holds only that entry's payload — there is no accumulated state.

## Usage

### Append an event

```sh
git chain append refs/comments/blob/a1b2c3d -m "this needs a bounds check"
```

With a payload file:

```sh
git chain append refs/comments/blob/a1b2c3d -m "suggested fix" --payload suggestion.diff
```

With multiple payload files:

```sh
git chain append refs/comments/blob/a1b2c3d -m "context for the review" --payload suggestion.diff --payload trace.log
```

### Walk a chain

Walk from tip to root (most recent first):

```sh
git chain walk refs/comments/blob/a1b2c3d
```

### Threading

Append a reply to a specific event (second parent creates a thread):

```sh
git chain append refs/comments/blob/a1b2c3d -m "good catch, fixed" --parent <commit>
```

Walk only the commits in a specific thread:

```sh
git chain walk refs/comments/blob/a1b2c3d --thread <commit>
```

### Use a different repository

```sh
git chain -C /path/to/repo append refs/comments/blob/a1b2c3d -m "needs review"
```

## Design

A chain is a Git ref.
Each `append` creates a new commit whose first parent is the previous tip, advancing the ref.
The commit tree holds only that event's payload blobs — not a running snapshot of all payloads.

Threading works via the second parent: `--parent <commit>` records a reply relationship without forking the main chain.
`walk --thread` follows these links to reconstruct a conversation or sub-sequence.
