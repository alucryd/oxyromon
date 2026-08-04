#!/bin/sh
# Fetch Web Awesome and the handful of Font Awesome icons its components use,
# into frontend/vendor/ for Trunk to copy into the build.
#
# The UI has to work offline — it is served by `oxyromon server` and bundled
# into the desktop app — so nothing may be loaded from a CDN at runtime. This
# pulls the pieces once at build time instead; the result is gitignored and the
# script is a no-op when the pinned version is already unpacked.
set -eu

WEBAWESOME_VERSION=3.11.0
FONTAWESOME_VERSION=7.3.1

# Every icon name referenced from inside a Web Awesome component. Without these
# vendored, components fall back to the Font Awesome CDN and render nothing when
# offline — a dialog would lose its close button.
ICONS="bars check chevron-down circle-xmark clock copy ellipsis eye eye-slash
grip-vertical minus pause plus star user xmark"

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
vendor="$root/vendor"
stamp="$vendor/.version"
want="webawesome-$WEBAWESOME_VERSION fontawesome-$FONTAWESOME_VERSION"

if [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$want" ]; then
    exit 0
fi

echo "fetching Web Awesome $WEBAWESOME_VERSION"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL "https://registry.npmjs.org/@awesome.me/webawesome/-/webawesome-$WEBAWESOME_VERSION.tgz" |
    tar -xzf- -C "$tmp"

rm -rf "$vendor"
mkdir -p "$vendor"
# dist-cdn is the pre-bundled build meant to be loaded straight from a browser,
# so it needs no bundler of its own.
cp -r "$tmp/package/dist-cdn" "$vendor/webawesome"

# Types, framework wrappers and documentation are not needed to run it, and
# together they are most of the package.
rm -rf "$vendor/webawesome/react" "$vendor/webawesome/ssr" \
    "$vendor/webawesome/skills" "$vendor/webawesome/types"
find "$vendor/webawesome" -name '*.d.ts' -delete
find "$vendor/webawesome" -name '*.map' -delete

# Editor and AI metadata: nearly half the remaining weight, and every byte of
# it would end up embedded in the oxyromon binary.
rm -f "$vendor/webawesome/custom-elements.json" \
    "$vendor/webawesome/custom-elements-jsx.d.ts" \
    "$vendor/webawesome/web-types.json" \
    "$vendor/webawesome/vscode.html-custom-data.json" \
    "$vendor/webawesome/llms.txt" \
    "$vendor/webawesome/webawesome.ssr-loader.js"

echo "fetching Font Awesome $FONTAWESOME_VERSION icons"
curl -fsSL "https://registry.npmjs.org/@fortawesome/fontawesome-free/-/fontawesome-free-$FONTAWESOME_VERSION.tgz" |
    tar -xzf- -C "$tmp" "package/svgs/solid"

# setIconPath() resolves `<wa-icon name="x">` to <path>/solid/x.svg, so keep
# that layout but only for the icons actually asked for.
mkdir -p "$vendor/icons/solid"
for icon in $ICONS; do
    if [ -f "$tmp/package/svgs/solid/$icon.svg" ]; then
        cp "$tmp/package/svgs/solid/$icon.svg" "$vendor/icons/solid/$icon.svg"
    else
        echo "  warning: no such Font Awesome icon: $icon" >&2
    fi
done

printf '%s' "$want" > "$stamp"
echo "vendored $(du -sh "$vendor" | cut -f1) into frontend/vendor"
