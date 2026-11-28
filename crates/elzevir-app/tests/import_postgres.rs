// SPDX-License-Identifier: GPL-3.0-or-later
//! Structural inventory of `PostgreSQL` COPY without an institution or writer.

use elzevir_app::import::postgres::inventory;

#[test]
fn pg_dump_10_fixture_reconciles_both_tables() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples-import-tool/BKP_BIBLIOTECA (2).backup");
    let report = inventory(std::io::BufReader::new(std::fs::File::open(path).unwrap())).unwrap();
    assert_eq!(report.len(), 2);
    assert_eq!(report["public.acervo"].rows, 11561);
    assert_eq!(report["public.ocorrencias"].rows, 5);
    assert_eq!(
        (report["public.acervo"].nulls, report["public.acervo"].empty),
        (196_081, 13_319)
    );
    assert_eq!(
        (
            report["public.ocorrencias"].nulls,
            report["public.ocorrencias"].empty
        ),
        (0, 6)
    );
}

#[test]
fn copy_counts_lexical_cells_without_disclosing_them() {
    let sql = b"CREATE TABLE empty_table (x text);\nCOPY public.x (a,b) FROM stdin;\r\n\\N\t\r\n\\\\N\tSELECT secret;\r\n\\.\r\n";
    let report = inventory(&sql[..]).unwrap();
    let x = &report["public.x"];
    assert_eq!(x.columns, ["a", "b"]);
    assert_eq!((x.rows, x.nulls, x.empty), (2, 1, 1));
    assert_eq!(report["empty_table"].rows, 0);
    assert!(!serde_json::to_string(&report).unwrap().contains("secret"));
}

#[test]
fn incomplete_data_and_unsupported_commands_return_no_inventory() {
    for sql in [
        "COPY t (a,a) FROM stdin;\n",
        "CREATE TABLE t (id) AS SELECT generate_series(1,10);\n",
        "SET client_encoding = 'SJIS';\nCREATE TABLE t (id int);\n",
        "SET search_path = other;\nCREATE TABLE x (id int);\n",
        "CREATE TABLE t (id int);\nALTER TABLE t RENAME TO q;\n",
        "COMMENT ON EXTENSION plpgsql IS 'private;\nCOPY t (id) FROM stdin;\n",
        "COPY t (v) FROM PROGRAM 'other';\n",
        "COPY t (v) FROM 'file';\n",
        "COPY t (v) FROM stdin WITH CSV;\n",
        "COPY t (v) FROM stdin;\nprivate\n",
        "COPY t (v) FROM stdin;\nprivate\textra\n\\.\n",
        "COPY t (v) FROM stdin;\nprivate\\\n\\.\n",
        "COPY t (v) FROM stdin;\n\\.\nDROP TABLE t;\n",
    ] {
        let error = inventory(sql.as_bytes()).unwrap_err().to_string();
        assert!(!error.contains("private"));
    }
}

#[test]
fn repeated_blocks_are_aggregated_only_when_their_layouts_agree() {
    let block = "CoPy \"Branch\".t (a, \"B\") FrOm StDiN;\n1\t2\n\\.\n";
    let sql = block.repeat(2);
    let report = inventory(sql.as_bytes()).unwrap();
    assert_eq!(report["Branch.t"].columns, ["a", "B"]);
    assert_eq!(report["Branch.t"].rows, 2);
    assert!(
        inventory(format!("{block}{}", block.replace("a, \"B\"", "\"B\", a")).as_bytes()).is_err()
    );
}

#[test]
fn interrupted_read_discards_completed_tables_and_partial_block() {
    use std::io::{self, BufReader, Read};
    struct InterruptedUpload;
    impl Read for InterruptedUpload {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::ConnectionAborted.into())
        }
    }
    let prefix = b"COPY t (v) FROM stdin;\n1\n\\.\nCOPY q (v) FROM stdin;\n2\n";
    let error = inventory(BufReader::new(prefix.as_slice().chain(InterruptedUpload))).unwrap_err();
    assert!(error.to_string().contains("input interrupted"));
}

#[test]
fn oversize_fields_lines_and_statements_refuse_the_whole_source() {
    for (start, size) in [
        ("COPY t (v) FROM stdin;\n", 1024 * 1024 + 1),
        ("--", 4 * 1024 * 1024),
        ("CREATE TABLE ", 65536),
    ] {
        let sql = format!("{start}{}\n\\.\n", "v".repeat(size));
        assert!(
            inventory(sql.as_bytes())
                .unwrap_err()
                .to_string()
                .contains("limit")
        );
    }
}
