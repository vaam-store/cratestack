//! Guard #1 for cratestack#779's API floors: the offline half, mirroring
//! `cratestack-client-dart/src/package_floors_tests.rs`. Guard #2 (a real
//! `npm install` + typecheck at the exact floors) lives in CI, because
//! only the registry can say whether a version was actually published —
//! the class of defect #754 found when a hand-written `^0.8.8` floor
//! turned out to name a version that never existed.
//!
//! A unit-test module rather than `tests/`, matching this workspace's
//! existing `*_tests.rs` convention, so the constants stay `pub(crate)`
//! instead of being widened into the public API just to be asserted on.

use super::{CRATESTACK_ADAPTER_RTK_FLOOR, CRATESTACK_CBOR_FLOOR, CRATESTACK_REFINE_FLOOR};

/// Every floor this module guards, paired with the constant's name so a
/// failure names the thing to edit rather than just its value.
const FLOORS: [(&str, &str); 3] = [
    ("CRATESTACK_REFINE_FLOOR", CRATESTACK_REFINE_FLOOR),
    ("CRATESTACK_CBOR_FLOOR", CRATESTACK_CBOR_FLOOR),
    ("CRATESTACK_ADAPTER_RTK_FLOOR", CRATESTACK_ADAPTER_RTK_FLOOR),
];

/// The LOWER BOUND of any shape a floor is written in, as `(X, Y, Z)`: a
/// bare `0.8.15` (how the constants in [`super`] are now written), a caret
/// `^0.8.15`, or the two-sided range [`super::requirement`] composes once
/// the derived ceiling is attached.
///
/// Panics rather than returning an `Option`: every caller here is a test
/// whose failure message is more useful than a `None`.
///
/// Only the lower bound is ever compared in this module, and that is the
/// point rather than a limitation. Every assertion here asks whether the
/// floor names a release that exists and is not ahead of the workspace —
/// questions the ceiling cannot answer, because it is derived and names a
/// version that deliberately does not exist yet.
fn parse_caret(requirement: &str) -> (u64, u64, u64) {
    let first = requirement
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("empty version requirement {requirement:?}"));
    let digits = first
        .strip_prefix(">=")
        .or_else(|| first.strip_prefix('^'))
        .unwrap_or(first);
    let mut parts = digits.split('.');
    let mut next = |which: &str| -> u64 {
        parts
            .next()
            .unwrap_or_else(|| panic!("{requirement:?} has no {which} component"))
            .parse()
            .unwrap_or_else(|error| panic!("{requirement:?}'s {which} component: {error}"))
    };
    (next("major"), next("minor"), next("patch"))
}

/// A floor **above** the current workspace version is unresolvable for
/// exactly the reason #779 exists: `just bump` moves this crate's
/// `CARGO_PKG_VERSION` before the tag that publishes the npm packages,
/// so a floor ahead of the workspace names something the registry cannot
/// serve. That half is absolute.
///
/// The *equal* case is the subtle one, and this used to reject it
/// (cratestack#806). `<` rather than `<=` was a deliberate, conservative
/// proxy for two properties at once —
///
/// 1. the floor names a release that has already shipped, and
/// 2. the floor is not tracking the release version,
///
/// — the second being the property that *is* #779's fix, and the one a
/// well-meaning "keep it in sync with the bump" change would quietly
/// undo.
///
/// The proxy is right during the window that matters (on a bump PR the
/// workspace version genuinely is unpublished) and **wrong in exactly
/// one other window**: after a release tag publishes and before the next
/// bump, the workspace version *is* on the registry, and a floor naming
/// it is both resolvable and correct. #806 landed in that window — the
/// `Bytes` byte-string fix shipped in `0.8.15` while the workspace still
/// read `0.8.15` — so refusing the equal case would have forced a
/// correctness fix to wait for an unrelated version bump.
///
/// So the equal case is now allowed, and property (2) is preserved by
/// [`PUBLISHED_EQUAL_FLOORS`]: a floor may equal the workspace version
/// only if it is listed there with the reason. That list is the thing a
/// "keep it in sync with the bump" change would have to edit, which is
/// precisely the signal the strict `<` existed to produce. Adding an
/// entry is a deliberate act with a comment attached; drifting into one
/// is not possible.
///
/// Still deliberately not a claim that the floor was actually published:
/// npm is the only authority for that, and it is CI's
/// install-at-the-floor step (`just verify-typescript-floors`) that
/// checks it. This test is offline by design.
/// Empty as of the 0.9.0 bump, and that is the mechanism working rather
/// than an omission. `CRATESTACK_CBOR_FLOOR` was listed here while it
/// equalled the workspace version at `0.8.15` (cratestack#806 shipped the
/// `Bytes` byte-string fix inside that release window). At 0.9.0 the floor
/// is strictly below the workspace version again, so the ordinary `<` rule
/// covers it and `published_equal_floor_entries_are_still_needed` requires
/// the entry to be deleted — leaving it would silently widen the exemption
/// to whatever floor next lands on the current version.
const PUBLISHED_EQUAL_FLOORS: [(&str, &str); 0] = [];

#[test]
fn floors_never_exceed_the_workspace_version() {
    let current = parse_caret(&format!("^{}", env!("CARGO_PKG_VERSION")));
    for (name, floor) in FLOORS {
        let parsed = parse_caret(floor);
        if parsed == current {
            assert!(
                PUBLISHED_EQUAL_FLOORS
                    .iter()
                    .any(|(listed, _)| *listed == name),
                "{name} is {floor}, which equals the workspace version {current:?}. That is \
                 allowed only for a floor whose release has already published — add it to \
                 PUBLISHED_EQUAL_FLOORS with the evidence, or leave the floor below. Floors are \
                 API-compatibility constants; they must not follow `just bump`."
            );
            continue;
        }
        assert!(
            parsed < current,
            "{name} is {floor}, but this crate is at {current:?} — a floor ABOVE the current \
             version names something npm cannot serve under any circumstances, which is \
             cratestack#779 itself."
        );
    }
}

/// The list above is only a safety valve if it cannot rot into a blanket
/// exemption. An entry that no longer equals the workspace version has
/// outlived its purpose — the next `just bump` puts the floor genuinely
/// below the version, at which point the ordinary rule covers it and the
/// entry is just a hole waiting for a future floor to fall through.
#[test]
fn published_equal_floor_entries_are_still_needed() {
    let current = parse_caret(&format!("^{}", env!("CARGO_PKG_VERSION")));
    for (name, reason) in PUBLISHED_EQUAL_FLOORS {
        let floor = FLOORS
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .unwrap_or_else(|| panic!("PUBLISHED_EQUAL_FLOORS names {name}, which is not a floor"))
            .1;
        assert_eq!(
            parse_caret(floor),
            current,
            "PUBLISHED_EQUAL_FLOORS still lists {name} ({floor}), but it no longer equals the \
             workspace version {current:?} — the ordinary rule now covers it. Delete the entry; \
             leaving it in place widens the exemption for whatever floor lands on this version \
             next. Its recorded reason was: {reason}"
        );
    }
}
