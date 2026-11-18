#!/bin/sh
set -eu

public_dir=${1:?usage: check-client-styles.sh <web-public-dir>}
assets_dir="$public_dir/assets"

for asset in tailwind main workspace workflow-editor workflow-editor-canvas type-scale workflow-document-menu; do
    matches=$(find "$assets_dir" -maxdepth 1 -type f -name "$asset-*.css" -print -quit)
    if [ -z "$matches" ] || [ ! -s "$matches" ]; then
        echo "Missing compiled stylesheet for $asset in $assets_dir" >&2
        exit 1
    fi
done

if ! grep -l '\.login-screen' "$assets_dir"/workspace-*.css >/dev/null; then
    echo "Compiled workspace stylesheet is missing the login screen rules" >&2
    exit 1
fi
