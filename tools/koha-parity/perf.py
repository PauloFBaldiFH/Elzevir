#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Measures the Elzevir on a copy of the library: how long a command takes and how much
memory it needs, and how fast a running server answers the OPAC and the API
(docs/specs/migracao/performance.md).

    python3 -I tools/koha-parity/perf.py run LABEL [--log FILE] -- COMMAND ...
    python3 -I tools/koha-parity/perf.py http BASE QUERIES.json [--repeat 20] [--records 200] [--pid PID]

`run` prints one line: the label, the wall time in seconds, the most memory the command
held (its peak resident set) and its exit status. The command's own output goes to
`--log`, or nowhere, so no record or patron reaches the report.

`http` asks each query of QUERIES.json (a list of words, as typed in the search box)
`--repeat` times on the OPAC's `/search` and on the API's `/api/v1/search`, then opens
the record page of up to `--records` Instances the searches found, and prints the
median, the 95th percentile and the slowest answer of each, in milliseconds. One pass
runs first to warm the caches and is not counted, except for the record pages, whose
first opening is printed apart. With `--pid`, it also prints the
server's resident memory before and after. Only counts and times are printed.
"""

import argparse
import json
import resource
import subprocess
import sys
import time
import urllib.parse
import urllib.request

SHOWN_RECORDS = 200


def percentile(values, share):
    """The value below which `share` of `values` fall (nearest rank)."""
    ordered = sorted(values)
    if not ordered:
        return 0.0
    rank = max(1, -(-len(ordered) * share // 1))
    return ordered[int(rank) - 1]


def summary(name, times):
    """One report line for `times`, in seconds."""
    if not times:
        return f"{name}: no answers"
    milliseconds = [value * 1000 for value in times]
    return (f"{name}: {len(times)} answers, median {percentile(milliseconds, 0.5):.1f} ms, "
            f"p95 {percentile(milliseconds, 0.95):.1f} ms, slowest {max(milliseconds):.1f} ms")


def timed_get(url):
    """The seconds a GET of `url` took and its body; an error status stops the run."""
    started = time.perf_counter()
    with urllib.request.urlopen(url, timeout=120) as response:
        body = response.read()
    return time.perf_counter() - started, body


def resident_mb(pid):
    """The resident memory of process `pid`, in MiB, from /proc."""
    with open(f"/proc/{pid}/status", encoding="ascii") as status:
        for line in status:
            if line.startswith("VmRSS:"):
                return int(line.split()[1]) / 1024
    return 0.0


def run_command(args):
    """Runs the command and prints its time and peak memory."""
    log = open(args.log, "wb") if args.log else subprocess.DEVNULL
    try:
        started = time.perf_counter()
        status = subprocess.run(args.command, stdout=log, stderr=subprocess.STDOUT, check=False).returncode
        seconds = time.perf_counter() - started
    finally:
        if args.log:
            log.close()
    # Linux gives ru_maxrss in KiB.
    peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss / 1024
    print(f"{args.label}: {seconds:.1f} s, peak memory {peak:.0f} MiB, exit {status}")
    return status


def measure_http(args):
    """Asks the server and prints the latencies."""
    with open(args.queries, encoding="utf-8") as file:
        queries = json.load(file)
    base = args.base.rstrip("/")
    lines = []
    if args.pid:
        lines.append(f"server memory before: {resident_mb(args.pid):.0f} MiB")
    opac = [f"{base}/search?q=" + urllib.parse.quote(query) for query in queries]
    api = [f"{base}/api/v1/search?q=" + urllib.parse.quote(query) for query in queries]
    instances = []
    for url in api:
        _, body = timed_get(url)
        instances += [hit["instance"] for hit in json.loads(body).get("results", [])]
    records = [f"{base}/record/{number}" for number in dict.fromkeys(instances)][: args.records]
    for name, urls, repeat in (("OPAC search", opac, args.repeat), ("API search", api, args.repeat),
                               ("OPAC record", records, 1)):
        first = [timed_get(url)[0] for url in urls]
        if name == "OPAC record":
            # A record page is kept rendered once opened: the first opening reads it.
            lines.append(summary("OPAC record, first opening", first))
        times = [timed_get(url)[0] for _ in range(repeat) for url in urls]
        lines.append(summary(name, times))
    if args.pid:
        lines.append(f"server memory after: {resident_mb(args.pid):.0f} MiB")
    print("\n".join(lines))
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    actions = parser.add_subparsers(dest="action", required=True)
    run = actions.add_parser("run", help="time a command")
    run.add_argument("label")
    run.add_argument("--log")
    http = actions.add_parser("http", help="time the answers of a running server")
    http.add_argument("base")
    http.add_argument("queries")
    http.add_argument("--repeat", type=int, default=20)
    http.add_argument("--records", type=int, default=SHOWN_RECORDS)
    http.add_argument("--pid", type=int)
    argv = list(sys.argv[1:] if argv is None else argv)
    # Everything after `--` is the command, whatever options it has.
    command = argv[argv.index("--") + 1:] if "--" in argv else []
    argv = argv[: argv.index("--")] if "--" in argv else argv
    args = parser.parse_args(argv)
    if args.action == "run":
        if not command:
            parser.error("run needs a command after --")
        args.command = command
        return run_command(args)
    return measure_http(args)


if __name__ == "__main__":
    sys.exit(main())
