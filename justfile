# Photo Cruiser. `just` lists recipes. Every cargo call needs the system pkg-config
# (Linuxbrew's shadows it and breaks libheif detection).

export PKG_CONFIG := "/usr/bin/pkg-config"
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

# Format all crates
fmt:
    cargo fmt --all

# All tests (pass e.g. `-p photo-cruiser-core` or a test name)
test *args:
    cargo test --workspace {{args}}

# Build the debug binaries (pcr + photo-cruiser)
build:
    cargo build --workspace

# Launch the GUI on a folder (default: fixtures/)
[no-cd]
run dir=fixtures:
    cargo run -q -p photo-cruiser-app -- "{{dir}}"

# Run the CLI: `just cli list fixtures --json`
[no-cd]
cli *args:
    cargo run -q -p photo-cruiser-cli -- {{args}}

# Drive the real GUI window with a script; writes <artifacts>/<name>.png + .json
[no-cd]
shot script=(root / "scripts/smoke.json") dir=fixtures name="shot":
    mkdir -p "{{artifacts}}"
    cargo run -q -p photo-cruiser-app -- "{{dir}}" --script "{{script}}" --shot "{{artifacts}}/{{name}}.png" --dump-state "{{artifacts}}/{{name}}.json"
    cat "{{artifacts}}/{{name}}.json"

# Regenerate fixtures/ (needs uv; pulls pillow + pillow-heif)
fixtures:
    uv run -q "{{root}}/scripts/gen_fixtures.py" "{{fixtures}}"

# Full verification: check + CLI smoke + real-window smoke shot with state assertions
verify: check build
    #!/usr/bin/env bash
    set -euo pipefail
    pcr="{{root}}/target/debug/pcr"
    mkdir -p "{{artifacts}}"
    test "$("$pcr" list "{{fixtures}}" --json | jq .count)" = 9
    "$pcr" render "{{fixtures}}/img5.HEIC" --crop 10,20,100,50 -o "{{artifacts}}/verify-crop.png" | grep -q "100x50"
    "{{root}}/target/debug/photo-cruiser" "{{fixtures}}" --script "{{root}}/scripts/smoke.json" \
        --shot "{{artifacts}}/verify.png" --dump-state "{{artifacts}}/verify.json"
    jq -e '.index == 4 and .mode == "crop" and .crop == {"x":40,"y":30,"w":200,"h":150} and .errors == [] and .loading == false' \
        "{{artifacts}}/verify.json" >/dev/null || { cat "{{artifacts}}/verify.json"; exit 1; }
    echo "verify ok: {{artifacts}}/verify.png {{artifacts}}/verify.json"
