#![feature(iter_intersperse)]

#[macro_use]
extern crate clap;
extern crate chrono;
extern crate shellexpand;

use chrono::{Duration, Local, NaiveDateTime};
use clap::App;
use std::cmp::min;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

fn abspath(p: &str) -> Option<PathBuf> {
    shellexpand::full(p)
        .ok()
        .and_then(|x| Path::new(OsStr::new(x.as_ref())).canonicalize().ok())
}

fn parse_log_timestamp(line: &str) -> Option<(NaiveDateTime, String)> {
    // Parse the timestamp
    let words = line.split_ascii_whitespace().map(|w| w.to_string() + " ");
    let ts = words.clone().take(2).collect::<String>();
    if let Ok(ts) = NaiveDateTime::parse_from_str(&ts.trim_end(), "%Y%m%d %H:%M:%S%.f") {
        let rest = words.skip(2).collect::<String>();
        Some((ts, rest))
    } else {
        None
    }
}

fn main() {
    // Parse CLI args
    let yaml = load_yaml!("cli.yml");
    let args = App::from_yaml(yaml).get_matches();

    // Load the grin server log file
    let logpath = args
        .value_of("logfile")
        .unwrap_or("~/.grin/main/grin-server.log");
    println!("Parsing {}", &logpath);
    let logpath = abspath(logpath).unwrap();
    let logpath = Path::new(&logpath);
    if !logpath.exists() {
        println!("Couldn't find grin log file at {:?}\n", logpath);
        println!("See help menu to specify custom file path");
        return ();
    }

    // Setup timing parameters
    let avg_duration = match args.value_of("time") {
        None => None,
        Some(min) => match min.parse::<i64>() {
            Ok(min) => Some(Duration::minutes(min)),
            Err(_) => {
                println!("Couldn't parse '--time' option. Please provide an integer.");
                return ();
            }
        },
    };
    let ts_now = Local::now().naive_local();
    let ts_start_calc = match avg_duration {
        Some(d) => Some(ts_now - d),
        None => None,
    };
    let mut ts_first_log = Local::now().naive_local();
    let mut ts_last_log = Local::now().naive_local();

    // Parse and collect worker stats
    let mut worker_stats = HashMap::new();
    BufReader::new(File::open(logpath).unwrap())
        .lines()
        .filter_map(|l| Some(l.expect("Failed to get log line")))
        .filter_map(|l| parse_log_timestamp(&l))
        .inspect(|(ts, _)| {
            // Update time stats
            ts_first_log = min(ts_first_log, *ts);
            ts_last_log = *ts;
        })
        .filter(|(ts, _)| ts_start_calc.is_none() || ts >= &ts_start_calc.unwrap())
        .filter(|(_, rest)| rest.find("mining").is_some())
        .map(|(ts, rest)| {
            // Filter logs for share reports
            let needle = "submitted by ";
            if let Some(pos) = rest.rfind(needle) {
                let worker_name = rest[pos + needle.len()..].trim().to_string();
                let shares = worker_stats.entry(worker_name).or_insert(0);
                *shares += 1;
            }
            (ts, rest)
        })
        .count();

    // Make sure we had enough data to run the requested stats
    let log_duration = ts_last_log - ts_first_log;
    if let Some(d) = avg_duration && log_duration <= d {
        println!("Error! Not enough data.");
        println!(
            "Only have {} minutes of data. Need {} minutes.",
            d.num_minutes(),
            d.num_minutes()
        );
        return ();
    }

    // Print summary
    let avg_duration = ts_last_log - ts_start_calc.unwrap_or(ts_first_log);
    for (worker, shares) in worker_stats {
        let hr = 42.0 * shares as f64 / avg_duration.num_seconds() as f64;
        println!(
            "{}: {:.2} G/s ({} shares in {} min)",
            worker,
            hr,
            shares,
            avg_duration.num_minutes()
        );
    }
}
