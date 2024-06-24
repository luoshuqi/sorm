//! A query builder for constructing SELECT, INSERT, UPDATE, or DELETE queries.
use std::collections::HashSet;

use log::debug;
use sqlx::{Arguments, Decode, Executor, FromRow, Row, Type};

use crate::{concat_ident, concat_idents, Clause, Database, Error, Param};

#[cfg(feature = "test")]
pub mod test {
    use crate::Param;
    use std::mem::take;
    use std::sync::Mutex;

    pub struct Query {
        query: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl Query {
        pub const fn new() -> Self {
            Self {
                query: Mutex::new(Vec::new()),
            }
        }

        pub fn add(&self, sql: &str, params: &[&(dyn Param + Sync)]) {
            self.query.lock().unwrap().push((
                sql.to_string(),
                params.to_vec().into_iter().map(|v| v.to_string()).collect(),
            ));
        }

        pub fn take(&self) -> Vec<(String, Vec<String>)> {
            take(&mut *self.query.lock().unwrap())
        }
    }

    pub static QUERY: Query = Query::new();
}

enum Select<'q> {
    Columns(&'q [&'q str]),
    Raw(&'q str),
    Omitted(HashSet<&'q str>),
    None,
}

enum OrderBy<'q> {
    Asc(&'q str),
    Desc(&'q str),
    Raw(&'q str),
}

/// Represents the fields to update.
///
/// # Examples
///
/// ```rust
/// use sorm::query::{Query, Update};
/// use sorm::{Result, Database};
/// async fn update(db: impl sqlx::Executor<'_, Database = Database>) -> Result<u64> { ///
///     Query::table("users")
///         .r#where("id=1")
///         .update(
///             db,
///             &Update::new()
///                 .set("name", &"foo")
///                 .set_raw("updated_at", "NOW()"),
///         )
///         .await
/// }
/// ```
pub struct Update<'q> {
    expr: String,
    params: Vec<&'q (dyn Param<'q> + Sync)>,
}

impl<'q> Update<'q> {
    /// Constructs a new, empty `Update`.
    #[inline]
    pub fn new() -> Self {
        Self {
            expr: String::new(),
            params: Vec::new(),
        }
    }

    /// Constructs a new, empty `Update` with at least the specified capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            expr: String::with_capacity(10 * capacity),
            params: Vec::with_capacity(capacity),
        }
    }

    /// Returns `true` if this `Update` is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.expr.is_empty()
    }

    /// Sets a column and its corresponding value for an update operation.
    pub fn set(mut self, column: &'q str, value: &'q (dyn Param<'q> + Sync)) -> Self {
        if !self.expr.is_empty() {
            self.expr.push_str(",");
        }
        concat_ident(&mut self.expr, column);
        self.expr.push_str("=?");
        self.params.push(value);
        self
    }

    /// Sets a column to a raw SQL value for an update operation.
    pub fn set_raw(mut self, column: &'q str, value: &'q str) -> Self {
        if !self.expr.is_empty() {
            self.expr.push_str(",");
        }
        concat_ident(&mut self.expr, column);
        self.expr.push_str("=");
        self.expr.push_str(value);
        self
    }
}

impl<'q> Clause<'q> for &'q Update<'q> {
    #[inline]
    fn expr(&self) -> &'q str {
        &self.expr
    }

    #[inline]
    fn params(&self) -> &'q [&'q (dyn Param<'q> + Sync)] {
        &self.params
    }
}

/// A query builder for constructing SELECT, INSERT, UPDATE, or DELETE queries.
pub struct Query<'q> {
    table: &'q str,
    columns: Option<&'q [&'q str]>,
    select: Select<'q>,
    criteria: Vec<(&'q str, &'q [&'q (dyn Param<'q> + Sync)])>,
    group_by: Select<'q>,
    having: Vec<(&'q str, &'q [&'q (dyn Param<'q> + Sync)])>,
    order_by: Vec<OrderBy<'q>>,
    offset: Option<usize>,
    limit: Option<usize>,
}

impl<'q> Query<'q> {
    pub(crate) fn new(table: &'q str, columns: Option<&'q [&'q str]>) -> Self {
        Self {
            table,
            columns,
            select: Select::None,
            criteria: Vec::new(),
            group_by: Select::None,
            having: Vec::new(),
            order_by: Vec::new(),
            offset: None,
            limit: None,
        }
    }

    /// Creates a new query builder instance for a specified table.
    ///
    /// This method initializes a new instance of the query builder for the given
    /// table name. It is typically used as the starting point for building a query.
    #[inline]
    pub fn table(table: &'q str) -> Self {
        Self::new(table, None)
    }

    /// Sets the columns to be selected in the query.
    ///
    /// This method allows you to specify the columns to be selected in the
    /// query. It can only be called once. If called multiple times, the last
    /// call will overwrite the previous one. If `select_raw` has been called
    /// previously, it will also be overwritten by this method.
    #[inline]
    pub fn select(&mut self, columns: &'q [&str]) -> &mut Self {
        self.select = Select::Columns(columns);
        self
    }

    /// Sets the SELECT clause of the query to a raw SQL expression.
    ///
    /// This method assigns a raw SQL expression to the SELECT clause of the query builder.
    /// It can only be called once. If called multiple times, the last
    /// call will overwrite the previous one. If `select` has been called
    /// previously, it will also be overwritten by this method.
    #[inline]
    pub fn select_raw(&mut self, expr: &'q str) -> &mut Self {
        self.select = Select::Raw(expr);
        self
    }

    pub(crate) fn omit(&mut self, columns: &'q [&str]) -> &mut Self {
        if columns.is_empty() {
            return self;
        }

        let mut set = HashSet::with_capacity(columns.len());
        for v in columns {
            set.insert(*v);
        }
        self.select = Select::Omitted(set);
        self
    }

    /// Adds a WHERE clause to the query builder.
    ///
    /// This method appends a WHERE clause with the "AND" operator to the existing criteria of the
    /// query builder. The WHERE clause is constructed using the provided `clause`, which must
    /// implement the [`Clause`] trait.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use sorm::query::Query;
    /// # use sorm::clause;
    /// let mut query = Query::table("users");
    /// // Use the string if no parameters are involved...
    /// query.r#where("name IS NOT NULL");
    ///
    /// // ...otherwise use the clause! macro.
    /// let name = "root";
    /// let enable = 1;
    /// query.r#where(clause!("name={&name} AND enable={enable}"));
    /// ```
    pub fn r#where(&mut self, clause: impl Clause<'q>) -> &mut Self {
        if !self.criteria.is_empty() {
            self.criteria.push((" AND ", &[]));
        }
        self.criteria.push((clause.expr(), clause.params()));
        self
    }

    /// Adds an OR WHERE clause to the query builder.
    ///
    /// See [`Query:: where `] for the usage of the `clause` argument.
    pub fn or_where(&mut self, clause: impl Clause<'q>) -> &mut Self {
        if !self.criteria.is_empty() {
            self.criteria.push((" OR ", &[]));
        }
        self.criteria.push((clause.expr(), clause.params()));
        self
    }

    /// Specifies the GROUP BY clause for the query builder.
    ///
    /// This method sets the GROUP BY clause of the query to group the results by the specified fields.
    /// It can only be called once. If called multiple times, the last
    /// call will overwrite the previous one. If `group_by_raw` has been called
    /// previously, it will also be overwritten by this method.
    #[inline]
    pub fn group_by(&mut self, fields: &'q [&str]) -> &mut Self {
        self.group_by = Select::Columns(fields);
        self
    }

    /// Specifies a raw SQL expression for the GROUP BY clause in the query builder.
    ///
    /// This method sets the GROUP BY clause of the query to the specified raw SQL expression.
    /// It can only be called once. If called multiple times, the last
    /// call will overwrite the previous one. If `group_by` has been called previously,
    /// it will also be overwritten by this method.
    #[inline]
    pub fn group_by_raw(&mut self, expr: &'q str) -> &mut Self {
        self.group_by = Select::Raw(expr);
        self
    }

    /// Adds a HAVING clause to the query builder.
    ///
    /// See [`Query:: where `] for the usage of the `clause` argument.
    pub fn having(&mut self, clause: impl Clause<'q>) -> &mut Self {
        if !self.having.is_empty() {
            self.having.push((" AND ", &[]));
        }
        self.having.push((clause.expr(), clause.params()));
        self
    }

    /// Adds a OR HAVING clause to the query builder.
    ///
    /// See [`Query:: where `] for the usage of the `clause` argument.
    pub fn or_having(&mut self, clause: impl Clause<'q>) -> &mut Self {
        if !self.having.is_empty() {
            self.having.push((" OR ", &[]));
        }
        self.having.push((clause.expr(), clause.params()));
        self
    }

    /// Adds an ORDER BY clause to the query builder with ascending order.
    #[inline]
    pub fn order_by(&mut self, order_by: &'q str) -> &mut Self {
        self.order_by.push(OrderBy::Asc(order_by));
        self
    }

    /// Adds an ORDER BY clause to the query builder with descending order.
    #[inline]
    pub fn order_by_desc(&mut self, order_by: &'q str) -> &mut Self {
        self.order_by.push(OrderBy::Desc(order_by));
        self
    }

    /// Adds a raw ORDER BY clause to the query builder.
    #[inline]
    pub fn order_by_raw(&mut self, order_by: &'q str) -> &mut Self {
        self.order_by.push(OrderBy::Raw(order_by));
        self
    }

    /// Sets the OFFSET clause for the query builder.
    #[inline]
    pub fn offset(&mut self, offset: usize) -> &mut Self {
        self.offset = Some(offset);
        self
    }

    /// Sets the LIMIT clause for the query builder.
    #[inline]
    pub fn limit(&mut self, limit: usize) -> &mut Self {
        self.limit = Some(limit);
        self
    }

    /// Fetch a given column
    ///
    /// If more than one column is given, the first column is used.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use sorm::query::Query;
    /// # use sorm::{Database, clause, Result};    
    /// async fn get_all_id(
    ///     db: impl sqlx::Executor<'_, Database = Database>,
    /// ) -> Result<Vec<i64>> {
    ///     Query::table("users").select(&["id"]).plunk(db).await
    /// }
    /// ```
    pub async fn plunk<T>(
        &self,
        executor: impl Executor<'_, Database = Database>,
    ) -> crate::Result<Vec<T>>
    where
        T: for<'r> Decode<'r, Database> + Type<Database>,
    {
        let (sql, params) = self.build_select(None);
        let rows = sqlx::query_with(&sql, to_args(params))
            .fetch_all(executor)
            .await?;
        let mut list = Vec::with_capacity(rows.len());
        for row in rows {
            list.push(row.try_get::<T, _>(0)?);
        }
        Ok(list)
    }

    /// Fetch a single value for a given column.
    ///
    /// If more than one column is given, the first column is used.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use sorm::query::Query;
    /// # use sorm::{Database, clause, Result};
    /// async fn count_user(
    ///     db: impl sqlx::Executor<'_, Database = Database>,
    ///     name: &str,
    /// ) -> Result<Option<i64>> {
    ///     Query::table("users").select_raw("COUNT(0)").value(db).await
    /// }
    /// ```
    pub async fn value<T>(
        &self,
        executor: impl Executor<'_, Database = Database>,
    ) -> crate::Result<T>
    where
        T: for<'r> Decode<'r, Database> + Type<Database>,
    {
        let (sql, params) = self.build_select(Some(1));
        let row = sqlx::query_with(&sql, to_args(params))
            .fetch_one(executor)
            .await?;
        Ok(row.try_get(0)?)
    }

    /// Fetch an optional value for a given column.
    ///
    /// If more than one column is given, the first column is used.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use sorm::query::Query;
    /// # use sorm::{Database, clause, Result};
    /// async fn get_max_id(
    ///     db: impl sqlx::Executor<'_, Database = Database>,
    ///     name: &str,
    /// ) -> Result<Option<i64>> {
    ///     Query::table("users").select_raw("MAX(id)").value(db).await
    /// }
    /// ```
    pub async fn value_optional<T>(
        &self,
        executor: impl Executor<'_, Database = Database>,
    ) -> crate::Result<Option<T>>
    where
        T: for<'r> Decode<'r, Database> + Type<Database>,
    {
        let (sql, params) = self.build_select(Some(1));
        match sqlx::query_with(&sql, to_args(params))
            .fetch_optional(executor)
            .await?
        {
            Some(row) => Ok(row.try_get(0)?),
            None => Ok(None),
        }
    }

    /// Executes a SELECT query and fetches the results mapped to items of type `T`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use sqlx::FromRow;
    /// # use sorm::query::Query;
    /// # use sorm::{Database, clause, Result};
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i64,
    ///     name: String,
    /// }
    ///
    /// async fn get_all_user(
    ///     db: impl sqlx::Executor<'_, Database = Database>,
    ///     name: &str,
    /// ) -> Result<Vec<User>> {
    ///     Query::table("users").get(db).await
    /// }
    /// ```
    pub async fn get<T>(
        &self,
        executor: impl Executor<'q, Database = Database>,
    ) -> crate::Result<Vec<T>>
    where
        T: for<'r> FromRow<'r, crate::Row> + Send + Unpin,
    {
        let (sql, params) = self.build_select(None);
        Ok(sqlx::query_as_with(&sql, to_args(params))
            .fetch_all(executor)
            .await?)
    }

    /// Executes a SELECT query and fetches a single result mapped to item of type `T`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use sqlx::FromRow;
    /// # use sorm::query::Query;
    /// # use sorm::{Database, clause, Result};
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i64,
    ///     name: String,
    /// }
    ///
    /// async fn find_by_name_or_fail(
    ///     db: impl sqlx::Executor<'_, Database = Database>,
    ///     name: &str,
    /// ) -> Result<User> {
    ///     Query::table("users").r#where(clause!("name={&name}")).find(db).await
    /// }
    /// ```
    pub async fn find<T>(
        &self,
        executor: impl Executor<'q, Database = Database>,
    ) -> crate::Result<T>
    where
        T: for<'r> FromRow<'r, crate::Row> + Send + Unpin,
    {
        let (sql, params) = self.build_select(Some(1));
        Ok(sqlx::query_as_with(&sql, to_args(params))
            .fetch_one(executor)
            .await?)
    }

    /// Executes a SELECT query and fetches an optional result mapped to an item of type `T`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use sqlx::FromRow;
    /// # use sorm::query::Query;
    /// # use sorm::{Database, clause, Result};
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i64,
    ///     name: String,
    /// }
    ///
    /// async fn find_by_name(
    ///     db: impl sqlx::Executor<'_, Database = Database>,
    ///     name: &str,
    /// ) -> Result<Option<User>> {
    ///     Query::table("users").r#where(clause!("name={&name}")).find_optional(db).await
    /// }
    /// ```
    pub async fn find_optional<T>(
        &self,
        executor: impl Executor<'q, Database = Database>,
    ) -> crate::Result<Option<T>>
    where
        T: for<'r> FromRow<'r, crate::Row> + Send + Unpin,
    {
        let (sql, params) = self.build_select(Some(1));
        Ok(sqlx::query_as_with(&sql, to_args(params))
            .fetch_optional(executor)
            .await?)
    }

    fn build_select(&self, limit: Option<usize>) -> (String, Vec<&'q (dyn Param<'q> + Sync)>) {
        let (expr_len, param_count) = self.criteria_size();
        let mut sql = String::with_capacity(64 + expr_len);
        let mut params = Vec::with_capacity(param_count);

        sql.push_str("SELECT ");
        match self.select {
            Select::Columns(fields) => concat_idents(&mut sql, fields),
            Select::Raw(expr) => sql.push_str(expr),
            Select::Omitted(ref omit) => {
                for v in self.columns.unwrap() {
                    if !omit.contains(v) {
                        concat_ident(&mut sql, v);
                    }
                }
            }
            Select::None => match self.columns {
                Some(columns) => concat_idents(&mut sql, columns),
                None => sql.push_str("*"),
            },
        }

        sql.push_str(" FROM ");
        concat_ident(&mut sql, self.table);

        if !self.criteria.is_empty() {
            sql.push_str(" WHERE ");
            for v in &self.criteria {
                sql.push_str(v.expr());
                params.extend_from_slice(v.params());
            }
        }

        match self.group_by {
            Select::Columns(fields) => {
                sql.push_str(" GROUP BY ");
                concat_idents(&mut sql, fields);
            }
            Select::Raw(expr) => {
                sql.push_str(" GROUP BY ");
                sql.push_str(expr);
            }
            Select::Omitted(_) => unreachable!(),
            Select::None => (),
        }

        if !self.having.is_empty() {
            sql.push_str(" HAVING ");
            for v in &self.having {
                sql.push_str(v.expr());
                params.extend_from_slice(v.params());
            }
        }

        if !self.order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            for v in &self.order_by {
                match v {
                    OrderBy::Asc(v) => concat_ident(&mut sql, v),
                    OrderBy::Desc(v) => {
                        concat_ident(&mut sql, v);
                        sql.push_str(" DESC");
                    }
                    OrderBy::Raw(v) => sql.push_str(v),
                }
                sql.push_str(",");
            }
            sql.pop();
        }

        if let Some(limit) = limit.or(self.limit) {
            sql.push_str(&format!(" LIMIT {}", limit))
        }
        if let Some(offset) = self.offset {
            sql.push_str(&format!(" OFFSET {}", offset))
        }

        #[cfg(feature = "postgres")]
        let sql = pg_replace_placeholder(&sql);
        debug!(target: "sorm", "{}", sql);
        #[cfg(feature = "test")]
        test::QUERY.add(&sql, &params);
        (sql, params)
    }

    fn criteria_size(&self) -> (usize, usize) {
        let mut s1 = 0;
        let mut s2 = 0;
        for v in &self.criteria {
            s1 += v.expr().len();
            s2 += v.params().len();
        }

        for v in &self.having {
            s1 += v.expr().len();
            s2 += v.params().len();
        }

        (s1, s2)
    }

    /// Executes a UPDATE query.
    pub async fn update(
        &self,
        executor: impl Executor<'q, Database = Database>,
        update: impl Clause<'q>,
    ) -> crate::Result<u64> {
        if self.criteria.is_empty() {
            return Err(Error::NoWhereClause);
        }

        let (expr_len, param_count) = self.criteria_size();
        let mut sql = String::with_capacity(32 + expr_len);
        let mut params = Vec::with_capacity(param_count);
        params.extend_from_slice(update.params());

        sql.push_str("UPDATE ");
        concat_ident(&mut sql, self.table);
        sql.push_str(" SET ");
        sql.push_str(update.expr());
        sql.push_str(" WHERE ");
        for v in &self.criteria {
            sql.push_str(v.expr());
            params.extend_from_slice(v.params());
        }
        #[cfg(feature = "postgres")]
        let sql = pg_replace_placeholder(&sql);
        debug!(target: "sorm", "{}", sql);
        #[cfg(feature = "test")]
        test::QUERY.add(&sql, &params);
        let result = sqlx::query_with(&sql, to_args(params))
            .execute(executor)
            .await?;
        Ok(result.rows_affected())
    }

    /// Executes a DELETE query.
    pub async fn delete(
        &self,
        executor: impl Executor<'q, Database = Database>,
    ) -> crate::Result<u64> {
        if self.criteria.is_empty() {
            return Err(Error::NoWhereClause);
        }

        let (expr_len, param_count) = self.criteria_size();
        let mut sql = String::with_capacity(32 + expr_len);
        let mut params = Vec::with_capacity(param_count);

        sql.push_str("DELETE FROM ");
        concat_ident(&mut sql, self.table);
        sql.push_str(" WHERE ");
        for v in &self.criteria {
            sql.push_str(v.expr());
            params.extend_from_slice(v.params());
        }
        #[cfg(feature = "postgres")]
        let sql = pg_replace_placeholder(&sql);
        debug!(target: "sorm", "{}", sql);
        #[cfg(feature = "test")]
        test::QUERY.add(&sql, &params);
        let result = sqlx::query_with(&sql, to_args(params))
            .execute(executor)
            .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(feature = "postgres")]
fn pg_replace_placeholder(sql: &str) -> String {
    let mut s = String::with_capacity(sql.len());
    let mut num = 1;
    for c in sql.chars() {
        match c {
            '?' => {
                s.push_str(&format!("${}", num));
                num += 1;
            }
            _ => s.push(c),
        }
    }
    s
}

#[cfg(feature = "sqlite")]
fn to_args<'q>(params: Vec<&'q (dyn Param<'q> + Sync)>) -> crate::Arguments<'q> {
    let mut args = crate::Arguments::default();
    args.reserve(params.len(), params.len());
    for v in params {
        v.add(&mut args);
    }
    args
}

#[cfg(any(feature = "mysql", feature = "postgres"))]
fn to_args<'q>(params: Vec<&'q (dyn Param<'q> + Sync)>) -> crate::Arguments {
    let mut args = crate::Arguments::default();
    args.reserve(params.len(), params.len());
    for v in params {
        v.add(&mut args);
    }
    args
}
