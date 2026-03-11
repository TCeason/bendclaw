use anyhow::Result;
use bendclaw::storage::sql::col_i64;
use bendclaw::storage::versioned::delete_versioned;
use bendclaw::storage::versioned::gen_id;
use bendclaw::storage::versioned::insert_versioned;
use bendclaw::storage::versioned::update_versioned;

use crate::common::setup::pool;
use crate::common::setup::uid;

// ── gen_id ──

#[test]
fn gen_id_returns_nonempty_string() {
    let id = gen_id();
    assert!(!id.is_empty());
}

#[test]
fn gen_id_returns_unique_values() {
    let a = gen_id();
    let b = gen_id();
    assert_ne!(a, b);
}
