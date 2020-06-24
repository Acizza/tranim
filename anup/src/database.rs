use crate::file::SaveDir;
use anyhow::{Context, Result};
use diesel::connection::SimpleConnection;
use diesel::deserialize::{self, FromSql};
use diesel::prelude::*;
use diesel::serialize::{self, Output, ToSql};
use diesel::sql_types::{Integer, Nullable, Text};
use smallvec::SmallVec;
use std::io::Write;
use std::ops::Deref;
use std::path::PathBuf;

pub mod schema {
    table! {
        series_configs {
            id -> Integer,
            nickname -> Text,
            path -> Text,
            episode_parser -> Nullable<Text>,
            player_args -> Nullable<Text>,
        }
    }

    table! {
        series_info {
            id -> Integer,
            title_preferred -> Text,
            title_romaji -> Text,
            episodes -> SmallInt,
            episode_length_mins -> SmallInt,
        }
    }

    table! {
        series_entries {
            id -> Integer,
            watched_episodes -> SmallInt,
            score -> Nullable<SmallInt>,
            status -> SmallInt,
            times_rewatched -> SmallInt,
            start_date -> Nullable<Date>,
            end_date -> Nullable<Date>,
            needs_sync -> Bool,
        }
    }
}

pub struct Database(SqliteConnection);

impl Database {
    pub fn open() -> Result<Self> {
        let path = Self::validated_path().context("getting path")?;
        let conn = SqliteConnection::establish(&path.to_string_lossy())?;

        conn.batch_execute(include_str!("../sql/pragmas.sql"))
            .context("executing pragmas")?;

        let db_version = Self::user_version(&conn).context("getting user version")?;

        conn.batch_execute(include_str!("../sql/schema.sql"))
            .context("executing schema")?;

        // Migrations for June 15th, 2020
        if db_version == 0 {
            conn.batch_execute(include_str!("../sql/migrations/rename_episode_matcher.sql"))
                .ok();

            conn.batch_execute(include_str!(
                "../sql/migrations/delete_series_info_sequels.sql"
            ))
            .ok();
        }

        Ok(Self(conn))
    }

    pub fn validated_path() -> Result<PathBuf> {
        let mut path = SaveDir::LocalData.validated_dir_path()?.to_path_buf();
        path.push("data.sqlite");
        Ok(path)
    }

    #[inline(always)]
    pub fn conn(&self) -> &SqliteConnection {
        &self.0
    }

    fn user_version(conn: &SqliteConnection) -> diesel::QueryResult<i32> {
        #[derive(QueryableByName)]
        struct UserVersion {
            #[sql_type = "Integer"]
            user_version: i32,
        }

        diesel::sql_query("PRAGMA user_version")
            .get_result::<UserVersion>(conn)
            .map(|ver| ver.user_version)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        self.conn().execute("PRAGMA optimize").ok();
    }
}

#[derive(Clone, Debug, Default, AsExpression, FromSqlRow)]
#[sql_type = "Text"]
pub struct PlayerArgs(SmallVec<[String; 3]>);

impl PlayerArgs {
    #[inline(always)]
    pub fn new() -> Self {
        Self(SmallVec::new())
    }
}

impl<DB> FromSql<Nullable<Text>, DB> for PlayerArgs
where
    DB: diesel::backend::Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        match bytes {
            Some(_) => {
                let args = String::from_sql(bytes)?
                    .split(";;")
                    .map(Into::into)
                    .collect();

                Ok(Self(args))
            }
            None => Ok(Self::new()),
        }
    }
}

impl<DB> ToSql<Text, DB> for PlayerArgs
where
    DB: diesel::backend::Backend,
    String: ToSql<Text, DB>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
        let value = self.0.join(";;");
        value.to_sql(out)
    }
}

impl AsRef<SmallVec<[String; 3]>> for PlayerArgs {
    fn as_ref(&self) -> &SmallVec<[String; 3]> {
        &self.0
    }
}

impl From<SmallVec<[String; 3]>> for PlayerArgs {
    fn from(value: SmallVec<[String; 3]>) -> Self {
        Self(value)
    }
}

impl Deref for PlayerArgs {
    type Target = SmallVec<[String; 3]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
