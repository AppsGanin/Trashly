#!/usr/bin/env bash
# Generate Trashly's self-signed code-signing certificate (free, no Apple
# Developer account).
#
# Why: macOS ties privacy grants such as Full Disk Access to an app's
# "designated requirement". An ad-hoc signature's requirement is the hash of
# that exact build, so every update silently loses the grant. Signing every
# release with this one certificate makes the requirement
#   identifier "com.ganin.trashly" and certificate leaf = H"<cert hash>"
# which stays the same across updates.
#
# Run once, then store the output as GitHub secrets (commands are printed at
# the end). Keep the .p12 backed up: a new certificate means every user has to
# grant Full Disk Access again.
#
# Usage: scripts/make-signing-cert.sh [output-dir]   (default: ~/.trashly-signing)
set -euo pipefail

NAME="Trashly Self-Signed" # must match APPLE_SIGNING_IDENTITY in release-please.yml
OUT="${1:-$HOME/.trashly-signing}"

if [ -e "$OUT/trashly-signing.p12" ]; then
  echo "error: $OUT/trashly-signing.p12 already exists — refusing to overwrite it." >&2
  exit 1
fi
mkdir -p "$OUT"
chmod 700 "$OUT"
cd "$OUT"

PASSWORD="$(openssl rand -hex 24)"

openssl req -x509 -newkey rsa:2048 -nodes -days 7300 \
  -subj "/CN=$NAME" \
  -addext "basicConstraints=critical,CA:false" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" \
  -keyout key.pem -out cert.pem 2>/dev/null

# macOS `security import` needs the legacy PKCS#12 ciphers; OpenSSL 3 only
# writes them with -legacy, while LibreSSL (/usr/bin/openssl) has no such flag.
if ! openssl pkcs12 -export -legacy -inkey key.pem -in cert.pem -name "$NAME" \
  -passout "pass:$PASSWORD" -out trashly-signing.p12 2>/dev/null; then
  openssl pkcs12 -export -inkey key.pem -in cert.pem -name "$NAME" \
    -passout "pass:$PASSWORD" -out trashly-signing.p12
fi
rm key.pem

base64 -i trashly-signing.p12 | tr -d '\n' > trashly-signing.p12.base64
printf '%s' "$PASSWORD" > trashly-signing.password
chmod 600 trashly-signing.*

cat <<EOF
Created in $OUT:
  trashly-signing.p12         certificate + private key (back this up!)
  trashly-signing.p12.base64  value for the MACOS_SIGNING_CERT secret
  trashly-signing.password    value for the MACOS_SIGNING_CERT_PASSWORD secret
  cert.pem                    public certificate

Add the secrets to the repository:
  gh secret set MACOS_SIGNING_CERT < "$OUT/trashly-signing.p12.base64"
  gh secret set MACOS_SIGNING_CERT_PASSWORD < "$OUT/trashly-signing.password"
EOF
