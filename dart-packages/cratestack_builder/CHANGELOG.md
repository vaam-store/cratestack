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

- **`cratestack_annotations` widened to `>=0.8.10 <0.10.0`** — a range, not a raise, and the
  distinction is load-bearing. `^0.8.10` forbade the 0.9.x annotations release a generated client
  now wants; `^0.9.1` would have had an empty intersection with the `^0.8.10` floor every existing
  generated client still declares. Only a range satisfies both, which is what allows this to
  publish before the client floors move.

- **`analyzer` widened to `>=12.0.0 <14.0.0`** — this package now supports both majors at once.
  The old `<13.0.0` ceiling was documented as protecting `riverpod_generator`, which no longer
  holds (4.0.6 moved to `^13.0.0`), so the ceiling had become the cause of the incompatibility it
  was written to prevent.

  It is a range rather than a flip to `>=13` because two CI gates resolve this package from
  opposite sources — `just verify-dart` from the working tree, the flutter example job from
  pub.dev at the published floor. While those sat on different majors they were mutually
  exclusive. A range satisfies both, so the templates and `CRATESTACK_BUILDER_FLOOR` can move to
  analyzer 13 in a later release against a published builder that already accepts it.

  **`sdk:` stays `^3.5.0`** — no narrowing of the published compatibility promise. On an older SDK
  pub resolves analyzer 12 from the range; analyzer 13 needs Dart `^3.9`/`^3.11` and is selected
  only where available.

- **`param.isInitializingFormal` → `param is FieldFormalParameterElement`.** Analyzer 13 deprecates
  the getter and this package analyzes with `--fatal-infos`, so the old form fails the build there.
  Semantically identical, and valid on both majors — verified with `dart analyze --fatal-infos`
  plus a full `dart test` run against coherently-resolved analyzer 12.1.0 and 13.3.0 graphs.

## 0.8.15 (2026-08-28)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.14 (2026-08-27)

### The `cratestack_annotations` floor names a version that exists

This generator reads `touchFlagFields` and `nonDefaultingListFields` off `@CratestackBuilder(...)`
via `ConstantReader.read(...)`, which throws at *generation* time — not at `pub get` — when the
resolved `cratestack_annotations` has no such field. The declared constraint therefore has to name
the earliest release that really carries those arguments, and it did not.

This package's `pubspec.yaml` declared `cratestack_annotations: ^0.8.8`, justified in a comment as
"0.8.7 is the first release with the `touchFlagFields`/`nonDefaultingListFields` arguments this
generator reads". Checked against pub.dev's API and the published archives rather than against the
changelog, both halves were wrong: **0.8.8 was never published** (0.8.8 and 0.8.9 were skipped, so
versions run 0.8.7 → 0.8.10) and **0.8.7 contains neither identifier**. 0.8.10 is the first release
that does.

It was harmless only by accident — a caret constraint resolves upward, so `^0.8.8` landed on 0.8.10
anyway. The floor had rotted before anything could depend on it. It now reads `^0.8.10`, and the
generated-client floors it sits alongside are backed by tests that read this pubspec, so raising one
without the other fails rather than drifting quietly (cratestack#754).

The same wrong justification appears in this file's own 0.8.11 entry below, which describes `^0.8.8`
as naming "the earliest annotation surface this generator uses". That sentence is corrected in place
there rather than rewritten, so the claim and its refutation stay visible together.

Raise this floor **only** when this builder starts reading a newly-added annotation field, never as
part of a routine version bump: the package version moves in lockstep with the CrateStack workspace,
and the floor deliberately does not. See `docs/tooling/dart-publishing.md`.

Nothing under `lib/` changed in this range; generator behaviour is identical to 0.8.13.

### A correction to this file

0.8.14 was tagged and published with the raw, unedited seed placeholder still in this section — the
`<!-- TODO -->` marker, the "Do not commit with this placeholder text" line, and a dump of every
workspace commit in the range, most of which never touched this package. That is the same failure
0.8.11's entry below records, recurring for the same reason: `main` has no required status checks,
so the `changelog (no unedited seeds)` gate reported the problem without being able to block the
merge.

This entry replaces that placeholder. The repository and every archive from here on carry the real
text; pub.dev's published 0.8.14 page keeps the seed, since an uploaded archive is immutable.

## 0.8.13 (2026-08-26)

### A `{field}IsSet` touch flag no longer gets a fluent setter of its own

Found by measuring the generated builders against the inline ones this package replaces, rather
than by reading the code. A patch input's touch flag (`noteIsSet` beside `note`) is an ordinary
constructor parameter as far as the analyzer is concerned, so it was getting a setter like any other
field — which let a caller write

```dart
UpdateGadgetInputBuilder().note('x').noteIsSet(false).build()
```

and produce a patch claiming the field is untouched while carrying a value. Order-dependent, and
unrepresentable in the inline builder this replaces, which kept its tracking bool private.

The flag is derived state: the owning field's setter marks it and `build()` defaults it to `false`.
Both still happen; only the independent setter is gone.

Suppression is computed from `touchFlagFields` — naming `note` already implies `noteIsSet` — so this
needs no additional annotation argument.

## 0.8.12 (2026-08-24)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.11 (2026-08-24)

`pubspec.yaml` no longer carries a `dependency_overrides` block pointing `cratestack_annotations` at
the sibling directory (cratestack#714). It was a bootstrap affordance from the phase where the
`touchFlagFields`/`nonDefaultingListFields` arguments existed only in unpublished source;
`cratestack_annotations` 0.8.10 is live on pub.dev, so this package now resolves it from the registry
like any other consumer. That was confirmed at the time by checking the *resolved* path
(`.pub-cache/hosted/pub.dev/cratestack_annotations-0.8.10`) rather than by reading the manifest — a
stale override is invisible from the manifest alone.

The declared constraint is unchanged at `^0.8.8`, deliberately: it names the earliest annotation
surface this generator uses, not the current version, and caret on a `0.x` version already pins the
second component, so `^0.8.8` resolves 0.8.10.

> **Corrected in 0.8.14.** The middle clause above is wrong: `^0.8.8` did *not* name the earliest
> annotation surface this generator uses. 0.8.8 was never published, and published 0.8.7 carries
> neither `touchFlagFields` nor `nonDefaultingListFields`. 0.8.10 is the earliest release that does,
> and the constraint says so from 0.8.14 on. The last clause stands: caret resolution upward is why
> the wrong floor was harmless.

No generator behaviour changed — nothing under `lib/` was touched in this range.

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

### Fix: a field's setter silently failed to mark its `{field}IsSet` touch flag touched

Any `@CratestackBuilder()`-annotated class carrying `cratestack-client-dart`'s `{field}`/`{field}IsSet`
touch-flag pair (issue #663 — a nullable Patch field needs a way to distinguish "untouched" from
"explicitly cleared to null" on the wire) generated a `{field}` setter that updated only `{field}`'s own
backing field, leaving `{field}IsSet` at its default. `build()` therefore computed `{field}IsSet: false`
even when the caller had explicitly called `.{field}(null)`, indistinguishable from never having touched
the field at all — silently reintroducing the exact bug issue #663 fixed, for every generated
`Update{Model}Input`'s nullable field, the moment issue #668 phase 2 moved builder generation out of
`cratestack-client-dart`. Caught by `crates/cratestack-client-dart/tests/fixtures/
builder_edge_cases_patch_test.dart`'s `an explicitly-cleared nullable field serializes as an explicit
null` under `just verify-dart` — a real, running package + `build_runner` + `flutter test` sequence, not
a text-level assertion.

Fixed by recovering the `{field}`/`{field}IsSet` link structurally (a `bool`-typed field named exactly
`{other}IsSet` is treated as `{other}`'s touch flag) and having `{field}`'s own setter also mark the
linked flag touched, mirroring the pre-#668 inline template's behavior.

**Superseded within this same release** — see "the structural touch-flag heuristic collided with
ordinary fields" below: the by-name heuristic this fix introduced was itself wrong and has been replaced
with an explicit `touchFlagFields` annotation argument.

### Fix: an optional non-nullable field with a default value crashed `build()`

Any `@CratestackBuilder()`-annotated class whose constructor has an optional (non-`required`), non-list,
non-nullable named parameter with a default value — the shape `cratestack-client-dart`'s own
`Update{Model}Input.{field}IsSet` touch flag uses (issue #663) — produced a `build()` that passed the
nullable backing field straight through, a real `argument_type_not_assignable` compile error whenever
the field was never explicitly set via the builder. Every generated `Update{Model}Input` with at least
one nullable field hit this the moment issue #668 phase 2 started annotating patch classes.

Fixed by falling back to the parameter's own recovered default (`FormalParameterElement.defaultValueCode`)
instead of the raw backing field, e.g. `noteIsSet: _noteIsSet ?? false`.

### Fix: the structural touch-flag heuristic collided with ordinary fields

The by-name heuristic added above — any `bool`-typed field whose identifier ends in `IsSet` is treated
as some other field's touch flag — fires on a schema that legitimately declares a standalone field shaped
that way. `cratestack-parser`'s `tests_patch_touch_flag_collisions.rs` deliberately accepts a
non-nullable `weight` beside an unrelated `weightIsSet` field (`weight` is non-nullable, so Rust
synthesizes no touch flag for it at all); the heuristic linked them anyway, so `.weight(5)` silently
overwrote whatever the caller had explicitly set via `.weightIsSet(false)`, order-dependently.

Fixed by replacing the heuristic with an explicit `touchFlagFields: Set<String>` argument on
`@CratestackBuilder(...)`, naming exactly the fields Rust actually synthesized a touch flag for. The
by-name recovery is gone — a hand-written class that wants the same linkage now states it explicitly.

### Fix: a to-many relation field on a model class defaulted to `[]` instead of staying `null`

`package:cratestack_builder` derives list-ness purely from `DartType.isDartCoreList`, which cannot
distinguish a scalar list field from a to-many relation field on a generated model class — the two are
structurally identical Dart (`final List<Post>? posts;`). Every list field on a non-patch class
(`listDefaults: true`) therefore defaulted an unset value to `[]` and gained an `add{Field}` setter,
including relation fields — conflating "this relation was not included in the response" with "included
and empty" (the exact cross-language divergence issue #661 exists to prevent), since Rust's own model
builder has no counterpart for a relation field at all (`scalar_model_fields` drops them).

Fixed by adding a `nonDefaultingListFields: Set<String>` argument on `@CratestackBuilder(...)`: field
identifiers to treat as non-list for builder purposes (no `add{Field}` setter, no `?? []` default) even
though `listDefaults` is `true` for the class as a whole.

## 0.8.5

Initial release. Generates a fluent `{Class}Builder` into a
`part '<file>.builder.dart'` for every class annotated with
`@CratestackBuilder` from `package:cratestack_annotations`.
