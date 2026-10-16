# Photo Cruiser. `just` lists recipes. PKG_CONFIG for libheif is set in .cargo/config.toml.

root := justfile_directory()
fixtures := root / "fixtures"
# Scratch dir for scripts, shots and state dumps; outside the repo.
artifacts := env_var_or_default("PC_ARTIFACTS", "/tmp/photo-cruiser")

default:
    @just --list --justfile "{{root}}/justfile"

# fmt check + clippy (-D warnings) + all tests
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

# All tests (pass e.g. `-p photo-cruiser-core` or a test name)
test *args:
    cargo test --workspace {{args}}

# Launch the GUI on a folder (default: fixtures/). Always a visible window.
[no-cd]
run dir=fixtures:
    cargo run -q -p photo-cruiser-app -- "{{dir}}"

# Live development: visible window that rebuilds and restarts on the same photo on every save
[no-cd]
dev dir=fixtures:
    "{{root}}/scripts/dev.sh" "{{dir}}"

# Run the CLI: `just cli list fixtures --json`
[no-cd]
cli *args:
    cargo run -q -p photo-cruiser-cli -- {{args}}

# Drive the real GUI window with a script; writes <artifacts>/<name>.png + .json.
# Runs in a headless compositor; PC_HEADLESS=0 shows the window.
[no-cd]
shot script=(root / "scripts/smoke.json") dir=fixtures name="shot":
    mkdir -p "{{artifacts}}"
    cargo build -q -p photo-cruiser-app
    "{{root}}/scripts/headless.sh" "{{root}}/target/debug/photo-cruiser" "{{dir}}" --script "{{script}}" --shot "{{artifacts}}/{{name}}.png" --dump-state "{{artifacts}}/{{name}}.json"
    cat "{{artifacts}}/{{name}}.json"

# Rewrite the render golden images (crates/core/tests/golden/) from current output.
# Only after checking the new output is right; plain test runs never write them.
bless-golden:
    PC_BLESS_GOLDEN=1 cargo test -p photo-cruiser-core --test golden render_matches_golden_images

# Regenerate fixtures/ (needs uv; pulls pillow + pillow-heif)
fixtures:
    uv run -q "{{root}}/scripts/gen_fixtures.py" "{{fixtures}}"

# Full verification: check + CLI smoke + real-window smoke shot (headless) with state assertions
verify: check
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --workspace
    pcr="{{root}}/target/debug/pcr"
    mkdir -p "{{artifacts}}"
    test "$("$pcr" list "{{fixtures}}" --json | jq .count)" = 9
    "$pcr" render "{{fixtures}}/img5.HEIC" --crop 10,20,100,50 -o "{{artifacts}}/verify-crop.png" | grep -q "100x50"
    code=0; out=$("$pcr" render "{{fixtures}}/img1.png" --crop 400,0,50,50 -o "{{artifacts}}/verify-empty.png" --json 2>/dev/null) || code=$?
    test "$code" = 3 && jq -e '.error.kind == "empty_crop"' <<<"$out" >/dev/null || { echo "empty crop: exit $code, $out"; exit 1; }
    "{{root}}/scripts/headless.sh" "{{root}}/target/debug/photo-cruiser" "{{fixtures}}" --script "{{root}}/scripts/smoke.json" \
        --shot "{{artifacts}}/verify.png" --dump-state "{{artifacts}}/verify.json"
    jq -e '.index == 4 and .view == "photo" and .mode == "crop" and .crop == {"x":40,"y":30,"w":200,"h":150} and .errors == [] and .loading == false' \
        "{{artifacts}}/verify.json" >/dev/null || { cat "{{artifacts}}/verify.json"; exit 1; }
    "{{root}}/scripts/headless.sh" "{{root}}/target/debug/photo-cruiser" "{{fixtures}}" --script "{{root}}/scripts/grid.json" \
        --shot "{{artifacts}}/verify-grid.png" --dump-state "{{artifacts}}/verify-grid.json"
    jq -e '.index == 1 and .view == "grid" and .errors == [] and .loading == false' \
        "{{artifacts}}/verify-grid.json" >/dev/null || { cat "{{artifacts}}/verify-grid.json"; exit 1; }
    # Save on leaving (no apply, no save step), on a scratch copy: never into fixtures/.
    photos="{{artifacts}}/verify-save-photos"
    if [ -e "$photos" ]; then trash-put "$photos"; fi
    mkdir -p "$photos" && cp -r "{{fixtures}}/." "$photos"
    "{{root}}/scripts/headless.sh" "{{root}}/target/debug/photo-cruiser" "$photos" --script "{{root}}/scripts/save.json" \
        --shot "{{artifacts}}/verify-save.png" --dump-state "{{artifacts}}/verify-save.json"
    jq -e '.index == 0 and .dirty == false and .saving == [] and .edits == [{"op":"crop","x":40,"y":30,"w":200,"h":150}] and (.backup | endswith(".photo-cruiser/originals/img1.png")) and .errors == []' \
        "{{artifacts}}/verify-save.json" >/dev/null || { cat "{{artifacts}}/verify-save.json"; exit 1; }
    "$pcr" info "$photos/img1.png" | grep -q "size: 200x150" || { echo "img1.png not saved cropped"; exit 1; }
    # Exposure: opens at auto, set by the slider, saved on leaving; reopened, it keeps its value.
    "{{root}}/scripts/headless.sh" "{{root}}/target/debug/photo-cruiser" "$photos" --script "{{root}}/scripts/exposure.json" \
        --shot "{{artifacts}}/verify-exposure.png" --dump-state "{{artifacts}}/verify-exposure.json"
    jq -e '.index == 1 and .dirty == false and .edits == [{"op":"adjust","tool":"exposure","value":0.8}] and .tool.value == 0.8 and .tool.opening < 0 and .preview == true and (.backup | endswith(".photo-cruiser/originals/img2.jpg")) and .errors == []' \
        "{{artifacts}}/verify-exposure.json" >/dev/null || { cat "{{artifacts}}/verify-exposure.json"; exit 1; }
    "$pcr" edits "$photos/img2.jpg" | grep -q '^saved: \[{"op":"adjust","tool":"exposure","value":0.8}\]' || { echo "img2.jpg not saved with exposure"; exit 1; }
    # Auto levels on the page of a document photo: opens at 1, double-click back to 1.
    "{{root}}/scripts/headless.sh" "{{root}}/target/debug/photo-cruiser" "$photos/adjust" --script "{{root}}/scripts/levels.json" \
        --shot "{{artifacts}}/verify-levels.png" --dump-state "{{artifacts}}/verify-levels.json"
    jq -e '.edits == [{"op":"crop","x":60,"y":0,"w":260,"h":240},{"op":"adjust","tool":"levels","value":1}] and .tool.tool == "levels" and .tool.value == 1 and .tool.opening == 1 and .preview == true and .dirty == true and .errors == []' \
        "{{artifacts}}/verify-levels.json" >/dev/null || { cat "{{artifacts}}/verify-levels.json"; exit 1; }
    echo "verify ok: {{artifacts}}/verify.png {{artifacts}}/verify-grid.png {{artifacts}}/verify-save.png {{artifacts}}/verify-exposure.png {{artifacts}}/verify-levels.png"

# Release build of the GUI and the CLI (run before `just install`)
build-release:
    cargo build --release --locked -p photo-cruiser-app -p photo-cruiser-cli

appid := "com.lukastrumm.PhotoCruiser"
# Current user by default; `sudo just prefix=/usr/local install` for all users.
prefix := home_directory() / ".local"
share := prefix / "share"

# Install the binaries, desktop entry and icon (`just build-release` first)
install:
    #!/usr/bin/env bash
    set -euo pipefail
    cd "{{root}}"
    for bin in photo-cruiser pcr; do
        test -x "target/release/$bin" || { echo "target/release/$bin missing: run 'just build-release' first"; exit 1; }
        install -Dm0755 "target/release/$bin" "{{prefix}}/bin/$bin"
    done
    install -Dm0644 "resources/{{appid}}.desktop" "{{share}}/applications/{{appid}}.desktop"
    install -Dm0644 "resources/icons/hicolor/scalable/apps/{{appid}}.svg" "{{share}}/icons/hicolor/scalable/apps/{{appid}}.svg"
    gtk-update-icon-cache -qtf "{{share}}/icons/hicolor" 2>/dev/null || true
    update-desktop-database -q "{{share}}/applications" 2>/dev/null || true
    echo "installed to {{prefix}}"

# Remove what `install` put in place (same prefix)
uninstall:
    -rm "{{prefix}}/bin/photo-cruiser" "{{prefix}}/bin/pcr" "{{share}}/applications/{{appid}}.desktop" "{{share}}/icons/hicolor/scalable/apps/{{appid}}.svg"
    -update-desktop-database -q "{{share}}/applications"
