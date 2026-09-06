## Unreleased

## 0.12.0 (2026-09-06)

- No functional changes to this package. Version kept in lockstep with the
  CrateStack workspace, which every published CrateStack artifact shares.
- The one workspace change that reaches this package's native side is inert
  here: `cratestack-client-flutter` — the frb-bridged crate this package's
  codec ships from — gained a `middleware` Cargo feature forwarding
  `cratestack-client-rust`'s pluggable HTTP transport (#926, #927). It is
  additive and off by default, adds nothing to the dependency graph unless
  enabled, and does not touch the CBOR bridge: the only diff in that crate is
  the feature declaration in its `Cargo.toml`, with no source change.

## 0.11.1 (2026-09-03)

- **`example/tool/verify_web_console.dart`'s headless-Chrome readiness check no longer flakes on a
  cold CI runner.** The DevTools-readiness deadline was a hardcoded 15s that a loaded runner could
  miss with zero diagnostics (Chrome's stderr was discarded, and nothing checked whether Chrome had
  already exited); this failed `just cbor-example-verify`'s web step three times in one day before
  clearing on a plain rerun. The deadline is now 60s by default (`--devtools-ready-timeout-seconds` /
  `CRATESTACK_CBOR_DEVTOOLS_READY_SECONDS`), a dead Chrome now fails immediately with its captured
  stderr and exit code instead of waiting out the deadline, and one automatic relaunch is attempted
  before giving up. Not a published-artifact change — this is example/CI tooling only, split into
  `example/tool/verify_web_console/*.dart` to stay under this repo's 200-line-per-file convention.
- **That same fix's first landing hung the job it was fixing** — reading `process.exitCode` to
  detect an already-exited Chrome opens a native exit-watch handle that keeps the Dart isolate alive
  until the process is truly reaped, and a bare `process.kill()` doesn't guarantee that. Every exit
  path now tears down deterministically (`ChromeProcess.shutDown`, escalating to SIGKILL) and
  finishes with an explicit `exit(code)` instead of trusting the event loop to drain on its own; a
  new `HardTimeoutWatchdog` (`--hard-timeout-seconds`, default 180s) is an in-process backstop, and
  `just cbor-example-verify` now also wraps the tool invocation in `timeout 300` as a second,
  OS-level line of defence.

## 0.11.0 (2026-09-03)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.10.1 (2026-09-01)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.10.0 (2026-08-31)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.9.4 (2026-08-30)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.9.3 (2026-08-30)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.9.2 (2026-08-30)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.9.1 (2026-08-29)

- **`lints` dev-dependency raised `^5.0.0` → `^6.0.0`**, matching every other package in this
  repo — this was the sole straggler. No lint violations surfaced in analyzable code. Stated
  precisely because it is weaker than a clean run: this package cannot be fully analyzed in a
  bare checkout, since it needs flutter_rust_bridge-generated glue that is not committed, so the
  analyzer's findings here are all missing-file errors rather than lint results.

- **Linux arm64 is blocked upstream in both halves, not just the Flutter
  one** (cratestack#823). The README's "Scope of this release", the library
  doc comment, this package's `UnsupportedError` message and
  `native_cbor_codec.dart`'s header all said that plain `dart test`/`dart
  run` "needs no Flutter bundling at all" and was therefore separately
  reachable on arm64 Linux. Measured, and it is not: this package declares
  `flutter.plugin.platforms`, which obliges `environment.flutter`, so a
  standalone Dart SDK fails with `Because cratestack_cbor requires the
  Flutter SDK, version solving failed` — for a pub.dev dependency, a
  `path:` dependency, and the package in place alike, and on x86_64 too.

  The standalone Dart SDK does ship arm64 Linux; that was the true half of
  the claim and it is not sufficient. Since Flutter publishes no arm64 Linux
  SDK on any channel, an arm64 user fails at `pub get` before
  `createCborCodec()` is called. Use `--no-native-cbor` for that target.
  Text only — no behaviour change.

- **The Linux arm64 `dart test`/`dart run` gap is now tracked on
  cratestack#823, not cratestack#563.** cratestack#563 — the ticket that built
  and published this package — was closed as completed on 2026-08-29, so the
  three places naming it as that gap's *open* home were pointing readers at a
  closed issue: `lib/cratestack_cbor.dart`'s library doc,
  `lib/src/native/native_cbor_codec.dart`'s header comment, and the
  `UnsupportedError` message a user actually sees on an unsupported host.

  Nothing about the gap itself changed. The Dart SDK does ship arm64 Linux, so
  the dev-mode `Isolate.resolvePackageUri` path is reachable there and throws;
  Flutter on arm64 Linux remains blocked upstream (no arm64 Linux SDK on any
  channel) rather than deferred. Text-only — no behaviour change, and every
  other cratestack#563 reference in this package is historical provenance and
  stays as it is.

## 0.8.15 (2026-08-28)

- **`createCborCodec()` is idempotent, and resolves its vendored library under
  `flutter test`** (cratestack#794). Three related fixes; the first is the one
  that turns a footgun into a non-issue.

  1. **A second `createCborCodec()` no longer throws `Bad state: Should not
     initialize flutter_rust_bridge twice`.** Any app that uses this package
     directly *and* has a generated `transport rpc` Dart client has two
     independent callers — its own code, and the generated client's RPC codec,
     which imports `package:cratestack_cbor` itself and cannot be handed an
     existing codec. Neither call site is wrong and neither can see the other.
     The returned `Future` is now memoized, so concurrent callers share one
     initialization instead of racing, and the `init` is guarded on
     flutter_rust_bridge's own state rather than on a flag private to this
     library, so a consumer that bootstrapped the bridge itself is respected
     too. Only a *successful* initialization is memoized: a failure is usually
     fixable in-process, and a memoized rejection would replay it forever.
     The web backend gets the same memoization — its second call was never
     fatal, but the race was identical.
  2. **New `isCborRuntimeInitialized`**, exported alongside `createCborCodec`
     on every platform. It reports the backend runtime's own state, so a
     consumer with its own bootstrap path can cooperate rather than guess.
  3. **`flutter test` can now resolve the vendored library with no
     `CRATESTACK_CBOR_NATIVE_LIB`.** `flutter_tester` does not implement
     `Isolate.resolvePackageUriSync`, so the dev-mode resolution strategy did
     not merely fail there, it threw `Unsupported operation`. Resolution now
     falls back to reading `.dart_tool/package_config.json` directly (walking
     up from the working directory, so nested directories and pub workspaces
     resolve too). This removes the reason consumers wrote a second bootstrap
     in the first place — the workaround for this gap is what created the
     double-init in (1). The package's own suite now runs under `flutter test`
     as well as `dart test`, in CI, with that env var explicitly unset.

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

- **`flutter_rust_bridge` moves from 2.12.0 to 2.13.0.** This is the pin that
  decides which Flutter apps can depend on this package at all: a bare version
  is an exact pin in pub's grammar, so an app on any other flutter_rust_bridge
  version cannot add `cratestack_cbor` — `pub get` fails during version
  solving (cratestack#716). The pin cannot be widened (see below), so moving it
  is the only lever there is.

  **This is a breaking change for anyone currently on 2.12.0**, and a fix for
  anyone on 2.13.0. If you are pinned to a 2.13.0 *prerelease* such as
  `2.13.0-beta.6`, you are still blocked and need to move to stable 2.13.0 —
  pub excludes prereleases from ranges, so there is no constraint we can write
  that admits both.

  Verified end to end rather than by editing version strings: glue regenerated
  with codegen 2.13.0, `cargo build --features frb-glue`, the Dart round-trip
  harness, and this package's own `dart test` (7 tests) all pass, with the
  cross-binding CBOR fixtures still matching byte for byte — the wire format
  is unchanged by the upgrade.

- **The pin is now documented as an install-blocking constraint**, in a README
  section placed ahead of the quickstart rather than left implicit in a
  dependency line. It explains why a range is not an option: a range resolves
  to the *newest* match while the shipped glue is fixed at one version, so it
  would work today and start handing consumers 2.14.0 against 2.13.0 glue the
  day upstream publishes it — breaking on upstream's release schedule rather
  than ours, with our CI still green. The README also documents the workaround
  an affected app has today (`cratestack generate-dart --no-native-cbor`, the
  pure-Dart codec, which has no flutter_rust_bridge dependency) and notes that
  web-only apps are constrained by the pin too, since pub has no conditional
  dependencies and the web backend imports no flutter_rust_bridge at all.

  **Correction:** the first draft of these docs claimed flutter_rust_bridge's
  codegen "rejects a ranged constraint outright". That was wrong and is
  retracted. The `bail!("unexpected version range")` it cited applies to
  `ffigen`, and reaches `flutter_rust_bridge` only through an `.is_ok()` in
  `auto_upgrade.rs` that discards it. Measured: `just cbor-vendor-glue` runs to
  completion with a ranged constraint in `pubspec.yaml`. Tooling does not block
  a range — the runtime version mismatch does, and that alone is sufficient.

- **The example app's round-trip marker no longer depends on a widget
  building.** The round trip hung off a `late final` field on the page's
  `State`, read only inside `build()`, making the marker every headless
  verification greps for a side effect of constructing the widget tree. It
  now starts in `main()`, and the widget is handed the already-running
  future. Scope, precisely: `runApp` schedules the root attach on a bare
  `Timer.run` and inflates the tree synchronously, so the old code ran one
  event-loop turn later — no frame or platform scene was ever required, and
  this is not a fix for cratestack#704. Example-only; no change to the
  published `cratestack_cbor` API or to either codec backend.

- **The example's `flutter test` now runs as part of `just cbor-example-verify`.**
  It previously ran nowhere (not in CI, and it failed on a clean checkout with
  `Unsupported operation: Isolate.resolvePackageUriSync` — `flutter test`'s
  test VM does not support the synchronous package-URI resolution the native
  backend's dev-mode fallback tries). The pre-existing
  `CRATESTACK_CBOR_NATIVE_LIB` override, checked before that resolution runs,
  now points the test at the vendored Linux blob directly. No change to the
  published API.

- **Linux arm64 is now documented as blocked upstream rather than as pending
  work.** Flutter publishes no arm64 Linux SDK on any channel (verified
  against the release manifest: 732 entries, all x64, zero containing `arm`
  or `aarch`), and a spike on a real `ubuntu-24.04-arm` runner confirmed
  `flutter build linux` therefore cannot run on such a host. No behaviour
  change — the platform already threw a clear `UnsupportedError`; the message
  and the docs now say *why*, and distinguish the Flutter case (impossible)
  from plain `dart test`/`dart run` on arm64 Linux (still open — the Dart
  SDK, unlike Flutter's, does ship for that host).

## 0.8.10 (2026-08-23)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.9 (2026-08-23)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.7 (2026-08-23)

- **Adds macOS, Windows and iOS.** The package previously supported Linux x64,
  web and Android; it now also ships prebuilt binaries for **macOS**
  (arm64 + x86_64, one universal xcframework), **Windows** x64, and **iOS**
  (device `ios-arm64` plus a universal simulator slice). As with every other
  platform here, these are vendored prebuilt artifacts: no Rust toolchain, no
  cargokit, and no network fetch at your build time. The same CBOR fixture
  round-trips byte-identically on all six targets, each verified by building
  and running a real Flutter app rather than by compiling alone.
- **Linux arm64 remains unsupported**, and is the only platform left in the
  matrix. Every other platform the package claims now has a real prebuilt
  binary and a real end-to-end test behind it.
- The macOS xcframework is shipped as a `.zip` inside the archive and unpacked
  by the plugin's CocoaPods `prepare_command` at pod-install time. This is
  invisible if you just depend on the package, and is required because
  `dart pub publish` dereferences symlinks: a macOS framework is a versioned
  bundle whose symlinks are structural, and without them `codesign` rejects it
  and `flutter build macos` fails. iOS frameworks are shallow bundles with no
  symlinks, so iOS ships unpacked.
- The archive grew accordingly — every consumer carries every platform's
  payload, which is the cost of one package covering the whole matrix.

## 0.8.6 (2026-08-21)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.3

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.
- First version published by the automated release pipeline rather than by
  hand (cratestack#563).

## 0.8.2

Package metadata only — the codec, the platform support and the vendored
artifacts are unchanged from 0.8.0.

- Declares `environment.flutter: ">=1.20.0"`. This package deliberately ships
  no `ios/` folder, and Flutter only permits a plugin to omit platform folders
  from 1.20 onward — pub.dev rejects the upload otherwise. The Dart constraint
  (`sdk: ^3.5.0`) remains the real floor.
- Removes the `publish_to: none` guard that kept earlier revisions from being
  published by accident.
- Shortens the package description to pub.dev's 180-character recommendation.

(0.8.1 was never published to pub.dev.)

## 0.8.0

- Initial package structure (cratestack#563). One uniform `CratestackCborCodec`
  API, auto-selected per platform:
  - Native: flutter_rust_bridge over a vendored prebuilt library. This
    release vendors **Linux x86_64 and Android (arm64-v8a, x86_64,
    armeabi-v7a)** — the remaining platform matrix (iOS, macOS, Windows,
    Linux arm64) is follow-up work.
  - Web: the existing `cratestack-cbor-wasm` wasm-bindgen artifact,
    vendored and loaded via `dart:js_interop`.
- Flutter app integration, proven by real builds (cratestack#563):
  - Linux: a Flutter FFI plugin (`linux/CMakeLists.txt`) bundles the
    vendored `.so` into a real `flutter build linux` app, instead of the
    `cargokit` build-Rust-from-source pattern most flutter_rust_bridge
    plugins use.
  - Android: a Flutter FFI plugin (`android/build.gradle`) packages the
    vendored per-ABI `.so` files into the APK via the standard `jniLibs`
    mechanism — no CMake/NDK invocation at consumer build time. Verified
    by a real `flutter build apk`, per-ABI presence assertion, and a
    real install-and-run on an Android emulator round-tripping CBOR.
  - Web: `pubspec.yaml`'s `flutter: assets:` vendors the `.js`/`.wasm`
    pair so a release `flutter build web` actually ships them; the web
    loader now tries both the dev-server and release asset URL
    conventions.
  - `example/`: a minimal Flutter app exercising the codec, verified with
    real `flutter build linux`/`flutter build web`/`flutter build apk`
    builds — see `just cbor-example-verify` and
    `just cbor-example-verify-android`.
