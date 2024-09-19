# sorm

Simple ORM (Object-Relational Mapping) built on SQLx.

## Examples

```rust
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use sorm::model::Model;
use sorm::{clause, sorm};
use sqlx::{Executor, SqlitePool};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let db = SqlitePool::connect("sqlite://:memory:").await?;
    db.execute(
        r#"
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    enable INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
)
"#,
    )
        .await?;

    // Create
    let mut user = User::new();
    user.set_name("foo".to_string());
    user.create(&db).await?;
    println!("{}", json!(user)); // => {"created_at":1719131281,"enable":1,"id":1,"name":"foo","updated_at":1719131281}

    // Update
    user.set_enable(0);
    user.update(&db).await?;
    println!("{}", json!(user)); // => {"created_at":1719131281,"enable":0,"id":1,"name":"foo","updated_at":1719131281}

    // Find by primary key
    let user = User::find(&db, &1).await?;
    println!("{}", json!(user)); // => {"created_at":1719131281,"enable":0,"id":1,"name":"foo","updated_at":1719131281}

    // Use query builder
    let name = "foo";
    let user = User::query()
        .select(&[User::ID, User::NAME])
        .r#where(clause!("name={&name}"))
        .find(&db)
        .await?;
    println!("{}", json!(user)); // => {"id":1,"name":"foo"}

    // Delete
    user.delete(&db).await?;
    User::destroy(&db, &1).await?;

    Ok(())
}

#[sorm(table = "users", serialize)]
struct User {
    #[sorm(primary_key(increment))]
    id: i64,

    name: String,

    #[sorm(default = "1")]
    enable: i8,

    #[sorm(update_time)]
    updated_at: i64,

    #[sorm(create_time)]
    created_at: i64,
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
```

## Install

- sqlite

  `cargo add sorm --features sqlite`

- postgres

  `cargo add sorm --features postgres`

- mysql

  `cargo add sorm --features mysql`