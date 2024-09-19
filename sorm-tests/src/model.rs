use std::any::TypeId;

use serde_json::{from_str, to_string};

use sorm::model::{HasNoPrimaryKey, Model};
use sorm::{sorm, Param};

fn same_type<T: 'static, U: 'static>() -> bool {
    TypeId::of::<T>() == TypeId::of::<U>()
}

fn param_equal(a: &dyn Param, b: &dyn Param) -> bool {
    a.to_string() == b.to_string()
}

#[test]
fn test_table() {
    {
        #[sorm]
        struct CamelCase {
            id: i64,
        }
        assert_eq!(CamelCase::TABLE, "camel_case");
    }

    {
        #[sorm(table = "users")]
        struct User {
            id: i64,
        }
        assert_eq!(User::TABLE, "users");
    }
}

#[test]
fn test_primary_key() {
    {
        #[sorm]
        struct User {
            id: i64,
        }
        assert_eq!(User::PRIMARY_KEY, "");
        assert!(same_type::<<User as Model>::PrimaryKey, HasNoPrimaryKey>());
    }

    {
        #[sorm]
        struct User {
            #[sorm(primary_key)]
            id: i64,
        }
        assert_eq!(User::PRIMARY_KEY, "id");
        assert!(same_type::<<User as Model>::PrimaryKey, i64>());
        assert!(!User::INCREMENT);
    }

    {
        #[sorm]
        struct User {
            #[sorm(primary_key(increment))]
            id: i64,
        }
        assert!(User::INCREMENT);
        let mut user = User::new();
        user.set_id(1);
        assert_eq!(user.primary_key().unwrap(), &1);

        user.set_increment_id(2);
        assert_eq!(user.id().unwrap(), &2);
    }
}

#[test]
fn test_columns() {
    #[sorm]
    struct User {
        id: i64,
        name: String,
    }

    assert_eq!(User::ID, "id");
    assert_eq!(User::NAME, "name");
    assert_eq!(User::COLUMNS, &["id", "name"]);
}

#[sorm]
struct User {
    id: i64,
    name: String,
    enable: i8,
}

#[test]
fn test_collected() {
    let mut user = User::new();
    assert!(user.collect_filled().is_empty());
    user.set_name("foo".to_string());
    user.set_enable(1);
    let filled = user.collect_filled();
    assert_eq!(filled.len(), 2);
    assert_eq!(filled[0].0, "name");
    assert_eq!(filled[1].0, "enable");
    assert!(param_equal(filled[0].1, &"foo"));
    assert!(param_equal(filled[1].1, &1));

    let changed = user.collect_changed();
    assert_eq!(changed.len(), 2);
    assert_eq!(changed[0].0, "name");
    assert_eq!(changed[1].0, "enable");
    assert!(param_equal(changed[0].1, &"foo"));
    assert!(param_equal(changed[1].1, &1));

    user.flush();
    assert_eq!(user.collect_changed().len(), 0);
    user.set_enable(2);
    let changed = user.collect_changed();
    assert_eq!(user.collect_changed().len(), 1);
    assert_eq!(changed[0].0, "enable");
    assert!(param_equal(changed[0].1, &2));
}

#[test]
fn test_serialize() {
    #[sorm(serialize)]
    struct User {
        id: i64,
        name: String,
        enable: i8,
    }
    let mut user = User::new();
    user.set_name("foo".to_string());
    assert_eq!(to_string(&user).unwrap(), r#"{"name":"foo"}"#)
}

#[test]
fn test_deserialize() {
    #[sorm(deserialize)]
    struct User {
        id: i64,
        name: String,
        enable: i8,
    }
    let user: User = from_str(r#"{"name":"foo"}"#).unwrap();
    let filled = user.collect_filled();
    assert_eq!(filled.len(), 1);
    assert_eq!(filled[0].0, "name");
    assert!(param_equal(filled[0].1, &"foo"));
}
