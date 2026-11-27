// SPDX-License-Identifier: GPL-3.0-or-later
//! The four tables of a whole Koha database that the patron migration reads
//! (docs/specs/migracao/leitores-do-koha.md, item 3).

use std::collections::BTreeMap;

use crate::import::koha_sql::{DumpError, DumpProblem, MAX_DATABASE_DUMP_BYTES, Target};

/// Most patrons, Items, loans, holds or restrictions read from one dump.
pub const MAX_KOHA_ROWS: usize = 1_000_000;
/// Most rows read from each of the small tables.
const MAX_SMALL_ROWS: usize = 100_000;

/// The columns kept of `reserves`.
const RESERVES: &[&str] = &[
    "reserve_id",
    "borrowernumber",
    "reservedate",
    "biblionumber",
    "branchcode",
    "priority",
    "found",
    "itemnumber",
    "waitingdate",
    "expirationdate",
    "suspend",
    "suspend_until",
    "item_level_hold",
    "timestamp",
];

/// The columns kept of `accountlines`.
const ACCOUNT_LINES: &[&str] = &[
    "accountlines_id",
    "issue_id",
    "borrowernumber",
    "itemnumber",
    "date",
    "amount",
    "credit_type_code",
    "debit_type_code",
    "status",
    "amountoutstanding",
    "timestamp",
    "branchcode",
];

/// The columns kept of `old_issues`.
const OLD_ISSUES: &[&str] = &[
    "issue_id",
    "borrowernumber",
    "itemnumber",
    "date_due",
    "branchcode",
    "returndate",
    "checkin_library",
    "renewals_count",
    "issuedate",
    "onsite_checkout",
];

/// The columns kept of `repeatable_holidays`.
const REPEATABLE_HOLIDAYS: &[&str] = &[
    "id",
    "branchcode",
    "weekday",
    "day",
    "month",
    "title",
    "description",
];

/// The columns kept of `special_holidays`.
const SPECIAL_HOLIDAYS: &[&str] = &[
    "id",
    "branchcode",
    "day",
    "month",
    "year",
    "isexception",
    "title",
    "description",
];

/// The columns kept of `borrower_attribute_types`.
const ATTRIBUTE_TYPES: &[&str] = &[
    "code",
    "description",
    "repeatable",
    "unique_id",
    "is_date",
    "opac_display",
    "opac_editable",
    "staff_searchable",
    "searched_by_default",
    "authorised_value_category",
    "display_checkout",
    "category_code",
    "class",
    "keep_for_pseudonymization",
    "mandatory",
    "opac_mandatory",
];

/// The table a kept row came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Table {
    Branches,
    Categories,
    Borrowers,
    Debarments,
    Items,
    Issues,
    Reserves,
    AccountLines,
    AccountOffsets,
    OldIssues,
    /// `circulation_rules` (troca-do-koha.md, item 7a).
    Rules,
    /// `repeatable_holidays` (troca-do-koha.md, item 7b).
    RepeatableHolidays,
    /// `special_holidays` (troca-do-koha.md, item 7b).
    SpecialHolidays,
    /// `borrower_attribute_types` (patron-attributes.md, item 16).
    AttributeTypes,
    /// `borrower_attribute_types_branches`, only counted.
    AttributeTypeBranches,
    /// `borrower_attributes`.
    Attributes,
    /// `authorised_values`, for the options of the attribute types.
    AuthorisedValues,
    /// `currency`, for the SIP2 currency (patron-attributes.md, item 20).
    Currencies,
}

impl Table {
    /// The columns kept, lowercase as the grammar reads them.
    const fn columns(self) -> &'static [&'static str] {
        match self {
            Self::Branches => &[
                "branchcode",
                "branchname",
                "branchaddress1",
                "branchaddress2",
                "branchaddress3",
                "branchcity",
                "branchstate",
                "branchzip",
                "branchcountry",
                "branchphone",
                "branchemail",
                "branchurl",
            ],
            Self::Categories => &[
                "categorycode",
                "description",
                "enrolmentperiod",
                "enrolmentperioddate",
            ],
            Self::Borrowers => &[
                "borrowernumber",
                "cardnumber",
                "surname",
                "firstname",
                "dateofbirth",
                "email",
                "emailpro",
                "phone",
                "mobile",
                "phonepro",
                "streettype",
                "address",
                "streetnumber",
                "address2",
                "city",
                "state",
                "zipcode",
                "country",
                "branchcode",
                "categorycode",
                "dateenrolled",
                "dateexpiry",
                "borrowernotes",
                "privacy",
                "flags",
                "password",
            ],
            Self::Debarments => &["borrowernumber", "type", "expiration", "created"],
            Self::Items => &["itemnumber", "barcode"],
            Self::Issues => &[
                "issue_id",
                "borrowernumber",
                "itemnumber",
                "date_due",
                "branchcode",
                "issuedate",
                "lastreneweddate",
                "renewals_count",
                "onsite_checkout",
            ],
            Self::Reserves => RESERVES,
            Self::AccountLines => ACCOUNT_LINES,
            Self::OldIssues => OLD_ISSUES,
            Self::Rules => &[
                "id",
                "branchcode",
                "categorycode",
                "itemtype",
                "rule_name",
                "rule_value",
            ],
            Self::RepeatableHolidays => REPEATABLE_HOLIDAYS,
            Self::AttributeTypes => ATTRIBUTE_TYPES,
            Self::AttributeTypeBranches => &["bat_code"],
            Self::Attributes => &["id", "borrowernumber", "code", "attribute"],
            Self::AuthorisedValues => &["id", "category", "authorised_value", "lib"],
            Self::Currencies => &["currency", "active"],
            Self::SpecialHolidays => SPECIAL_HOLIDAYS,
            Self::AccountOffsets => &[
                "id",
                "credit_id",
                "debit_id",
                "type",
                "amount",
                "created_on",
            ],
        }
    }

    /// The column a row cannot do without.
    const fn key(self) -> &'static str {
        match self {
            Self::Branches => "branchcode",
            Self::Categories => "categorycode",
            Self::Borrowers | Self::Debarments => "borrowernumber",
            Self::Items => "itemnumber",
            Self::Issues | Self::OldIssues => "issue_id",
            Self::Reserves => "reserve_id",
            Self::AccountLines => "accountlines_id",
            Self::AttributeTypes => "code",
            Self::AttributeTypeBranches => "bat_code",
            Self::Currencies => "currency",
            Self::AccountOffsets
            | Self::Attributes
            | Self::AuthorisedValues
            | Self::Rules
            | Self::RepeatableHolidays
            | Self::SpecialHolidays => "id",
        }
    }
}

/// Columns of `borrowers` the Elzevir does not keep, counted for the report when filled
/// (item 17). Koha 26.05 `kohastructure.sql`.
pub const LEFT_OUT_COLUMNS: &[&str] = &[
    "title",
    "othernames",
    "initials",
    "pronouns",
    "preferred_name",
    "middle_name",
    "fax",
    "b_streetnumber",
    "b_streettype",
    "b_address",
    "b_address2",
    "b_city",
    "b_state",
    "b_zipcode",
    "b_country",
    "b_email",
    "b_phone",
    "contactname",
    "contactfirstname",
    "contacttitle",
    "relationship",
    "sex",
    "opacnote",
    "contactnote",
    "sort1",
    "sort2",
    "altcontactfirstname",
    "altcontactsurname",
    "altcontactaddress1",
    "altcontactaddress2",
    "altcontactaddress3",
    "altcontactstate",
    "altcontactzipcode",
    "altcontactcountry",
    "altcontactphone",
    "smsalertnumber",
    "userid",
    "gonenoaddress",
    "lost",
];

/// One row: the kept columns and their values, `None` for SQL `NULL`.
pub(super) type Row = Vec<(&'static str, Option<Vec<u8>>)>;

/// What the dump had in the four tables.
#[derive(Default)]
pub(super) struct KohaTables {
    pub(super) rows: BTreeMap<Table, Vec<Row>>,
    /// For each column of [`LEFT_OUT_COLUMNS`], how many patrons had it filled.
    pub(super) left_out: BTreeMap<&'static str, usize>,
    /// Whether the dump declares `borrowers`, even empty.
    declares_patrons: bool,
}

impl Target for KohaTables {
    type Table = Table;
    type Column = &'static str;
    const WHOLE_DATABASE: bool = true;
    const MAX_BYTES: u64 = MAX_DATABASE_DUMP_BYTES;

    fn table(&mut self, name: &str, declared: bool) -> Result<Option<Table>, DumpProblem> {
        self.declares_patrons |= declared && name == "borrowers";
        Ok(match name {
            "branches" => Some(Table::Branches),
            "categories" => Some(Table::Categories),
            "borrowers" => Some(Table::Borrowers),
            "borrower_debarments" => Some(Table::Debarments),
            "items" => Some(Table::Items),
            "issues" => Some(Table::Issues),
            "reserves" => Some(Table::Reserves),
            "accountlines" => Some(Table::AccountLines),
            "account_offsets" => Some(Table::AccountOffsets),
            "old_issues" => Some(Table::OldIssues),
            "circulation_rules" => Some(Table::Rules),
            "repeatable_holidays" => Some(Table::RepeatableHolidays),
            "special_holidays" => Some(Table::SpecialHolidays),
            "borrower_attribute_types" => Some(Table::AttributeTypes),
            "borrower_attribute_types_branches" => Some(Table::AttributeTypeBranches),
            "borrower_attributes" => Some(Table::Attributes),
            "authorised_values" => Some(Table::AuthorisedValues),
            "currency" => Some(Table::Currencies),
            _ => None,
        })
    }

    fn column(&self, table: Table, name: &str) -> Option<&'static str> {
        let left_out: &[&'static str] = if table == Table::Borrowers {
            LEFT_OUT_COLUMNS
        } else {
            &[]
        };
        table
            .columns()
            .iter()
            .chain(left_out)
            .find(|column| **column == name)
            .copied()
    }

    fn has_keys(&self, table: Table, layout: &[Option<&'static str>]) -> bool {
        layout.contains(&Some(table.key()))
    }

    fn push(&mut self, table: Table, offset: u64, cells: Row) -> Result<(), DumpError> {
        let rows = self.rows.entry(table).or_default();
        let most = if matches!(
            table,
            Table::Branches
                | Table::Categories
                | Table::Rules
                | Table::RepeatableHolidays
                | Table::AttributeTypes
                | Table::AttributeTypeBranches
                | Table::AuthorisedValues
                | Table::Currencies
        ) {
            MAX_SMALL_ROWS
        } else {
            MAX_KOHA_ROWS
        };
        if rows.len() == most {
            return Err(DumpError {
                offset,
                problem: DumpProblem::Limit("rows of one table"),
            });
        }
        let mut kept = Vec::with_capacity(cells.len());
        for (column, value) in cells {
            // Only `borrowers` has left-out columns, and `title` is kept elsewhere.
            if table == Table::Borrowers && LEFT_OUT_COLUMNS.contains(&column) {
                if value
                    .as_deref()
                    .is_some_and(|value| !matches!(value.trim_ascii(), b"" | b"0"))
                {
                    *self.left_out.entry(column).or_default() += 1;
                }
            } else {
                kept.push((column, value));
            }
        }
        rows.push(kept);
        Ok(())
    }

    fn finish(&self, _any: bool) -> Result<(), DumpProblem> {
        if self.declares_patrons {
            Ok(())
        } else {
            Err(DumpProblem::MissingPatrons)
        }
    }
}
