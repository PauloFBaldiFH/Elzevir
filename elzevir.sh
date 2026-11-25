#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Run from a source checkout, including before the first build.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$root"
exec cargo run --locked -p elzevir -- "$@"
