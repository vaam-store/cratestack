# cratestack_cbor's macOS FFI-plugin build file (cratestack#563).
#
# Deliberately NOT the cargokit shape most flutter_rust_bridge plugins use
# (build Rust from source at consumer build time via an ExternalProject /
# CMake-invokes-cargo step) — the maintainer decision recorded on the ticket
# rejected imposing a Rust toolchain on every consuming Flutter developer's
# machine and CI, same as `linux/CMakeLists.txt`, `windows/CMakeLists.txt`,
# and `android/build.gradle`. This podspec has no Classes/ sources and does
# not invoke cargo; its only job is to hand the ALREADY-VENDORED prebuilt
# universal xcframework (`Frameworks/CratestackCborNative.xcframework`,
# produced by `just cbor-vendor-macos` — see this package's README) to
# CocoaPods' own `vendored_frameworks` mechanism.
#
# CocoaPods, NOT Swift Package Manager — verified on a real `macos-latest`
# runner (`spike/cbor-macos-xcframework`,
# .github/workflows/spike-cbor-macos.yml, cratestack#563): `flutter create
# --platforms=macos` on a plugin with no prior `macos/` produces
# `macos/Podfile` and no `Package.swift` anywhere. This is a podspec, not a
# `Package.swift` — do not add one; Flutter's own tooling generates the
# consuming app's Podfile, this file only has to satisfy CocoaPods' half of
# that contract. (Flutter 3.24+ defaults NEW plugin/app scaffolding to
# Swift Package Manager, but that default only applies to plugins that
# themselves ship SPM support — a plugin with only a podspec, like this
# one, is integrated via CocoaPods in a "mixed" SPM+CocoaPods build. On a
# host with no Xcode installed (this repo's own Linux dev machine),
# Flutter's `setupPodfile` step no-ops entirely — `_xcodeProjectInterpreter
# .isInstalled` gates it in `package:flutter_tools/src/macos/cocoapods
# .dart` — which is why a real Podfile could not be produced or verified
# here; see `example/README.md`/this PR's own notes for what that means for
# `example/macos/`.)
#
# The FRAMEWORK NAME is `CratestackCborNative`, deliberately different from
# this podspec's own `s.name` (`cratestack_cbor`) — verified on the spike:
# under `use_frameworks!` (Flutter's default for macOS), CocoaPods
# generates a `<pod_name>.framework` for every pod, so a *vendored*
# framework sharing that exact name would collide with the one CocoaPods
# itself is trying to produce for this pod.
#
# `vendored_frameworks` points at `Frameworks/CratestackCborNative
# .xcframework` — INSIDE this podspec's own directory, not
# `../blobs/macos/...` the way `linux/CMakeLists.txt`/`windows/CMakeLists
# .txt` reach into `../blobs/<platform>/`. CocoaPods resolves
# `vendored_frameworks` relative to the podspec root and does not reliably
# accept `..` escapes out of it (verified on the spike branch) — so
# `just cbor-vendor-macos` deliberately assembles its output directly under
# `macos/Frameworks/`, not `blobs/macos/` (see that recipe's own comment).
# `macos/Frameworks/` is gitignored the same way `blobs/` is — see this
# package's `.gitignore` — this is still build output, just not placed
# under `blobs/`.
#
# See `../lib/src/native/native_cbor_codec.dart`'s macOS branch for the
# Dart-side half of this contract: it resolves the vendored library with a
# FIXED relative string (`CratestackCborNative.framework/
# CratestackCborNative`), no path computation at all — a third mechanism,
# different again from Linux/Windows' executable-relative path computation
# and Android's bare-SONAME `dlopen`. That only works because CocoaPods
# LINKS this vendored framework into the built app (not merely copies it),
# so dyld has already loaded the image by the time `DynamicLibrary.open`
# runs and matches it by path suffix — see that file's doc comment for the
# full explanation (verified via `otool -L` on the spike's built app
# binary).
Pod::Spec.new do |s|
  s.name             = 'cratestack_cbor'
  s.version          = '0.12.0'
  s.summary          = 'Native CBOR codec for CrateStack Dart/Flutter clients (macOS).'
  s.description      = <<-DESC
Vendors a prebuilt universal (arm64 + x86_64) xcframework wrapping
crates/cratestack-client-flutter's flutter_rust_bridge cbor module. No Rust
toolchain, no network fetch, at consumer build time — see this package's
README and docs/tooling/cratestack-cbor-development.md.
                       DESC
  s.homepage         = 'https://cratestack.dev'
  s.license          = { :file => '../LICENSE' }
  s.author           = { 'CrateStack' => 'https://cratestack.dev' }
  s.source           = { :path => '.' }

  # No Classes/ — this pod has no source of its own to compile. See the
  # file header above for why `vendored_frameworks` points inside this
  # directory rather than at `../blobs/macos/`.
  s.vendored_frameworks = 'Frameworks/CratestackCborNative.xcframework'

  # WHAT SHIPS IS THE ZIP, NOT THE DIRECTORY ABOVE — and this command is
  # what turns one into the other on the consumer's machine.
  #
  # macOS frameworks are VERSIONED bundles: `Versions/A/...` plus three
  # symlinks (`Versions/Current`, the top-level binary, `Resources`). Those
  # symlinks are mandatory — measured on a real Mac, every symlink-free
  # layout fails, either at `codesign` ("bundle format is ambiguous") or at
  # build time ("did not contain an Info.plist" / "does not use shallow
  # bundles"). But `dart pub publish` DEREFERENCES symlinks when it builds
  # its archive, so a framework directory placed in the archive reaches the
  # consumer as exactly that broken shape, and their `flutter build macos`
  # dies with "Command CodeSign failed with a nonzero exit code".
  #
  # A zip stores symlinks, so the archive carries
  # `Frameworks/CratestackCborNative.xcframework.zip` (built by `just
  # cbor-vendor-macos`) and this `prepare_command` unpacks it here at
  # `pod install` time, restoring the symlinks before CocoaPods ever reads
  # `vendored_frameworks` above. `prepare_command` running for a `:path`
  # pod — the only way Flutter ever integrates a plugin — was verified
  # directly, not assumed.
  #
  # Idempotent by design: `pod install` runs on every build, and the
  # unpacked directory is the normal state on a machine that just ran
  # `just cbor-vendor-macos`. The zip is absent in exactly that case for a
  # source checkout, hence both guards rather than an unconditional unzip.
  # `ditto -x -k` mirrors the `ditto -c -k` that wrote it; `unzip` is NOT
  # equivalent here (it is not guaranteed present, and Apple's archiver is
  # what preserves the bundle faithfully).
  s.prepare_command = <<-CMD
    set -e
    fw="Frameworks/CratestackCborNative.xcframework"
    if [ ! -d "$fw" ] && [ -f "$fw.zip" ]; then
      ditto -x -k "$fw.zip" Frameworks/
    fi
  CMD

  s.dependency 'FlutterMacOS'

  # 10.15 matches the xcframework's own Info.plist `LSMinimumSystemVersion`
  # (see `just cbor-vendor-macos`) — kept in lockstep so CocoaPods' own
  # deployment-target check and the vendored binary's declared minimum
  # agree, rather than picking a different value here independently.
  s.platform = :osx, '10.15'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
end
