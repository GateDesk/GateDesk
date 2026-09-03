#!/usr/bin/env bash
# Create a stable self-signed code-signing identity in the login keychain.
# TCC (Screen Recording / Microphone) grants are bound to this identity, so as
# long as the certificate and bundle id do not change, permissions survive
# recompiling gatedesk.
#
# Note: `find-identity` reports self-signed certs as "0 valid" because they are
# not trusted for chain evaluation. The authoritative check is a trial
# `codesign`, which is what we gate on here.
#
# Homebrew OpenSSL builds the certificate (LibreSSL drops extendedKeyUsage in
# `x509 -req`); LibreSSL exports the .p12 (macOS rejects OpenSSL 3 PKCS12).
set -euo pipefail

IDENTITY="${1:-GateDesk Development}"
KEYCHAIN="${KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"

if command -v brew >/dev/null 2>&1 && [ -x "$(brew --prefix openssl 2>/dev/null)/bin/openssl" ]; then
    GEN_OPENSSL="$(brew --prefix openssl)/bin/openssl"
else
    GEN_OPENSSL="/usr/bin/openssl"
fi
P12_OPENSSL="/usr/bin/openssl"

identity_usable() {
    local probe
    probe="$(mktemp)"
    printf 'x' > "$probe"
    if codesign --force --timestamp=none --sign "$IDENTITY" "$probe" >/dev/null 2>&1; then
        rm -f "$probe"
        return 0
    fi
    rm -f "$probe"
    return 1
}

if identity_usable; then
    echo "Code-signing identity already usable: $IDENTITY"
    exit 0
fi

# Drop any earlier GateDesk certs so codesign never sees an ambiguous match.
security delete-certificate -c "$IDENTITY" >/dev/null 2>&1 || true

echo "Generating with: $("$GEN_OPENSSL" version)"
echo "Creating self-signed code-signing identity '$IDENTITY' ..."

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/ext.cnf" <<EOF
[gd]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = codeSigning
EOF

"$GEN_OPENSSL" genrsa -out "$tmp/key.pem" 2048 2>/dev/null
"$GEN_OPENSSL" req -new -key "$tmp/key.pem" -out "$tmp/req.csr" -subj "/CN=$IDENTITY/O=GateDesk"
"$GEN_OPENSSL" x509 -req -days 3650 -in "$tmp/req.csr" -signkey "$tmp/key.pem" \
    -out "$tmp/cert.pem" -extfile "$tmp/ext.cnf" -extensions gd 2>/dev/null
"$P12_OPENSSL" pkcs12 -export -out "$tmp/id.p12" -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
    -name "$IDENTITY" -passout "pass:gd"

security import "$tmp/id.p12" -k "$KEYCHAIN" -P gd \
    -T /usr/bin/codesign -T /usr/bin/security

if identity_usable; then
    echo "Identity '$IDENTITY' is ready (may have prompted for keychain access on first use)."
else
    echo "error: identity '$IDENTITY' not usable after import." >&2
    exit 1
fi
