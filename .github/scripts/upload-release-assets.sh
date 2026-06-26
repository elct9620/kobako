#!/bin/bash
# Attach the Guest Binary variants and gem backups to the GitHub Release
# named by $TAG. Expects the kobako-wasm artifact under data/ and the
# built gems under pkg/.
#
# The pure Guest Binary feeds `rake bench:confirm[<version>]` (paired
# regression arbitration against the released build); the regexp and json
# variants are downloadables for guests that opt the capability in, and
# the .gem files are the RubyGems release backup.
set -euo pipefail

version="${TAG#v}"
cp data/kobako.wasm "kobako-${version}.wasm"
cp "data/kobako+regexp.wasm" "kobako+regexp-${version}.wasm"
cp "data/kobako+regexp-unicode.wasm" "kobako+regexp-unicode-${version}.wasm"
cp "data/kobako+json.wasm" "kobako+json-${version}.wasm"
cp "data/kobako+full.wasm" "kobako+full-${version}.wasm"
gh release upload "$TAG" \
  "kobako-${version}.wasm" \
  "kobako+regexp-${version}.wasm" \
  "kobako+regexp-unicode-${version}.wasm" \
  "kobako+json-${version}.wasm" \
  "kobako+full-${version}.wasm" \
  pkg/*.gem --clobber
