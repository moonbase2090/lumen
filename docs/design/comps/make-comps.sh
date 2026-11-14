#!/bin/sh
# Renders the comps: each scene in docs/design/comps/scenes.py, in each
# built-in theme, through the terminal's headless snapshot.
#
#   make-comps.sh BRIGHT_BINARY SCENE_DIR OUT_DIR
#
# Each theme gets its own throwaway home directory holding only the settings
# file (the theme) and the policy file (images off, so the image block shows
# its alt text). The program
# turns off output processing first, as a program that writes frames does, so
# the line discipline does not change the frames' bytes.
set -eu
bin=$1
scenes=$2
out=$3
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for theme in light dark high-contrast; do
    home=$work/$theme
    mkdir -p "$home/.config/lumen"
    chmod 700 "$home" "$home/.config" "$home/.config/lumen"
    printf 'theme = "%s"\n' "$theme" >"$home/.config/lumen/settings.toml"
    printf 'images = false\n' >"$home/.config/lumen/policy.toml"
    chmod 600 "$home/.config/lumen/settings.toml" "$home/.config/lumen/policy.toml"
    for scene in grid rich; do
        cp "$scenes/$scene.bin" "$home/scene.bin"
        (cd "$home" && HOME=$home "$bin" --size 1200x760 --scale 2 --wait 1500 \
            --snapshot "$out/$scene-$theme.png" -- /bin/sh -c 'stty -opost; cat scene.bin; sleep 3')
    done
done
