// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure library domain: circulation, holds, fines and calendars. No I/O: time, rules and calendars are passed in.

#![forbid(unsafe_code)]

pub mod account;
pub mod calendar;
pub mod call_number;
pub mod catalog;
pub mod checkin;
pub mod checkout;
pub mod circulation;
pub mod circulation_rules;
pub mod due_date;
pub mod fine;
pub mod hold;
pub mod inventory;
pub mod labels;
pub mod long_overdue;
pub mod notice;
pub mod notice_text;
pub mod patron;
pub mod renewal;
pub mod statement;
pub mod suspension;
