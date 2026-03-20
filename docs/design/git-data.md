+++
title = "git-data"
subtitle = "Design Specification"
version = "0.1.0"
date = 2026-03-19
status = "Draft"
+++

# git-data

## Overview

git-data is a workspace of three crates that provide structured data primitives over Git refs.

Git has refs, commits, trees, and blobs.
It has no built-in concept of structured annotations on objects, versioned records outside of branches, or bidirectional relationships between refs.
These three patterns recur in any system that uses Git as a data store.

git-data provides them as independent, composable libraries.


## Crates

### git-metadata

Structured annotations on existing Git objects.

A metadata entry is keyed by the OID of the object it annotates.
The annotated object exists independently; the metadata describes it.
This extends Git's notes (which map OIDs to blobs) to map OIDs to trees, allowing multiple tools to attach named entries under the same OID without conflict.

**Ref structure:**

```text
refs/metadata/<namespace> → commit → tree
  <oid-prefix>/
    <oid-suffix>/
      <entry-name>          # blob: arbitrary content
```

The two-level fanout by OID prevents pathological tree sizes, matching the pattern Git uses internally for loose objects.

**Operations:**

- `attach(oid, path, content)` — write a blob at `<oid>/<path>` in the metadata tree.
- `read(oid, path)` — read a single entry.
- `read_all(oid)` — list all entries for an object.
- `remove(oid, path)` — delete an entry.

Every write is a new commit on the metadata ref.
The commit history is the audit log of all annotation changes.

**Concurrency:**

Two writers annotating different OIDs touch disjoint tree paths.
A three-way tree merge resolves these automatically.
Two writers annotating the same OID at different paths also merge cleanly.
Conflict occurs only when two writers modify the same entry on the same OID simultaneously — the correct resolution is rejection and retry.


### git-ledger

Versioned records stored as refs.

A ledger entry is a standalone ref with its own lifecycle.
It is not metadata on any object — it is an independent record with a sequential ID, commit history as an audit log, and tree-structured state.

**Ref structure:**

```text
refs/<namespace>/<id> → commit → tree
  <field>             # blob: field value
  <field>
  <subdir>/
    <field>
```

Each record is its own ref.
Two writers modifying different records never conflict.

**ID assignment:**

Sequential IDs are assigned by scanning `refs/<namespace>/` to find the highest existing ID and incrementing.
The ref creation itself is the compare-and-swap: if another writer created the same ID, the push fails and the creator rescans and retries.

No counter ref is required.
The source of truth for "what IDs exist" is the refs themselves.

At large scale (thousands of records), scanning all refs to find the max becomes expensive.
An optional counter ref can serve as an acceleration structure, same as any other derived index — a performance optimization, not a correctness requirement.
If the counter is lost or stale, a rescan rebuilds it.

**Operations:**

- `create(namespace, fields)` — scan for the next available ID, create a new ref with an initial commit containing the given tree.
  Retry on conflict.
- `read(namespace, id)` — read the current tree at a record's ref.
- `update(namespace, id, mutations)` — commit a new tree to the record's ref.
  The previous state is preserved in history.
- `list(namespace)` — prefix scan over `refs/<namespace>/` to enumerate records.
- `history(namespace, id)` — walk the commit chain on a record's ref.

**Namespace scoping:**

Namespaces partition records into independent groups, each with its own ref subtree:

```text
refs/<namespace>/<scope>/<id>
```

Scopes are fully independent: no cross-scope contention on ID assignment or record writes.


### git-links

Bidirectional relationships between refs.

A link connects two keys.
It does not belong to either of them.
Both directions are written in a single commit to a single ref, guaranteeing consistency without multi-ref atomicity.

**Ref structure:**

```text
refs/<namespace> → commit → tree
  <key-a>/
    <key-b>             # blob: empty or optional metadata
  <key-b>/
    <key-a>             # blob: empty or optional metadata
```

Keys are opaque path segments.
The library does not interpret them.
Consumers assign meaning.

When metadata is absent, the tree entry points to the empty blob (`e69de29...`).
Every metadata-free link shares this single object.

**Operations:**

- `link(a, b, metadata?)` — write both directions in one commit.
- `unlink(a, b)` — remove both directions in one commit.
- `linked(key)` — list all keys linked to this key (single tree read).
- `is_linked(a, b)` — check existence (single tree entry lookup).

**Concurrency:**

Two writers linking disjoint key pairs touch disjoint tree paths.
A three-way tree merge resolves these automatically.
Conflict occurs only when two writers modify the same link simultaneously.

**Ref ownership:**

The `namespace` is caller-provided.
The library owns no ref namespace.
A consumer passes `"refs/links"` or `"refs/my-tool/links"` — the library does not care.

**Example: forge issue linking.**

Forge uses `git-links` with `refs/forge/links` as the namespace.
Keys are type/ID strings that forge constructs; the library stores them verbatim.

Linking issue 42 to review 7 and commit `abc123`:

```rust
let links = LinkStore::new(&repo, "refs/forge/links");

links.link("issue/42", "review/7", None, &sig)?;
links.link("issue/42", "commit/abc123", None, &sig)?;
```

This produces:

```text
refs/forge/links → commit → tree
  issue/42/
    review/7                # empty blob
    commit/abc123           # empty blob
  review/7/
    issue/42                # empty blob
  commit/abc123/
    issue/42                # empty blob
```

Querying everything linked to issue 42:

```rust
let related = links.linked("issue/42")?;
// → ["review/7", "commit/abc123"]
```

Querying the reverse — all issues referencing commit `abc123`:

```rust
let related = links.linked("commit/abc123")?;
// → ["issue/42"]
```

Both directions are tree reads.
Forge parses the key strings to recover type and ID.
The library never does.


## Layering

```text
git (objects, refs, transport)
├── git-metadata    (annotations on objects)
├── git-ledger      (versioned records as refs)
└── git-links       (bidirectional relationships)
```

The three crates are independent.
None depends on another.
A consumer may use any combination.

The shared machinery — ref → commit → tree reads and writes, tree merging, commit signing — is either inlined or extracted to a shared internal crate if duplication warrants it.
This is a code organization decision, not an architectural one.


## What git-data Is Not

git-data is not a framework.
It imposes no schema, no workflow, no naming convention beyond ref structure.

git-data does not run hooks or enforce policy.
Consumers (forge, kiln, other tools) own domain logic.

git-data does not handle transport.
Push, fetch, and ref advertisement filtering are the consumer's responsibility.

git-data does not handle merge strategy selection.
It provides the primitives (tree reads, tree writes, atomic commits) that make auto-merge possible.
The consumer decides when and how to merge.


## Workspace Layout

```text
git-data/
├── Cargo.toml              # workspace root
├── crates/
│   ├── git-metadata/
│   │   ├── Cargo.toml
│   │   └── src/
│   ├── git-ledger/
│   │   ├── Cargo.toml
│   │   └── src/
│   └── git-links/
│       ├── Cargo.toml
│       └── src/
```

Each crate publishes independently to crates.io.
The workspace shares test infrastructure, CI, and release tooling.


## CLI

git-metadata ships a CLI as `git-metadata` (invoked as `git metadata`).
It is the only crate with a CLI at this time.

git-ledger and git-links are library-only.
They may gain CLIs if direct human use outside of a consumer tool proves valuable.
This is unlikely — the operations are meaningful only in the context of a specific schema, which the consumer defines.
