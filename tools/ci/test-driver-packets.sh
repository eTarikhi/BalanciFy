#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

${CC:-cc} -std=c11 -pedantic -Wall -Wextra -Wconversion -Wsign-conversion -Werror \
    "$repo_root/driver/tests/test_ip_packet_validation.c" \
    -o "$tmp_dir/test_ip_packet_validation"
"$tmp_dir/test_ip_packet_validation"
${CC:-cc} -std=c11 -pedantic -Wall -Wextra -Wconversion -Wsign-conversion -Werror \
    "$repo_root/driver/tests/test_mtu_config.c" \
    -o "$tmp_dir/test_mtu_config"
"$tmp_dir/test_mtu_config"
python3 "$repo_root/driver/tests/test_oid_contract.py"
printf '%s\n' 'driver packet and OID contract tests passed'
