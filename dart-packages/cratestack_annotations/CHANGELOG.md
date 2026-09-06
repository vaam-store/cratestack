# Changelog

## Unreleased

## 0.12.0 (2026-09-06)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.11.1 (2026-09-03)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.11.0 (2026-09-03)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.10.1 (2026-09-01)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.10.0 (2026-08-31)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.15 (2026-08-28)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.14 (2026-08-27)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.13 (2026-08-26)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.12 (2026-08-24)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.11 (2026-08-24)

No functional changes. The annotation surface (`listDefaults`, `touchFlagFields`,
`nonDefaultingListFields`) is unchanged from 0.8.10 — nothing under `lib/` was touched in this
range. Only `pubspec.yaml`'s version moved, since these packages are bumped in lockstep with the
workspace rather than independently.

The single commit reaching this package in `v0.8.10..v0.8.11` was cratestack#714, which wrote the
retroactive 0.8.10 entry above and registered both Dart CHANGELOGs in `.ci/changelog-files.sh`'s
declared list. That is changelog text, not library code — but the release seeder's no-op auto-fill
keys off *any* commit touching the package directory, and a commit editing the package's own
CHANGELOG.md counts, so this section was seeded as a placeholder instead of being filled in
automatically.

That placeholder is what 0.8.11 actually shipped: the release went out with the raw seed still in
this file, because `main` has no required status checks and the gate that caught it could not block
the merge. This entry corrects the repository and every archive from 0.8.12 onward — pub.dev's
published 0.8.11 page keeps the seed text, since an uploaded archive is immutable.

## 0.8.10 (2026-08-23)

First release carrying the annotation arguments the CrateStack Dart generator needs, and the first
release of these packages that the repo's changelog tooling tracks — see below.

- `touchFlagFields` — names the fields that have a Rust-synthesized `{field}IsSet` sibling, so the
  generated setter marks it touched too. Explicit rather than recovered from the
  `{field}`/`{field}IsSet` name shape: a schema may legally declare an unrelated `bool` field ending
  in `IsSet`, and a name heuristic made the other field's setter silently clobber it.
- `nonDefaultingListFields` — names list fields that must NOT default to `[]` or gain an
  `add{Field}` setter: to-many relations on a projection model, and the synthesized
  `{Model}FindMany.orderBy`. There, `null` means "not included in the response" and `[]` means
  "included and empty".
- Fixes `argument_type_not_assignable: bool? -> bool` for an optional non-nullable defaulted field —
  exactly the `{field}IsSet` touch-flag shape — which the generator produced for its own output.

0.8.8 and 0.8.9 were never published; 0.8.6 carried no changes to these packages. Prior to this
release these CHANGELOGs were not in `.ci/changelog-files.sh`'s declared list, so the release
tooling never seeded or dated them and they silently fell behind `pubspec.yaml` — which is what
`dart pub publish` was warning about with "CHANGELOG.md doesn't mention current version".

## 0.8.7 (2026-08-23)

### `CratestackBuilder` gained `touchFlagFields` and `nonDefaultingListFields`

Both are additive, defaulting to an empty `Set<String>`, so no existing `@CratestackBuilder(...)` call
site needs to change. `package:cratestack_builder` 0.8.7 reads them to replace a by-name heuristic that
collided with ordinary schema fields (`touchFlagFields`) and to stop defaulting an unset to-many relation
field on a generated model class to `[]` (`nonDefaultingListFields`) — see that package's own CHANGELOG
for the full rationale.

## 0.8.5

Initial release. Provides `@CratestackBuilder`, consumed by
`package:cratestack_builder`.
