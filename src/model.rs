//! ORM model
use std::borrow::Borrow;
use std::marker::PhantomData;

use log::debug;
use sqlx::database::HasArguments;
use sqlx::encode::IsNull;
use sqlx::{Arguments, Decode, Encode, Executor, FromRow, Type};

use crate::{concat_ident, concat_idents, Clause, Database, Param, Row};

/// Represents a model.
#[allow(async_fn_in_trait)]
pub trait Model: Default + for<'r> FromRow<'r, Row> + Send + Unpin + 'static {
    #[cfg(not(feature = "test"))]
    /// The type of the primary key.
    type PrimaryKey: for<'q> Encode<'q, Database> + Type<Database> + Send + Sync;

    #[cfg(feature = "test")]
    /// The type of the primary key.
    type PrimaryKey: for<'q> Encode<'q, Database> + Type<Database> + std::fmt::Debug + Send + Sync;

    /// The column name of the primary key.
    const PRIMARY_KEY: &'static str;

    /// Indicates if the primary key is auto increment.
    const INCREMENT: bool;

    /// The table for the model.
    const TABLE: &'static str;

    /// The columns of the table.
    const COLUMNS: &'static [&'static str];

    /// Returns the primary.
    fn primary_key(&self) -> crate::Result<&Self::PrimaryKey>;

    /// If the primary key is auto incrementing, sets its value to `id`, do nothing otherwise.
    fn set_increment_id(&mut self, id: u64);

    /// Returns the fields that have been set.
    fn collect_filled(&self) -> Vec<(&'static str, &(dyn Param<'_> + Sync))>;

    /// Returns the fields that have been changed.
    fn collect_changed(&self) -> Vec<(&'static str, &(dyn Param<'_> + Sync))>;

    /// Returns true if there are any changed fields.
    fn is_changed(&self) -> bool;

    /// Marks the model as having no changed fields.
    ///
    /// It resets the change tracking mechanism, indicating that all fields are now considered unchanged.
    fn flush(&mut self);

    /// Sets default values for fields during creation.
    fn fill_create_default(&mut self) {}

    /// Sets default value for fields when updates.
    fn fill_update_default(&mut self) {}

    /// Creates a model using the provided `value`.
    fn from(value: impl Fill<Self>) -> Self {
        let mut model = Self::default();
        value.fill(&mut model);
        model
    }

    /// Fill the model using the provided `value`.
    #[inline]
    fn fill(&mut self, value: impl Fill<Self>) -> &mut Self {
        value.fill(self);
        self
    }

    /// Inserts the model to the database.
    async fn create(
        &mut self,
        executor: impl Executor<'_, Database = Database>,
    ) -> crate::Result<()> {
        self.fill_create_default();
        let fields = self.collect_filled();
        if fields.is_empty() {
            return Ok(());
        }

        let mut sql = String::with_capacity(fields.len() * 10 + 32);
        let mut args = crate::Arguments::default();
        args.reserve(fields.len(), fields.len());
        sql.push_str("INSERT INTO ");
        concat_ident(&mut sql, Self::TABLE);
        sql.push_str(" (");
        for field in &fields {
            concat_ident(&mut sql, field.0);
            sql.push_str(",");
            field.1.add(&mut args);
        }
        sql.pop();
        sql.push_str(") VALUES (");
        #[cfg(feature = "postgres")]
        let mut i = 1;
        for _ in fields {
            #[cfg(feature = "postgres")]
            {
                sql.push_str(&format!("${},", i));
                i += 1;
            }
            #[cfg(not(feature = "postgres"))]
            sql.push_str("?,");
        }
        sql.pop();
        sql.push_str(")");

        #[cfg(feature = "postgres")]
        {
            if Self::INCREMENT {
                sql.push_str(" RETURNING CAST(");
                concat_ident(&mut sql, Self::PRIMARY_KEY);
                sql.push_str(" AS BIGINT)");
                debug!(target: "sorm", "{}", sql);
                use sqlx::Row;
                let id: i64 = sqlx::query_with(&sql, args)
                    .fetch_one(executor)
                    .await?
                    .try_get(0)?;
                self.set_increment_id(id as _);
            } else {
                debug!(target: "sorm", "{}", sql);
                sqlx::query_with(&sql, args).execute(executor).await?;
            }
        }

        #[cfg(not(feature = "postgres"))]
        {
            debug!(target: "sorm", "{}", sql);
            let result = sqlx::query_with(&sql, args).execute(executor).await?;
            #[cfg(feature = "sqlite")]
            let id: u64 = result.last_insert_rowid() as _;
            #[cfg(feature = "mysql")]
            let id = result.last_insert_id();
            self.set_increment_id(id);
        }
        self.flush();
        Ok(())
    }

    /// Updates the model in the database.
    async fn update(
        &mut self,
        executor: impl Executor<'_, Database = Database>,
    ) -> crate::Result<()> {
        if !self.is_changed() {
            return Ok(());
        }
        self.fill_update_default();

        let fields = self.collect_changed();
        let primary_key = self.primary_key()?;
        let mut sql = String::with_capacity(32 + fields.len() * 10);
        let mut args = crate::Arguments::default();
        args.reserve(fields.len() + 1, fields.len() + 1);
        sql.push_str("UPDATE ");
        concat_ident(&mut sql, Self::TABLE);
        sql.push_str(" SET ");
        #[cfg(feature = "postgres")]
        let mut i = 1;
        for field in fields {
            field.1.add(&mut args);
            concat_ident(&mut sql, field.0);
            #[cfg(feature = "postgres")]
            {
                sql.push_str(&format!("=${},", i));
                i += 1;
            }
            #[cfg(not(feature = "postgres"))]
            sql.push_str("=?,");
        }
        sql.pop();
        sql.push_str(" WHERE ");
        concat_ident(&mut sql, Self::PRIMARY_KEY);
        #[cfg(feature = "postgres")]
        sql.push_str(&format!("=${}", i));
        #[cfg(not(feature = "postgres"))]
        sql.push_str("=?");
        (primary_key as &(dyn Param + Send)).add(&mut args);

        debug!(target: "sorm", "{}", sql);
        sqlx::query_with(&sql, args).execute(executor).await?;
        self.flush();
        Ok(())
    }

    /// Finds a model by its primary key or returns `Err`.
    async fn find<T>(
        executor: impl Executor<'_, Database = Database>,
        primary_key: &T,
    ) -> crate::Result<Self>
    where
        Self::PrimaryKey: Borrow<T>,
        for<'q> &'q T: Encode<'q, Database> + Type<Database>,
        T: Sync + ?Sized,
    {
        let mut sql = String::with_capacity(32 + Self::COLUMNS.len() * 10);
        sql.push_str("SELECT ");
        concat_idents(&mut sql, Self::COLUMNS);

        sql.push_str(" FROM ");
        concat_ident(&mut sql, Self::TABLE);
        sql.push_str(" WHERE ");
        concat_ident(&mut sql, Self::PRIMARY_KEY);
        #[cfg(feature = "postgres")]
        sql.push_str("=$1");
        #[cfg(not(feature = "postgres"))]
        sql.push_str("=?");

        debug!(target: "sorm", "{}", sql);
        Ok(sqlx::query_as(&sql)
            .bind(primary_key.borrow())
            .fetch_one(executor)
            .await?)
    }

    /// Finds a model by its primary key.
    async fn find_optional<T>(
        executor: impl Executor<'_, Database = Database>,
        primary_key: &T,
    ) -> crate::Result<Option<Self>>
    where
        Self::PrimaryKey: Borrow<T>,
        for<'q> &'q T: Encode<'q, Database> + Type<Database>,
        T: Sync + ?Sized,
    {
        // 以下实现在某些情况下会导致调用本函数的代码出现 "implementation of `Send` is not general enough" 编译错误，很奇怪
        /*match Self::find(executor, primary_key).await {
            Ok(v) => Ok(Some(v)),
            Err(crate::Error::Sqlx(sqlx::Error::RowNotFound)) => {
                Err(crate::Error::Sqlx(sqlx::Error::RowNotFound))
            }
            _ => Ok(None),
        }*/

        let mut sql = String::with_capacity(32 + Self::COLUMNS.len() * 10);
        sql.push_str("SELECT ");
        concat_idents(&mut sql, Self::COLUMNS);

        sql.push_str(" FROM ");
        concat_ident(&mut sql, Self::TABLE);
        sql.push_str(" WHERE ");
        concat_ident(&mut sql, Self::PRIMARY_KEY);
        #[cfg(feature = "postgres")]
        sql.push_str("=$1");
        #[cfg(not(feature = "postgres"))]
        sql.push_str("=?");

        debug!(target: "sorm", "{}", sql);
        Ok(sqlx::query_as(&sql)
            .bind(primary_key.borrow())
            .fetch_optional(executor)
            .await?)
    }

    /// Deletes a model by its primary key.
    async fn destroy<T>(
        executor: impl Executor<'_, Database = Database>,
        primary_key: &T,
    ) -> crate::Result<u64>
    where
        Self::PrimaryKey: Borrow<T>,
        for<'q> &'q T: Encode<'q, Database> + Type<Database>,
        T: Sync + ?Sized,
    {
        let mut sql = String::with_capacity(32);
        sql.push_str("DELETE FROM ");
        concat_ident(&mut sql, Self::TABLE);
        sql.push_str(" WHERE ");
        concat_ident(&mut sql, Self::PRIMARY_KEY);
        #[cfg(feature = "postgres")]
        sql.push_str("=$1");
        #[cfg(not(feature = "postgres"))]
        sql.push_str("=?");

        debug!(target: "sorm", "{}", sql);
        Ok(sqlx::query(&sql)
            .bind(primary_key.borrow())
            .execute(executor)
            .await?
            .rows_affected())
    }

    /// Deletes the model.
    #[inline]
    async fn delete(&self, executor: impl Executor<'_, Database = Database>) -> crate::Result<u64> {
        Self::destroy(executor, self.primary_key()?).await
    }

    /// Creates a query builder for interacting with a model.
    fn query<'q>() -> Query<'q, Self> {
        Query {
            query: crate::query::Query::new(Self::TABLE, Some(Self::COLUMNS)),
            _marker: PhantomData,
        }
    }
}

/// Wrapper struct for [`crate::query::Query`] which decodes rows into the type `T`.
pub struct Query<'q, T>
where
    T: for<'r> FromRow<'r, Row> + Send + Unpin,
{
    query: crate::query::Query<'q>,
    _marker: PhantomData<&'q T>,
}

impl<'q, T> Query<'q, T>
where
    T: for<'r> FromRow<'r, Row> + Send + Unpin,
{
    /// See [`crate::query::Query::select`]
    #[inline]
    pub fn select(&mut self, fields: &'q [&str]) -> &mut Self {
        self.query.select(fields);
        self
    }

    /// See [`crate::query::Query::select_raw`]
    #[inline]
    pub fn select_raw(&mut self, expr: &'q str) -> &mut Self {
        self.query.select_raw(expr);
        self
    }

    /// Omits specified fields from the query's select columns.
    ///
    /// It can only be called once. If called multiple times, the last call will overwrite the
    /// previous one. It will overwrite and overwritten by `select` and `select_raw`.
    #[inline]
    pub fn omit(&mut self, fields: &'q [&str]) -> &mut Self {
        self.query.omit(fields);
        self
    }

    /// See [`crate::query::Query:: where `]
    #[inline]
    pub fn r#where(&mut self, clause: impl Clause<'q>) -> &mut Self {
        self.query.r#where(clause);
        self
    }

    /// See [`crate::query::Query::or_where`]
    #[inline]
    pub fn or_where(&mut self, clause: impl Clause<'q>) -> &mut Self {
        self.query.or_where(clause);
        self
    }

    /// See [`crate::query::Query::group_by`]
    #[inline]
    pub fn group_by(&mut self, fields: &'q [&str]) -> &mut Self {
        self.query.group_by(fields);
        self
    }

    /// See [`crate::query::Query::group_by_raw`]
    #[inline]
    pub fn group_by_raw(&mut self, expr: &'q str) -> &mut Self {
        self.query.group_by_raw(expr);
        self
    }

    /// See [`crate::query::Query::having`]
    #[inline]
    pub fn having(&mut self, clause: impl Clause<'q>) -> &mut Self {
        self.query.having(clause);
        self
    }

    /// See [`crate::query::Query::or_having`]
    #[inline]
    pub fn or_having(&mut self, clause: impl Clause<'q>) -> &mut Self {
        self.query.or_having(clause);
        self
    }

    /// See [`crate::query::Query::order_by`]
    #[inline]
    pub fn order_by(&mut self, order_by: &'q str) -> &mut Self {
        self.query.order_by(order_by);
        self
    }

    /// See [`crate::query::Query::order_by_desc`]
    #[inline]
    pub fn order_by_desc(&mut self, order_by: &'q str) -> &mut Self {
        self.query.order_by_desc(order_by);
        self
    }

    /// See [`crate::query::Query::order_by_raw`]
    #[inline]
    pub fn order_by_raw(&mut self, order_by: &'q str) -> &mut Self {
        self.query.order_by_raw(order_by);
        self
    }

    /// See [`crate::query::Query::offset`]
    #[inline]
    pub fn offset(&mut self, offset: usize) -> &mut Self {
        self.query.offset(offset);
        self
    }

    /// See [`crate::query::Query::limit`]
    #[inline]
    pub fn limit(&mut self, limit: usize) -> &mut Self {
        self.query.limit(limit);
        self
    }

    /// See [`crate::query::Query::plunk`]
    #[inline]
    pub async fn plunk<U>(
        &self,
        executor: impl Executor<'_, Database = Database>,
    ) -> crate::Result<Vec<U>>
    where
        U: for<'r> Decode<'r, Database> + Type<Database>,
    {
        self.query.plunk(executor).await
    }

    /// See [`crate::query::Query::value`]
    #[inline]
    pub async fn value<U>(
        &self,
        executor: impl Executor<'_, Database = Database>,
    ) -> crate::Result<U>
    where
        U: for<'r> Decode<'r, Database> + Type<Database>,
    {
        self.query.value(executor).await
    }

    /// See [`crate::query::Query::value_optional`]
    #[inline]
    pub async fn value_optional<U>(
        &self,
        executor: impl Executor<'_, Database = Database>,
    ) -> crate::Result<Option<U>>
    where
        U: for<'r> Decode<'r, Database> + Type<Database>,
    {
        self.query.value_optional(executor).await
    }

    /// See [`crate::query::Query::get`]
    #[inline]
    pub async fn get(
        &self,
        executor: impl Executor<'q, Database = Database>,
    ) -> crate::Result<Vec<T>> {
        self.query.get(executor).await
    }

    /// See [`crate::query::Query::find`]
    #[inline]
    pub async fn find(&self, executor: impl Executor<'q, Database = Database>) -> crate::Result<T> {
        self.query.find(executor).await
    }

    /// See [`crate::query::Query::find_optional`]
    #[inline]
    pub async fn find_optional(
        &self,
        executor: impl Executor<'q, Database = Database>,
    ) -> crate::Result<Option<T>> {
        self.query.find_optional(executor).await
    }

    /// See [`crate::query::Query::delete`]
    #[inline]
    pub async fn delete(
        &self,
        executor: impl Executor<'q, Database = Database>,
    ) -> crate::Result<u64> {
        self.query.delete(executor).await
    }

    /// See [`crate::query::Query::update`]
    pub async fn update(
        &self,
        executor: impl Executor<'q, Database = Database>,
        update: impl Clause<'q>,
    ) -> crate::Result<u64> {
        self.query.update(executor, update).await
    }
}

/// Used to fill a model.
pub trait Fill<T: Model> {
    /// Fill the `model` with `self`
    fn fill(self, model: &mut T);
}

#[cfg(not(feature = "test"))]
/// Represents the type of the primary key for models without primary key.
pub enum HasNoPrimaryKey {}

#[cfg(feature = "test")]
/// Represents the type of the primary key for models without primary key.
#[derive(Debug)]
pub enum HasNoPrimaryKey {}

impl<'q, DB: sqlx::Database> Encode<'q, DB> for HasNoPrimaryKey {
    fn encode_by_ref(&self, _buf: &mut <DB as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        match *self {}
    }
}

impl<DB: sqlx::Database> Type<DB> for HasNoPrimaryKey {
    fn type_info() -> DB::TypeInfo {
        unreachable!()
    }
}

pub trait Int {}
impl Int for i32 {}
impl Int for i64 {}
impl Int for u32 {}
impl Int for u64 {}
