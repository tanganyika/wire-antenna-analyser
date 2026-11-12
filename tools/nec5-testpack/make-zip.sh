#!/bin/sh
# Builds dist/wireant-nec5-testpack.zip: the recipient's guide, the runner and the decks.
set -e
cd "$(dirname "$0")"
rm -rf dist && mkdir -p dist/wireant-nec5-testpack
cp README.txt run-tests.bat run-tests.ps1 dist/wireant-nec5-testpack/
cp -r decks dist/wireant-nec5-testpack/
(cd dist && zip -qr wireant-nec5-testpack.zip wireant-nec5-testpack && rm -r wireant-nec5-testpack)
echo "dist/wireant-nec5-testpack.zip"
