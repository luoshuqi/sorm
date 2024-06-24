use futures_util::stream::empty;
use sorm::query::{test, Query, Update};
use sorm::sqlx::database::HasStatement;
use sorm::sqlx::{Database, Describe, Either, Error, Execute, Executor};
use sorm::{clause, Param};
use sqlx::FromRow;
use std::io;
use std::sync::Mutex;

macro_rules! params {
    ($($args:expr),+) => {
        &[$(Param::to_string($args)),*] as &[String]
    };
}

#[derive(Debug)]
struct DummyDB;

impl<'c> Executor<'c> for DummyDB {
    type Database = sorm::Database;

    fn fetch_many<'e, 'q: 'e, E: 'q>(
        self,
        _query: E,
    ) -> futures_core::stream::BoxStream<
        'e,
        Result<
            Either<<Self::Database as Database>::QueryResult, <Self::Database as Database>::Row>,
            Error,
        >,
    >
    where
        'c: 'e,
        E: Execute<'q, Self::Database>,
    {
        Box::pin(empty())
    }

    fn fetch_optional<'e, 'q: 'e, E: 'q>(
        self,
        _query: E,
    ) -> futures_core::future::BoxFuture<'e, Result<Option<<Self::Database as Database>::Row>, Error>>
    where
        'c: 'e,
        E: Execute<'q, Self::Database>,
    {
        Box::pin(async { Ok(None) })
    }

    fn prepare_with<'e, 'q: 'e>(
        self,
        _sql: &'q str,
        _parameters: &'e [<Self::Database as Database>::TypeInfo],
    ) -> futures_core::future::BoxFuture<
        'e,
        Result<<Self::Database as HasStatement<'q>>::Statement, Error>,
    >
    where
        'c: 'e,
    {
        Box::pin(async { Err(Error::Io(io::Error::other("dummy"))) })
    }

    fn describe<'e, 'q: 'e>(
        self,
        _sql: &'q str,
    ) -> futures_core::future::BoxFuture<'e, Result<Describe<Self::Database>, Error>>
    where
        'c: 'e,
    {
        Box::pin(async { Err(Error::Io(io::Error::other("dummy"))) })
    }
}

#[allow(dead_code)]
#[derive(FromRow)]
struct User {
    id: i64,
    name: String,
}

static LOCK: Mutex<()> = Mutex::new(());

#[sqlx::test]
async fn test_find() {
    let _guard = LOCK.lock().unwrap();
    let _ = Query::table("users").find::<User>(DummyDB).await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "SELECT * FROM \"users\" LIMIT 1");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT * FROM `users` LIMIT 1");
}

#[sqlx::test]
async fn test_select() {
    let _guard = LOCK.lock().unwrap();

    let _ = Query::table("users")
        .select(&["id", "name"])
        .get::<User>(DummyDB)
        .await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "SELECT \"id\",\"name\" FROM \"users\"");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT `id`,`name` FROM `users`");

    let _ = Query::table("users")
        .select_raw("MAX(id)")
        .value::<i64>(DummyDB)
        .await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "SELECT MAX(id) FROM \"users\" LIMIT 1");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT MAX(id) FROM `users` LIMIT 1");
}

#[sqlx::test]
async fn test_where() {
    let _guard = LOCK.lock().unwrap();

    let id = 1;
    let name = "foo";
    let _ = Query::table("users")
        .r#where(clause!("id={id}"))
        .r#where(clause!("name={&name}"))
        .get::<User>(DummyDB)
        .await;
    let query = test::QUERY.take();
    assert_eq!(query.len(), 1);
    #[cfg(feature = "postgres")]
    assert_eq!(
        query[0].0,
        "SELECT * FROM \"users\" WHERE id=$1 AND name=$2"
    );
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT * FROM `users` WHERE id=? AND name=?");
    assert_eq!(&query[0].1, params![&1, &"foo"]);

    let _ = Query::table("users")
        .r#where(clause!("id={id}"))
        .or_where(clause!("name={&name}"))
        .get::<User>(DummyDB)
        .await;
    let query = test::QUERY.take();
    assert_eq!(query.len(), 1);
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "SELECT * FROM \"users\" WHERE id=$1 OR name=$2");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT * FROM `users` WHERE id=? OR name=?");
    assert_eq!(&query[0].1, params![&1, &"foo"]);
}

#[sqlx::test]
async fn test_group_by() {
    let _guard = LOCK.lock().unwrap();

    let _ = Query::table("users")
        .group_by(&["id"])
        .get::<User>(DummyDB)
        .await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "SELECT * FROM \"users\" GROUP BY \"id\"");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT * FROM `users` GROUP BY `id`");

    let _ = Query::table("users")
        .group_by_raw("id,name")
        .get::<User>(DummyDB)
        .await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "SELECT * FROM \"users\" GROUP BY id,name");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT * FROM `users` GROUP BY id,name");

    let id = 1;
    let name = "foo";
    let status = 3;
    let _ = Query::table("users")
        .r#where(clause!("status={status}"))
        .group_by(&["id"])
        .having(clause!("id={id}"))
        .having(clause!("name={&name}"))
        .or_having(clause!("id=2"))
        .get::<User>(DummyDB)
        .await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(
        query[0].0,
        "SELECT * FROM \"users\" WHERE status=$1 GROUP BY \"id\" HAVING id=$2 AND name=$3 OR id=2"
    );
    #[cfg(not(feature = "postgres"))]
    assert_eq!(
        query[0].0,
        "SELECT * FROM `users` WHERE status=? GROUP BY `id` HAVING id=? AND name=? OR id=2"
    );
    assert_eq!(&query[0].1, params![&3, &1, &"foo"]);
}

#[sqlx::test]
async fn test_order_by() {
    let _guard = LOCK.lock().unwrap();

    let _ = Query::table("users")
        .order_by("id")
        .order_by_desc("name")
        .get::<User>(DummyDB)
        .await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(
        query[0].0,
        "SELECT * FROM \"users\" ORDER BY \"id\",\"name\" DESC"
    );
    #[cfg(not(feature = "postgres"))]
    assert_eq!(
        query[0].0,
        "SELECT * FROM `users` ORDER BY `id`,`name` DESC"
    );
}

#[sqlx::test]
async fn test_limit() {
    let _guard = LOCK.lock().unwrap();

    let _ = Query::table("users").offset(10).get::<User>(DummyDB).await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "SELECT * FROM \"users\" OFFSET 10");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT * FROM `users` OFFSET 10");

    let _ = Query::table("users").limit(10).get::<User>(DummyDB).await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "SELECT * FROM \"users\" LIMIT 10");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT * FROM `users` LIMIT 10");

    let _ = Query::table("users")
        .offset(10)
        .limit(20)
        .get::<User>(DummyDB)
        .await;
    let query = test::QUERY.take();
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "SELECT * FROM \"users\" LIMIT 20 OFFSET 10");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "SELECT * FROM `users` LIMIT 20 OFFSET 10");
}

#[sqlx::test]
async fn test_update() {
    let _guard = LOCK.lock().unwrap();

    let id = 1;
    let name = "foo";
    let status = 2;
    let _ = Query::table("users")
        .r#where(clause!("id={id}"))
        .update(DummyDB, clause!("name={&name},status={status}"))
        .await;
    let query = test::QUERY.take();
    assert_eq!(query.len(), 1);
    #[cfg(feature = "postgres")]
    assert_eq!(
        query[0].0,
        "UPDATE \"users\" SET name=$1,status=$2 WHERE id=$3"
    );
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "UPDATE `users` SET name=?,status=? WHERE id=?");
    assert_eq!(&query[0].1, params![&"foo", &2, &1]);

    let _ = Query::table("users")
        .r#where(clause!("id={id}"))
        .update(
            DummyDB,
            &Update::new()
                .set("name", &name)
                .set_raw("updated_at", "NOW()"),
        )
        .await;
    let query = test::QUERY.take();
    assert_eq!(query.len(), 1);
    #[cfg(feature = "postgres")]
    assert_eq!(
        query[0].0,
        "UPDATE \"users\" SET \"name\"=$1,\"updated_at\"=NOW() WHERE id=$2"
    );
    #[cfg(not(feature = "postgres"))]
    assert_eq!(
        query[0].0,
        "UPDATE `users` SET `name`=?,`updated_at`=NOW() WHERE id=?"
    );
    assert_eq!(&query[0].1, params![&"foo", &1]);
}

#[sqlx::test]
async fn test_delete() {
    let _guard = LOCK.lock().unwrap();

    let id = 1;
    let _ = Query::table("users")
        .r#where(clause!("id={id}"))
        .delete(DummyDB)
        .await;
    let query = test::QUERY.take();
    assert_eq!(query.len(), 1);
    #[cfg(feature = "postgres")]
    assert_eq!(query[0].0, "DELETE FROM \"users\" WHERE id=$1");
    #[cfg(not(feature = "postgres"))]
    assert_eq!(query[0].0, "DELETE FROM `users` WHERE id=?");
    assert_eq!(&query[0].1, params![&1]);
}
