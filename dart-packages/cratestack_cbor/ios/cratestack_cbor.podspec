# cratestack_cbor's iOS FFI-plugin build file (cratestack#563).
#
# Deliberately NOT the cargokit shape most flutter_rust_bridge plugins use
# (build Rust from source at consumer build time via an ExternalProject /
# CMake-invokes-cargo step) — the maintainer decision recorded on the ticket
# rejected imposing a Rust toolchain on every consuming Flutter developer's
# machine and CI, same as `linux/CMakeLists.txt`, `windows/CMakeLists.txt`,
# `android/build.gradle`, and `macos/cratestack_cbor.podspec`. This podspec
# has no Classes/ sources and does not invoke cargo; its only job is to hand
# the ALREADY-VENDORED prebuilt xcframework
# (`Frameworks/CratestackCborNative.xcframework`, produced by `just
# cbor-vendor-ios` — see this package's README) to CocoaPods' own
# `vendored_frameworks` mechanism — the same mechanism
# `macos/cratestack_cbor.podspec` uses, modeled closely on it.
#
# UNPACKED, NOT ZIPPED — the one deliberate difference from the macOS
# podspec, and worth stating plainly since it looks like an omission
# otherwise. macOS ships `Frameworks/CratestackCborNative.xcframework.zip`
# and unpacks it via `prepare_command` because `dart pub publish`
# dereferences symlinks and a macOS framework (a VERSIONED bundle —
# `Versions/A/...` plus three symlinks) loses those symlinks in the archive,
# which fails `codesign` outright — see that podspec's header comment for
# the measurements. iOS frameworks are FLAT ("shallow") bundles — no
# `Versions/` indirection, no symlinks anywhere in the layout `just
# cbor-vendor-ios` constructs — so there is nothing for `dart pub publish`
# to dereference, and the zip + `prepare_command` apparatus buys nothing
# here. `just cbor-vendor-ios` asserts this at build time (0 symlinks or the
# recipe fails loudly) rather than merely assuming it — see that recipe's
# own header comment for the full reasoning and what to do if that
# assertion ever fires. Correspondingly there is no `ios/Frameworks/
# *.xcframework/` entry in this package's `.pubignore`, unlike the macOS
# one: the directory itself is exactly what ships.
#
# CocoaPods, NOT Swift Package Manager — same reasoning as
# `macos/cratestack_cbor.podspec`'s header comment (verified there on a real
# `macos-latest` runner for the analogous macOS case; not independently
# re-verified for iOS on real hardware, since this repo's dev toolchain has
# no Xcode — `cratestack-cbor-ios` in `.github/workflows/ci.yml` is this
# podspec's first real execution, same status the macOS/Windows slices had
# before their own first CI runs).
#
# The FRAMEWORK NAME is `CratestackCborNative`, deliberately different from
# this podspec's own `s.name` (`cratestack_cbor`) — same collision reasoning
# as macOS: under `use_frameworks!` (Flutter's default for iOS), CocoaPods
# generates a `<pod_name>.framework` for every pod, so a *vendored*
# framework sharing that exact name would collide with the one CocoaPods
# itself is trying to produce for this pod.
#
# `vendored_frameworks` points at `Frameworks/CratestackCborNative
# .xcframework` — INSIDE this podspec's own directory, not
# `../blobs/ios/...` — same reasoning as the macOS podspec: CocoaPods
# resolves `vendored_frameworks` relative to the podspec root and does not
# reliably accept `..` escapes out of it, so `just cbor-vendor-ios`
# deliberately assembles its output directly under `ios/Frameworks/`, not
# `blobs/ios/`. `ios/Frameworks/` is gitignored the same way `blobs/` and
# `macos/Frameworks/` are — see this package's `.gitignore` — this is still
# build output, just not placed under `blobs/`.
#
# See `../lib/src/native/native_cbor_codec.dart`'s iOS branch for the
# Dart-side half of this contract: it resolves the vendored library with the
# SAME fixed relative string macOS uses (`CratestackCborNative.framework/
# CratestackCborNative`) — no path computation at all. That only works
# because CocoaPods LINKS this vendored framework into the built app (not
# merely copies it), so dyld has already loaded the image by the time
# `DynamicLibrary.open` runs and matches it by path suffix — same mechanism
# macOS uses, see that file's doc comment for the full explanation.
Pod::Spec.new do |s|
  s.name             = 'cratestack_cbor'
  s.version          = '0.12.0'
  s.summary          = 'Native CBOR codec for CrateStack Dart/Flutter clients (iOS).'
  s.description      = <<-DESC
Vendors a prebuilt xcframework (device arm64 + universal simulator
arm64/x86_64) wrapping crates/cratestack-client-flutter's
flutter_rust_bridge cbor module. No Rust toolchain, no network fetch, at
consumer build time — see this package's README and
docs/tooling/cratestack-cbor-development.md.
                       DESC
  s.homepage         = 'https://cratestack.dev'
  s.license          = { :file => '../LICENSE' }
  s.author           = { 'CrateStack' => 'https://cratestack.dev' }
  s.source           = { :path => '.' }

  # No Classes/ — this pod has no source of its own to compile. See the
  # file header above for why `vendored_frameworks` points inside this
  # directory rather than at `../blobs/ios/`.
  s.vendored_frameworks = 'Frameworks/CratestackCborNative.xcframework'

  s.dependency 'Flutter'

  # 13.0 matches Flutter's own `plugin_ffi` iOS template
  # (`s.platform = :ios, '13.0'` in the Flutter SDK's
  # `templates/plugin_ffi/ios.tmpl/projectName.podspec.tmpl`) and this
  # repo's own `flutter create --platforms=ios` output
  # (`IPHONEOS_DEPLOYMENT_TARGET = 13.0` in the generated example Runner
  # project) — checked against this repo's pinned Flutter 3.44.1, not
  # guessed. Kept in lockstep with the xcframework's own Info.plist
  # `MinimumOSVersion` (see `just cbor-vendor-ios`), the same way the macOS
  # podspec's `10.15` is kept in lockstep with its Info.plist
  # `LSMinimumSystemVersion`.
  s.platform = :ios, '13.0'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
end
