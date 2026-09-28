use pgdog_config::{ReadWriteSplit, Role};

use crate::{config::config, frontend::Command};

use super::setup::*;

/// With `prefer_primary` enabled, a plain read routes to the primary.
#[test]
fn test_prefer_primary_routes_read_to_primary() {
    let mut test = QueryParserTest::new().with_rw_split(ReadWriteSplit::PreferPrimary);

    let command = test.execute(vec![Query::new("SELECT * FROM users").into()]);

    assert!(command.route().is_write());
}

/// Baseline: without `prefer_primary`, a plain read still routes to a replica.
#[test]
fn test_prefer_primary_disabled_routes_read_to_replica() {
    let mut test = QueryParserTest::new().with_rw_split(ReadWriteSplit::default());

    let command = test.execute(vec![Query::new("SELECT * FROM users").into()]);

    assert!(command.route().is_read());
}

/// An explicit `pgdog.role = replica` parameter opts a read back onto replicas.
#[test]
fn test_prefer_primary_param_replica_overrides() {
    let mut test = QueryParserTest::new()
        .with_rw_split(ReadWriteSplit::PreferPrimary)
        .with_param("pgdog.role", "replica");

    let command = test.execute(vec![Query::new("SELECT * FROM users").into()]);

    assert!(command.route().is_read());
}

/// A `/* pgdog_role: replica */` query comment opts a read back onto replicas.
#[test]
fn test_prefer_primary_comment_replica_overrides() {
    let mut test = QueryParserTest::new().with_rw_split(ReadWriteSplit::PreferPrimary);

    let command = test.execute(vec![
        Query::new("/* pgdog_role: replica */ SELECT * FROM users").into(),
    ]);

    assert!(command.route().is_read());
}

/// Writes are unaffected: they still route to the primary.
#[test]
fn test_prefer_primary_write_routes_to_primary() {
    let mut test = QueryParserTest::new().with_rw_split(ReadWriteSplit::PreferPrimary);

    let command = test.execute(vec![Query::new("INSERT INTO users (id) VALUES (1)").into()]);

    assert!(command.route().is_write());
}

/// Statements the parser routes as reads without consulting `write_override`.
///
/// In session mode, the first statement's route picks the backend for the whole
/// session, so any of these arriving first (e.g. a pool health check sending an
/// empty query) used to pin the session, and all its writes, to a replica.
const IMPLICIT_READS: &[&str] = &[
    "",
    ";",
    "-- ping",
    "PREPARE p AS SELECT 1",
    "COPY users TO STDOUT",
];

fn session_mode(rw_split: ReadWriteSplit) -> QueryParserTest {
    QueryParserTest::new_session_mode(&config()).with_rw_split(rw_split)
}

/// With `prefer_primary`, every one of those statements routes to the primary.
#[test]
fn test_prefer_primary_implicit_reads_route_to_primary() {
    for query in IMPLICIT_READS {
        let mut test = session_mode(ReadWriteSplit::PreferPrimary);
        let command = test.execute(vec![Query::new(*query).into()]);
        assert!(
            command.route().is_write(),
            "{query:?} should route to the primary with prefer_primary"
        );
    }
}

/// Baseline: without `prefer_primary`, those statements are still reads.
#[test]
fn test_implicit_reads_route_to_replica_without_prefer_primary() {
    for query in IMPLICIT_READS {
        let mut test = session_mode(ReadWriteSplit::IncludePrimary);
        let command = test.execute(vec![Query::new(*query).into()]);
        assert!(command.route().is_read(), "{query:?} should be a read");
    }
}

/// `SET pgdog.role TO 'replica'` still opts implicit reads onto replicas.
#[test]
fn test_prefer_primary_implicit_reads_param_replica_overrides() {
    for query in IMPLICIT_READS {
        let mut test =
            session_mode(ReadWriteSplit::PreferPrimary).with_param("pgdog.role", "replica");
        let command = test.execute(vec![Query::new(*query).into()]);
        assert!(command.route().is_read(), "{query:?} should be a read");
    }
}

/// `pgdog.role=replica` as a startup parameter still opts implicit reads onto replicas.
#[test]
fn test_prefer_primary_implicit_reads_sticky_replica_overrides() {
    for query in IMPLICIT_READS {
        let mut test = session_mode(ReadWriteSplit::PreferPrimary).with_sticky_role(Role::Replica);
        let command = test.execute(vec![Query::new(*query).into()]);
        assert!(command.route().is_read(), "{query:?} should be a read");
    }
}

/// A `/* pgdog_role: replica */` comment still opts implicit reads onto replicas.
#[test]
fn test_prefer_primary_implicit_reads_comment_replica_overrides() {
    for query in ["PREPARE p AS SELECT 1", "COPY users TO STDOUT"] {
        let mut test = session_mode(ReadWriteSplit::PreferPrimary);
        let command = test.execute(vec![
            Query::new(format!("/* pgdog_role: replica */ {query}")).into(),
        ]);
        assert!(command.route().is_read(), "{query:?} should be a read");
    }
}

/// `SET pgdog.role TO 'primary'` combined with a replica comment: the comment wins,
/// matching how comments override the parameter for plain `SELECT`s.
#[test]
fn test_prefer_primary_comment_replica_beats_param_primary() {
    let mut test = session_mode(ReadWriteSplit::PreferPrimary).with_param("pgdog.role", "primary");
    let command = test.execute(vec![
        Query::new("/* pgdog_role: replica */ SELECT * FROM users").into(),
    ]);
    assert!(command.route().is_read());
}

const READ_ONLY_BEGINS: &[&str] = &[
    "BEGIN READ ONLY",
    "START TRANSACTION READ ONLY",
    "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY",
];

/// `BEGIN READ ONLY` carries a read route; with `prefer_primary` it goes to the primary.
#[test]
fn test_prefer_primary_read_only_begin_routes_to_primary() {
    for query in READ_ONLY_BEGINS {
        let mut test = session_mode(ReadWriteSplit::PreferPrimary);
        let command = test.execute(vec![Query::new(*query).into()]);
        assert!(
            matches!(command, Command::StartTransaction { .. }),
            "{query:?} should start a transaction"
        );
        assert!(
            command.route().is_write(),
            "{query:?} should route to the primary with prefer_primary"
        );
    }
}

/// Baseline: without `prefer_primary`, `BEGIN READ ONLY` is a read.
#[test]
fn test_read_only_begin_routes_to_replica_without_prefer_primary() {
    for query in READ_ONLY_BEGINS {
        let mut test = session_mode(ReadWriteSplit::IncludePrimary);
        let command = test.execute(vec![Query::new(*query).into()]);
        assert!(command.route().is_read(), "{query:?} should be a read");
    }
}

/// `SET pgdog.role TO 'replica'` still opts `BEGIN READ ONLY` onto replicas.
#[test]
fn test_prefer_primary_read_only_begin_param_replica_overrides() {
    for query in READ_ONLY_BEGINS {
        let mut test =
            session_mode(ReadWriteSplit::PreferPrimary).with_param("pgdog.role", "replica");
        let command = test.execute(vec![Query::new(*query).into()]);
        assert!(command.route().is_read(), "{query:?} should be a read");
    }
}
