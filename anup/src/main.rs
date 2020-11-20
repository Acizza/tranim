#![warn(clippy::pedantic)]
#![allow(clippy::clippy::cast_possible_truncation)]
#![allow(clippy::inline_always)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::shadow_unrelated)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::map_err_ignore)]

#[macro_use]
extern crate diesel;

mod config;
mod database;
mod err;
mod file;
mod series;
mod tui;
mod user;
mod util;

use crate::config::Config;
use crate::database::Database;
use crate::file::SerializedFile;
use crate::series::config::SeriesConfig;
use crate::series::entry::SeriesEntry;
use crate::series::info::SeriesInfo;
use crate::series::{LastWatched, LoadedSeries, Series};
use crate::user::Users;
use anime::remote::Remote;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;

const ANILIST_CLIENT_ID: u32 = 427;

pub struct CmdOptions {
    pub offline: bool,
    pub single: bool,
    pub sync: bool,
    pub series: Option<String>,
}

impl CmdOptions {
    fn from_env() -> Result<Self> {
        let mut args = pico_args::Arguments::from_env();

        if args.contains(["-h", "--help"]) {
            Self::print_help();
        }

        let result = Self {
            offline: args.contains(["-o", "--offline"]),
            single: args.contains("--play-one"),
            sync: args.contains("--sync"),
            series: args.free()?.into_iter().next(),
        };

        Ok(result)
    }

    fn print_help() {
        println!(concat!(
            "Usage: ",
            env!("CARGO_PKG_NAME"),
            " [series] [OPTIONS]\n"
        ));

        println!("Free arguments:");
        println!("  series - the nickname of the series to watch");

        println!();

        println!("Optional arguments:");
        println!("  -o, --offline  run the program in offline mode");
        println!("  --play-one     play a single episode from the last played series");
        println!("  --sync         syncronize changes made while offline to AniList");

        std::process::exit(0);
    }
}

fn main() -> Result<()> {
    let args = CmdOptions::from_env()?;

    if args.single {
        play_episode(&args)
    } else if args.sync {
        sync(&args)
    } else {
        tui::run(&args)
    }
}

/// Initialize a new remote service specified by `args`.
///
/// If there are no users, returns Ok(None).
fn init_remote(args: &CmdOptions) -> Result<Option<Remote>> {
    use anime::remote::anilist::{AniList, Auth};

    if args.offline {
        Ok(Some(Remote::offline()))
    } else {
        let token = match Users::load_or_create()?.take_last_used_token() {
            Some(token) => token,
            None => return Ok(None),
        };

        let auth = Auth::retrieve(token)?;
        Ok(Some(AniList::Authenticated(auth).into()))
    }
}

fn sync(args: &CmdOptions) -> Result<()> {
    if args.offline {
        return Err(anyhow!("must be online to run this command"));
    }

    let db = Database::open().context("failed to open database")?;
    let mut list_entries = SeriesEntry::entries_that_need_sync(&db)?;

    if list_entries.is_empty() {
        return Ok(());
    }

    let remote =
        init_remote(&args)?.ok_or_else(|| anyhow!("no users found\nadd one in the TUI"))?;

    for entry in &mut list_entries {
        match SeriesInfo::load(&db, entry.id()) {
            Ok(info) => println!("{} is being synced..", info.title_preferred),
            Err(err) => eprintln!(
                "warning: failed to get info for anime with ID {}: {}",
                entry.id(),
                err
            ),
        }

        entry.sync_to_remote(&remote)?;
        entry.save(&db)?;
    }

    Ok(())
}

fn play_episode(args: &CmdOptions) -> Result<()> {
    use anime::remote::Status;

    let config = Config::load_or_create()?;
    let db = Database::open().context("failed to open database")?;
    let mut last_watched = LastWatched::load()?;

    let remote =
        init_remote(&args)?.ok_or_else(|| anyhow!("no users found\nadd one in the TUI"))?;

    let desired_series = args
        .series
        .as_ref()
        .or_else(|| last_watched.get())
        .ok_or_else(|| anyhow!("series name must be specified"))?;

    let mut series = {
        let cfg = SeriesConfig::load_by_name(&db, desired_series).with_context(|| {
            format!(
                "{} must be added to the program in the TUI first",
                desired_series
            )
        })?;

        match Series::load_from_config(cfg, &config, &db) {
            LoadedSeries::Complete(series) => series,
            LoadedSeries::Partial(_, err) => return Err(err.into()),
            LoadedSeries::None(_, err) => return Err(err),
        }
    };

    if last_watched.set(&series.data.config.nickname) {
        last_watched.save()?;
    }

    series.begin_watching(&remote, &config, &db)?;

    let progress_time = series.data.next_watch_progress_time(&config);
    let next_episode_num = series.data.entry.watched_episodes() + 1;

    series
        .play_episode(next_episode_num as u32, &config)?
        .wait()
        .context("waiting for episode to finish failed")?;

    if Utc::now() >= progress_time {
        series.episode_completed(&remote, &config, &db)?;

        if series.data.entry.status() == Status::Completed {
            println!("{} completed!", series.data.info.title_preferred);
        } else {
            println!(
                "{}/{} of {} completed",
                series.data.entry.watched_episodes(),
                series.data.info.episodes,
                series.data.info.title_preferred
            );
        }
    } else {
        println!("did not watch long enough to count episode as completed");
    }

    Ok(())
}
