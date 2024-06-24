#![doc = include_str!("../README.md")]

/// Generates a clause.
///
/// # Examples
///
/// ```rust
/// # use sorm_macros::clause;
/// let name = "foo";
/// let size = 100;
/// let status = &[1, 2, 3];
/// clause!("name={&name} AND size>{size} AND status IN ({#status})");
/// // expr() => name=? AND size>? AND status IN (?,?,?)
/// // params() => &[&"foo", &100, &1, &2, &3]
/// ```
///
/// Parameters can be any type that implements [Param].
/// The `#` before the parameter indicates that it's a vec or slice of parameters.
///
/// If a second argument is passed to clause!, it will be used to bind the params value,
/// which is necessary in certain scenarios.
///
/// ```rust
/// # use sorm::clause;
/// # use sorm::query::Query;
/// fn search(name: Option<&str>) {
///    let mut query = Query::table("user");
///    let params;
///    if let Some(ref name) = name {
///        // Compilation error without the `params`: temporary value dropped while borrowed.
///        query.r#where(clause!("name={name}", params));
///    }
///    query.select(&["id"]);
///    // ...
/// }
///
/// ```
///
/// Similarly, the third argument is used to bind the expr value.
///
/// ```rust
/// # use sorm::query::Query;
/// # use sorm_macros::clause;
/// fn search(names: Vec<&str>) {
///    let mut query = Query::table("user");
///    let params;
///    let expr;
///    if !names.is_empty() {
///        query.r#where(clause!("name IN ({#names})", params, expr));
///    }
///    query.select(&["id"]);
///    // ...
/// }
/// ```
pub use sorm_macros::clause;
/// Generates model implementations.
///
/// This macro generates implementations for [`crate::model::Model`].
///
/// # Examples
///
/// ```rust
/// # use sorm_macros::sorm;
/// #[sorm]
/// struct User {
///     id: i64,
///     name: String,
/// }
/// assert_eq!(User::TABLE, "user");
/// ```
///
/// Specifies a custom table for the model:
///
/// ```rust
/// # use sorm_macros::sorm;
/// #[sorm(table = "users")]
/// struct User {
///     id: i64,
///     name: String,
/// }
/// assert_eq!(User::TABLE, "users");
/// ```
///
/// Specifies the primary key...
///
/// ```rust
/// # use sorm_macros::sorm;
/// #[sorm]
/// struct User {
///     #[sorm(primary_key)]
///     id: i64,
///     name: String,
/// }
/// assert_eq!(User::PRIMARY_KEY, "id");
/// assert_eq!(User::INCREMENT, false);
/// ```
///
/// ...with auto incrementing:
///
/// ```rust
/// # use sorm_macros::sorm;
/// #[sorm]
/// struct User {
///     #[sorm(primary_key(increment))]
///     id: i64,
///     name: String,
/// }
/// assert_eq!(User::INCREMENT, true);
/// ```
///
/// Specifies default value:
///
/// ```rust
/// # use sorm_macros::sorm;
/// #[sorm]
/// struct User {
///     #[sorm(primary_key(increment))]
///     id: i64,
///     #[sorm(default)]
///     name: String,
///     #[sorm(default = "1")]
///     gender: i8,
/// }
/// let mut user = User::new();
/// user.fill_create_default();
/// assert_eq!(user.name().unwrap(), "");
/// assert_eq!(user.gender().unwrap(), &1);
/// ```
///
/// Timestamps:
///
/// ```rust
/// # use sorm_macros::sorm;
/// fn timestamp() -> i64 {
///    1
///}
///
/// #[sorm]
/// struct User {
///    #[sorm(primary_key(increment))]
///    id: i64,
///    name: String,
///    #[sorm(update_time = "timestamp()")] // #[sorm(update_time)] equals #[sorm(update_time = "crate::current_timestamp()")]
///    updated_at: i64,
///    #[sorm(create_time = "timestamp()")] // #[sorm(create_time)] equals #[sorm(create_time = "crate::current_timestamp()")]
///    created_at: i64
///}
/// let mut user = User::new();
/// user.fill_create_default();
/// assert_eq!(user.updated_at().unwrap(), &1);
/// assert_eq!(user.created_at().unwrap(), &1);
/// ```
///
/// Serialize and deserialize:
///
/// ```rust
/// # use sorm_macros::sorm;
/// #[sorm(serialize, deserialize)]
/// struct User {
///     id: i64
/// }
/// ```
///
pub use sorm_macros::sorm;
pub use sqlx;

pub use error::{Error, Result};
#[cfg(feature = "mysql")]
pub use mysql::*;
#[cfg(feature = "postgres")]
pub use postgres::*;
#[cfg(feature = "sqlite")]
pub use sqlite::*;

mod error;
pub mod model;
pub mod query;

#[cfg(all(
    not(feature = "sqlite"),
    not(feature = "mysql"),
    not(feature = "postgres")
))]
compile_error!("either the sqlite or mysql or postgres feature must be enabled)");

#[cfg(feature = "sqlite")]
mod sqlite {
    #[doc(hidden)]
    pub use sqlx::sqlite::SqliteArguments as Arguments;
    #[doc(hidden)]
    pub use sqlx::sqlite::SqliteRow as Row;
    #[doc(hidden)]
    pub use sqlx::Sqlite as Database;
    use sqlx::{Encode, Type};

    /// Represents a sql parameter.
    pub trait Param<'q> {
        fn add(&'q self, arguments: &mut Arguments<'q>);

        #[cfg(feature = "test")]
        fn to_string(&self) -> String;
    }

    #[cfg(not(feature = "test"))]
    impl<'q, T> Param<'q> for T
    where
        T: Encode<'q, Database> + Type<Database> + Send + Sync,
    {
        fn add(&'q self, arguments: &mut Arguments<'q>) {
            use sqlx::Arguments;
            arguments.add(self);
        }
    }

    #[cfg(feature = "test")]
    impl<'q, T> Param<'q> for T
    where
        T: Encode<'q, Database> + Type<Database> + std::fmt::Debug + Send + Sync,
    {
        fn add(&'q self, arguments: &mut Arguments<'q>) {
            use sqlx::Arguments;
            arguments.add(self);
        }

        fn to_string(&self) -> String {
            format!("{:?}", self)
        }
    }
}

#[cfg(feature = "mysql")]
mod mysql {
    #[doc(hidden)]
    pub use sqlx::mysql::MySqlArguments as Arguments;
    #[doc(hidden)]
    pub use sqlx::mysql::MySqlRow as Row;
    #[doc(hidden)]
    pub use sqlx::MySql as Database;
    use sqlx::{Encode, Type};

    /// Represents a sql parameter.
    pub trait Param<'q> {
        fn add(&'q self, arguments: &mut Arguments);

        #[cfg(feature = "test")]
        fn to_string(&self) -> String;
    }

    #[cfg(not(feature = "test"))]
    impl<'q, T> Param<'q> for T
    where
        T: Encode<'q, Database> + Type<Database> + Send + Sync,
    {
        fn add(&'q self, arguments: &mut Arguments) {
            use sqlx::Arguments;
            arguments.add(self);
        }
    }

    #[cfg(feature = "test")]
    impl<'q, T> Param<'q> for T
    where
        T: Encode<'q, Database> + Type<Database> + std::fmt::Debug + Send + Sync,
    {
        fn add(&'q self, arguments: &mut Arguments) {
            use sqlx::Arguments;
            arguments.add(self);
        }

        fn to_string(&self) -> String {
            format!("{:?}", self)
        }
    }
}

#[cfg(feature = "postgres")]
mod postgres {
    #[doc(hidden)]
    pub use sqlx::postgres::PgArguments as Arguments;
    #[doc(hidden)]
    pub use sqlx::postgres::PgRow as Row;
    #[doc(hidden)]
    pub use sqlx::Postgres as Database;
    use sqlx::{Encode, Type};

    /// Represents a sql parameter.
    pub trait Param<'q> {
        fn add(&'q self, arguments: &mut Arguments);

        #[cfg(feature = "test")]
        fn to_string(&self) -> String;
    }

    #[cfg(not(feature = "test"))]
    impl<'q, T> Param<'q> for T
    where
        T: Encode<'q, Database> + Type<Database> + Send + Sync,
    {
        #[inline]
        fn add(&'q self, arguments: &mut Arguments) {
            use sqlx::Arguments;
            arguments.add(self);
        }
    }

    #[cfg(feature = "test")]
    impl<'q, T> Param<'q> for T
    where
        T: Encode<'q, Database> + Type<Database> + std::fmt::Debug + Send + Sync,
    {
        #[inline]
        fn add(&'q self, arguments: &mut Arguments) {
            use sqlx::Arguments;
            arguments.add(self);
        }

        fn to_string(&self) -> String {
            format!("{:?}", self)
        }
    }
}

/// Represents a sql clause.
pub trait Clause<'q> {
    /// Returns the SQL expression corresponding to the clause.
    fn expr(&self) -> &'q str;

    /// Returns the parameters associated with the SQL expression.
    fn params(&self) -> &'q [&'q (dyn Param<'q> + Sync)];
}

impl<'q> Clause<'q> for &'q str {
    #[inline]
    fn expr(&self) -> &'q str {
        *self
    }

    #[inline]
    fn params(&self) -> &'q [&'q (dyn Param<'q> + Sync)] {
        &[]
    }
}

impl<'q> Clause<'q> for (&'q str, &'q [&'q (dyn Param<'q> + Sync)]) {
    #[inline]
    fn expr(&self) -> &'q str {
        self.0
    }

    #[inline]
    fn params(&self) -> &'q [&'q (dyn Param<'q> + Sync)] {
        self.1
    }
}

/// A trait for borrows a reference to the implementing type itself.
pub trait Lend {
    fn lend(&self) -> &Self;
}

impl<T: ?Sized> Lend for T {
    fn lend(&self) -> &Self {
        self
    }
}

#[cfg(feature = "postgres")]
#[inline]
fn concat_ident(s: &mut String, ident: &str) {
    s.push_str("\"");
    s.push_str(ident);
    s.push_str("\"");
}

#[cfg(not(feature = "postgres"))]
#[inline]
fn concat_ident(s: &mut String, ident: &str) {
    s.push_str("`");
    s.push_str(ident);
    s.push_str("`");
}

fn concat_idents(s: &mut String, idents: &[&str]) {
    if idents.is_empty() {
        return;
    }
    for v in idents {
        concat_ident(s, v);
        s.push_str(",");
    }
    s.pop();
}
