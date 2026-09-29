#!/usr/bin/env bash
# Fetches real Firefox-generated test profiles (from the firefox_decrypt project,
# GPL-3.0, not vendored here) and runs the decryption test against them.
#
#   tools/fetch-firefox-testdata.sh            # clone + run the test
#   LIMBO_FIREFOX_TESTDATA=... cargo test -p limbo-core real_firefox_profiles
set -euo pipefail
cache="$(dirname "$0")/.cache/firefox_decrypt"
if [ ! -d "$cache" ]; then
  git clone --depth 1 https://github.com/unode/firefox_decrypt.git "$cache"
fi
export LIMBO_FIREFOX_TESTDATA="$(cd "$cache/tests/test_data" && pwd)"
echo "LIMBO_FIREFOX_TESTDATA=$LIMBO_FIREFOX_TESTDATA"
cargo test -p limbo-core real_firefox_profiles -- --nocapture
