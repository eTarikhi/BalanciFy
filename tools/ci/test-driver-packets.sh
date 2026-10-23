#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM
compiler=${CC:-cc}
strict_flags='-std=c11 -pedantic -Wall -Wextra -Wconversion -Wsign-conversion -Werror'
san_flags='-fsanitize=address,undefined -fno-omit-frame-pointer -g'

run_c_test() {
    source_file=$1
    name=$(basename "$source_file" .c)
    # Keep a warning-clean portable build, then repeat under memory/UB sanitizers
    # so parser and queue boundary fixtures exercise both instrumented paths.
    # shellcheck disable=SC2086
    "$compiler" $strict_flags "$repo_root/$source_file" -o "$tmp_dir/$name"
    "$tmp_dir/$name"
    # shellcheck disable=SC2086
    "$compiler" $strict_flags $san_flags "$repo_root/$source_file" -o "$tmp_dir/${name}-sanitized"
    ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 \
        UBSAN_OPTIONS=halt_on_error=1 \
        "$tmp_dir/${name}-sanitized"
}

run_c_test driver/tests/test_ip_packet_validation.c
run_c_test driver/tests/test_mtu_config.c
run_c_test driver/tests/test_rx_pause_state.c
run_c_test driver/tests/test_tx_packet_queue.c
python3 "$repo_root/driver/tests/test_oid_contract.py"
printf '%s\n' 'driver packet and OID contract tests passed (strict C, ASan, UBSan)'
