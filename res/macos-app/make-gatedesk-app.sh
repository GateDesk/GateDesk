#!/usr/bin/env bash
# Package target/<profile>/gatedesk into GateDesk.app with a proper Info.plist
# and code signature, so macOS TCC attributes Screen Recording / Microphone
# grants to com.carriez.GateDesk instead of the terminal that launched it.
#
# Usage:
#   make-gatedesk-app.sh [release|debug]            # default release
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

BIN="$REPO/target/$PROFILE/gatedesk"
DYLIB="$REPO/target/$PROFILE/libsciter.dylib"   # runtime-dlopen'd by rust-sciter UI
APP="$REPO/target/$PROFILE/GateDesk.app"

if [ ! -x "$BIN" ]; then
    echo "error: binary not found: $BIN" >&2
    echo "build it first, e.g. cargo build --release" >&2
    exit 1
fi

"$SCRIPT_DIR/create-codesign-cert.sh" "$IDENTITY"

echo "Packaging $PROFILE build into $APP ..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/gatedesk"
if [ -f "$DYLIB" ]; then
    cp "$DYLIB" "$APP/Contents/MacOS/libsciter.dylib"
    # Sign the dylib itself so it is a properly sealed nested code object.
    codesign --force --timestamp=none --sign "$IDENTITY" \
        "$APP/Contents/MacOS/libsciter.dylib"
else
    echo "warning: $DYLIB not found; GateDesk UI will fail to load sciter" >&2
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
