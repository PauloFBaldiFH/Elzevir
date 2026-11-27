# Performance on the library's copy (v0)

> Status: v0, task 6.5. Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md), Phase 6, task 6.5; the goals of [doc 01](../../arquitetura/01-avaliacao-da-arquitetura.md), §4.5; [the switch](troca-do-koha.md); [backup](../operacao/backup.md). Task criterion: **numbers measured and recorded, with a goal for what is slow**.

The synthetic bench (`crates/elzevir/benches/opac.rs`) measures a catalog made by repeating the staff export. This page measures the real copy of the library on the oracle VM, with the commands a library runs: the switch, the check, a MARC import, the search, backup and restore.

## 1. Behaviour

### The tool

1. `tools/koha-parity/perf.py run LABEL [--log FILE] -- COMMAND ...` runs a command and prints one line: the wall time, the peak resident memory of the command and its exit status. The command's output goes to `--log` or nowhere, so no record and no patron reaches the report.
2. `tools/koha-parity/perf.py http BASE QUERIES.json [--repeat N] [--records N] [--pid PID]` asks a running server for:
   - each query of the list, `N` times, through the OPAC's `/search` and through the API's `/api/v1/search`;
   - then the record pages of the Instances those searches found.

   A first pass warms the caches and is not counted. The one exception is the record page, whose first opening is printed on its own line, because a page is kept rendered once it has been opened ([search and record](../opac/busca-e-registro.md), item 9). The report gives the median, the 95th percentile and the slowest answer, in milliseconds. With `--pid`, it also gives the server's resident memory before and after.
3. Only counts, times and memory are printed. The queries are words chosen by hand, with no patron data.

### How it is run

4. On the oracle VM, with the release build of the commit under test:
   - `mysqldump --single-transaction --databases` of the copy;
   - `elzevir init` in a new folder;
   - `switch-from-koha` and `verify-koha`, each through `perf.py run`;
   - `serve`, then `perf.py http`.

   Then:
   - `export`, `init` of a second folder, `import` of the exported file, and `serve` until the index stops growing;
   - `backup now`, `backup key`, `backup verify` and `backup restore` of the first folder.

   The server is stopped before each command that needs the folder to itself. In the end, the folders, the dump and the exported file are deleted.

## 2. Results on the oracle (2026-11-27)

The VM has an AMD Ryzen 5 5500 (12 threads), 28 GB of memory and a local disk. The copy has 18,919 records and 30,034 Items. It has one patron and no open loans or holds, because the oracle is sanitized. Koha runs on the same machine and was idle.

| Operation | Measured | Peak memory | Goal (doc 01, §4.5, for 1 million records) |
|---|---|---|---|
| `switch-from-koha`, whole | 39.6 s | 239 MiB | — |
| reading the dump (150 MB) | 6.8 s | | — |
| catalog (18,919 records, 30,034 Items) | 22.0 s, 860 records/s | | — |
| search index, built whole | 10.6 s, 1,785 records/s | | < 15 min (about 9 min at this rate) |
| `verify-koha` | 49.8 s | 186 MiB | — |
| `export` (MARC21, 14 MB) | 12.1 s | 49 MiB | — |
| `import` of that file | 19.7 s, 960 records/s, plus about 13 s for the index | 87 MiB | > 2,000 records/s with the index |
| `backup now` (38 MB encrypted) | 6.1 s | 36 MiB | — |
| `backup verify` | 2.9 s | 26 MiB | — |
| `backup restore`, index rebuilt | 16.1 s | 257 MiB | — |
| OPAC search, 20 queries × 20 | median 3.7 ms, p95 7.5 ms | | p95 < 50 ms |
| API search, the same | median 3.6 ms, p95 7.4 ms | | — |
| OPAC record, first opening (346 pages) | median 1.6 ms, p95 2.1 ms | | p95 < 30 ms |
| OPAC record, opened again | median 1.0 ms, p95 1.1 ms | | p95 < 30 ms |
| server memory at rest | 57 MiB, 106 MiB after the searches | | < 200 MB |

For comparison, Koha's OPAC on the same machine answered the same 20 queries, 3 times each after one warming pass, in a median of 1,375 ms (p95 2,214 ms).

## 3. What is slow, and the goal

- **MARC import: 960 records/s without the index, against the goal of 2,000 records/s with it.** The switch's catalog step is in the same range (860 records/s, matching included). For this library the whole switch takes under a minute, so this does not block the switch. It does fall short of doc 01's goal, and at this rate a catalog of a million records would take about 25 minutes to import. **Goal:** 2,000 records/s with indexing on this machine, measured with this tool. It goes on the list of gaps (6.6) with a `criterion` bench of the import, before any optimization (CLAUDE.md, "Desempenho").
- Everything else is inside the goals.
  - The search is two orders of magnitude faster than Koha's on the same machine.
  - The full index at this rate would take about 9 minutes for a million records.
  - Memory at rest is about half the goal.

## 4. Edge cases

1. A command that fails is still timed, and its exit status is printed; the run goes on, so one failure does not hide the other numbers.
2. A search that finds nothing still counts its answer time. A record page is opened only for the Instances the searches found, so a hidden Instance is never asked for.
3. The numbers depend on the machine, so the tool asserts nothing: a later run on other hardware is written down with that machine's description.

## 5. Out of this version

- Circulation at the desk (checkout and checkin p95 < 25 ms): the copy has a single sanitized patron. The synthetic bench and the desk day cover the paths. Measuring them with real volume waits for a copy with patrons, on Paulo's machine.
- Load with many clients at once (`oha` or `k6`) and the nightly run in CI (doc 02, §6).
