#!/usr/bin/env bash
# Package the built gatedesk into GateDesk.app with a proper Info.plist, icon and
# code signature, so macOS TCC attributes Screen Recording / Microphone grants to
# com.carriez.GateDesk instead of the terminal that launched it.
#
# Usage:
#   make-gatedesk-app.sh [release|debug]            # default release
#
# Every built slice is merged with lipo, so a .app that runs on both Intel and
# Apple Silicon needs both cross builds. The arm64 one needs its own vcpkg deps,
# because libs/scrap/build.rs links $VCPKG_ROOT/installed/arm64-osx when the
# target arch is aarch64:
#   export VCPKG_ROOT="$HOME/repos/vcpkg"
#   rustup target add aarch64-apple-darwin
#   "$VCPKG_ROOT/vcpkg" install --triplet arm64-osx --x-install-root="$VCPKG_ROOT/installed"
#   cargo build --release --target aarch64-apple-darwin
#   cargo build --release --target x86_64-apple-darwin
# With a single slice the .app is that architecture alone, and the two failure
# modes differ: x86_64 needs Rosetta 2 on Apple Silicon, arm64 is refused
# outright on an Intel Mac.
#
# After building, grant permissions once:
#   系统设置 > 隐私与安全性 > 屏幕录制          → GateDesk 开
#   系统设置 > 隐私与安全性 > 麦克风            → GateDesk 开
# Then launch with `open .../GateDesk.app`, or run the inner binary directly
# to keep logs in the terminal — TCC still attributes it to the enclosing bundle.
set -euo pipefail

PROFILE="${1:-release}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)"
IDENTITY="GateDesk Development"

APP="$REPO/target/$PROFILE/GateDesk.app"
# The same png flutter_launcher_icons uses for the Flutter build
# (flutter/pubspec.yaml), so both .app forms carry one icon.
ICON_SRC="$REPO/res/mac-icon.png"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# Every gatedesk that has been built, to be merged into one universal binary. A
# cross build lands in target/<triple>/<profile>, a plain host build in
# target/<profile>; the host path is listed last so an explicit x86_64 cross build
# wins over it. Keyed on the whole architecture list, so a candidate whose
# architectures are all covered by an earlier one is skipped - including the case
# of an already universal binary, which covers everything.
declare -a SLICES=()
ARCHS_SEEN=" "
for cand in \
    "$REPO/target/aarch64-apple-darwin/$PROFILE/gatedesk" \
    "$REPO/target/x86_64-apple-darwin/$PROFILE/gatedesk" \
    "$REPO/target/$PROFILE/gatedesk"
do
    [ -x "$cand" ] || continue
    arches="$(lipo -archs "$cand" 2>/dev/null || true)"
    [ -n "$arches" ] || continue
    case "$ARCHS_SEEN" in
        *"$arches"*) continue ;;
    esac
    ARCHS_SEEN="$ARCHS_SEEN$arches;"
    SLICES+=("$cand")
done

if [ ${#SLICES[@]} -eq 0 ]; then
    echo "error: no gatedesk binary under $REPO/target" >&2
    echo "build it first, e.g. cargo build --release --target x86_64-apple-darwin" >&2
    exit 1
fi

# runtime-dlopen'd by rust-sciter UI. The pristine sciter download is already a
# universal binary, so one copy serves every slice; a copy that an earlier build
# left next to the binary is the fallback.
DYLIB=""
for cand in "$REPO/libsciter.dylib" "$REPO/target/$PROFILE/libsciter.dylib"; do
    if [ -f "$cand" ]; then
        DYLIB="$cand"
        break
    fi
done

"$SCRIPT_DIR/create-codesign-cert.sh" "$IDENTITY"

echo "Packaging $PROFILE build into $APP ..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
if [ ${#SLICES[@]} -eq 1 ]; then
    cp "${SLICES[0]}" "$APP/Contents/MacOS/gatedesk"
else
    lipo -create "${SLICES[@]}" -output "$APP/Contents/MacOS/gatedesk"
fi
ARCHS="$(lipo -archs "$APP/Contents/MacOS/gatedesk")"
echo "Architectures: $ARCHS"
case " $ARCHS " in
    *" arm64 "*)
        case " $ARCHS " in *" x86_64 "*) ;; *) echo "note: no x86_64 slice; Intel Macs cannot run this build" >&2 ;; esac
        ;;
    *)
        echo "note: no arm64 slice; Apple Silicon opens this build through Rosetta 2 only" >&2
        ;;
esac

# Built here rather than committed so the icon cannot drift from ICON_SRC. Must land
# before the signature, which seals everything under Contents/.
if [ -f "$ICON_SRC" ]; then
    ICONSET="$TMP_DIR/AppIcon.iconset"
    mkdir -p "$ICONSET"
    for size in 16 32 128 256 512; do
        big=$((size * 2))
        sips -z "$size" "$size" "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
        sips -z "$big" "$big" "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
else
    echo "warning: $ICON_SRC not found; the bundle keeps the generic app icon" >&2
fi

if [ -n "$DYLIB" ]; then
    cp "$DYLIB" "$APP/Contents/MacOS/libsciter.dylib"
    # Sign the dylib itself so it is a properly sealed nested code object.
    codesign --force --timestamp=none --sign "$IDENTITY" \
        "$APP/Contents/MacOS/libsciter.dylib"
else
    echo "warning: libsciter.dylib not found (looked in $REPO); GateDesk UI will fail to load sciter" >&2
fi
cp "$SCRIPT_DIR/GateDesk.plist" "$APP/Contents/Info.plist"
cp "$SCRIPT_DIR/GateDesk.entitlements" "$APP/Contents/Resources/GateDesk.entitlements"

echo "Signing with identity '$IDENTITY' ..."
codesign --force --timestamp=none \
    --sign "$IDENTITY" \
    --entitlements "$APP/Contents/Resources/GateDesk.entitlements" \
    "$APP"

codesign --verify --strict --verbose=2 "$APP"
echo
echo "OK: $APP"
codesign -dv --verbose=4 "$APP" 2>&1 | sed -n '1,8p'
echo
cat <<EOF
Next steps:
  1. Grant permissions once (系统设置 > 隐私与安全性):
       - 屏幕录制 / Screen Recording: GateDesk
       - 麦克风 / Microphone:            GateDesk
  2. Launch:   open "$APP"
     or keep terminal logs:  "$APP/Contents/MacOS/gatedesk"
  3. After granting, sanity-check the TCC record exists:
       tccutil reset ScreenCapture com.carriez.GateDesk   # should NOT say "no such identifier"
EOF


