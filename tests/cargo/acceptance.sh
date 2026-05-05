#!/bin/bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
workspace_manifest="$repo_root/Cargo.toml"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/rapport-cargo-acceptance.XXXXXX")"
failure_log="$tmp_root/failures.log"

trap 'rm -rf "$tmp_root"' EXIT
cd "$repo_root"

run_step() {
    local title="$1"
    shift
    printf '\n=== %s ===\n' "$title"
    "$@"
}

toml_string() {
    local file="$1"
    local key="$2"

    awk -v key="$key" '
        $0 ~ "^[[:space:]]*" key "[[:space:]]*=" {
            value = $0
            sub(/^[^=]*=[[:space:]]*/, "", value)
            sub(/[[:space:]]*#.*$/, "", value)
            sub(/^[[:space:]]*/, "", value)
            sub(/[[:space:]]*$/, "", value)
            if (value ~ /^".*"$/) {
                sub(/^"/, "", value)
                sub(/"$/, "", value)
                print value
                found = 1
                exit
            }
        }
        END {
            if (!found) {
                exit 1
            }
        }
    ' "$file"
}

optional_toml_string() {
    toml_string "$@" 2>/dev/null || true
}

required_toml_string() {
    local value
    value="$(toml_string "$1" "$2")" || {
        printf '%s is missing string key `%s`\n' "$1" "$2" >&2
        exit 2
    }
    printf '%s\n' "$value"
}

assert_fixture_relative_path() {
    local file="$1"
    local key="$2"
    local path="$3"

    if [[ "$path" = /* || "$path" == ".." || "$path" == ../* || "$path" == */../* || "$path" == */.. ]]; then
        printf '%s `%s` must stay inside the fixture\n' "$file" "$key" >&2
        exit 2
    fi
}

sed_escape_replacement() {
    printf '%s' "$1" | sed 's/[&|]/\\&/g'
}

normalize_file() {
    local input="$1"
    local output="$2"
    local escaped_repo

    escaped_repo="$(sed_escape_replacement "$repo_root")"
    sed -E \
        -e 's/\r$//' \
        -e "s|$escaped_repo|{repo}|g" \
        -e 's/^duration: .*/duration: <duration>/' \
        "$input" > "$output"
}

compare_snapshot() {
    local test_name="$1"
    local stream="$2"
    local expected="$3"
    local actual="$4"
    local normalized_expected="$tmp_root/expected.$stream"
    local normalized_actual="$tmp_root/actual.$stream"

    normalize_file "$expected" "$normalized_expected"
    normalize_file "$actual" "$normalized_actual"

    if cmp -s "$normalized_expected" "$normalized_actual"; then
        return 0
    fi

    {
        printf '\n--- %s %s snapshot mismatch ---\n' "$test_name" "$stream"
        diff -u "$normalized_expected" "$normalized_actual" || true
    } >> "$failure_log"

    return 1
}

run_expectation() {
    local expectation="$1"
    local expectations_dir case_dir case_name expectation_name config input_path argument
    local verb status stdout_snapshot stderr_snapshot run_dir stdout stderr code expected_success
    local failed=0

    expectations_dir="$(dirname "$expectation")"
    case_dir="$(dirname "$expectations_dir")"
    case_name="${case_dir#"$script_dir"/}"
    expectation_name="$(basename "$expectation")"
    config="$case_dir/rapport.toml"

    input_path="$(optional_toml_string "$config" path)"
    if [[ -z "$input_path" ]]; then
        input_path="."
    fi
    assert_fixture_relative_path "$config" path "$input_path"

    verb="$(required_toml_string "$expectation" verb)"
    status="$(required_toml_string "$expectation" status)"
    stdout_snapshot="$(optional_toml_string "$expectation" stdout)"
    stderr_snapshot="$(optional_toml_string "$expectation" stderr)"

    argument="tests/cargo/$case_name"
    if [[ "$input_path" != "." ]]; then
        argument="$argument/$input_path"
    fi

    run_dir="$tmp_root/runs/${case_name//\//__}/${expectation_name%.toml}"
    mkdir -p "$run_dir/target" "$run_dir/cargo-home"
    stdout="$run_dir/stdout"
    stderr="$run_dir/stderr"

    set +e
    (
        unset CARGO_ENCODED_RUSTFLAGS
        unset CARGO_BUILD_TARGET
        unset RUSTC_WRAPPER
        unset RUSTC_WORKSPACE_WRAPPER
        unset RUSTFLAGS
        unset RUSTDOCFLAGS
        export CARGO_TARGET_DIR="$run_dir/target"
        export CARGO_HOME="$run_dir/cargo-home"
        export CARGO_BUILD_JOBS=1
        export CARGO_TERM_COLOR=never
        export CARGO_INCREMENTAL=0
        "$rapport_bin" "$verb" "$argument" > "$stdout" 2> "$stderr"
    )
    code=$?
    set -e

    case "$status" in
        ok)
            expected_success=0
            ;;
        fail)
            expected_success=1
            ;;
        *)
            printf '%s has unsupported status `%s`\n' "$expectation" "$status" >&2
            exit 2
            ;;
    esac

    if [[ "$expected_success" -eq 0 && "$code" -ne 0 ]] || [[ "$expected_success" -eq 1 && "$code" -eq 0 ]]; then
        {
            printf '\n--- %s/%s status mismatch ---\n' "$case_name" "$expectation_name"
            printf 'expected: %s\nactual: %s\n' "$status" "$code"
            printf 'command: %s %s %s\n' "$rapport_bin" "$verb" "$argument"
            printf 'stdout:\n'
            cat "$stdout"
            printf '\nstderr:\n'
            cat "$stderr"
            printf '\n'
        } >> "$failure_log"
        failed=1
    fi

    if [[ -n "$stdout_snapshot" ]]; then
        assert_fixture_relative_path "$expectation" stdout "$stdout_snapshot"
        compare_snapshot "$case_name/$expectation_name" stdout "$expectations_dir/$stdout_snapshot" "$stdout" || failed=1
    fi

    if [[ -n "$stderr_snapshot" ]]; then
        assert_fixture_relative_path "$expectation" stderr "$stderr_snapshot"
        compare_snapshot "$case_name/$expectation_name" stderr "$expectations_dir/$stderr_snapshot" "$stderr" || failed=1
    fi

    if [[ "$failed" -eq 0 ]]; then
        printf 'ok   %s/%s\n' "$case_name" "$expectation_name"
        return 0
    fi

    printf 'FAIL %s/%s\n' "$case_name" "$expectation_name"
    return 1
}

rapport_bin="${RAPPORT_BIN:-}"
if [[ -z "$rapport_bin" ]]; then
    run_step "build rapport" cargo build --manifest-path "$workspace_manifest" -p rapport
    rapport_bin="$repo_root/target/debug/rapport"
fi

if [[ ! -x "$rapport_bin" ]]; then
    printf 'rapport binary is not executable: %s\n' "$rapport_bin" >&2
    exit 2
fi

mapfile -d '' expectations < <(
    find "$script_dir" -path '*/expectations/*.toml' -type f \
        \( -name '*.ok.toml' -o -name '*.fail.toml' \) \
        -print0 | sort -z
)

if [[ "${#expectations[@]}" -eq 0 ]]; then
    printf 'no cargo acceptance expectations found in %s\n' "$script_dir" >&2
    exit 1
fi

printf '\n=== cargo acceptance ===\n'
failures=0
for expectation in "${expectations[@]}"; do
    if ! run_expectation "$expectation"; then
        failures=$((failures + 1))
    fi
done

printf '\n=== summary ===\n'
printf '%s expectation(s), %s failure(s)\n' "${#expectations[@]}" "$failures"

if [[ "$failures" -ne 0 ]]; then
    cat "$failure_log"
    exit 1
fi
