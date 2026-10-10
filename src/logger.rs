//! File logger: writes to <files dir>/liquid_demo.log (previous run -> liquid_demo.prev.log)
//! so the Java LogActivity can show/copy it even after a crash.
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use log::{Level, LevelFilter, Log, Metadata, Record};

const FILE_NAME: &str = "liquid_demo.log";
const PREV_NAME: &str = "liquid_demo.prev.log";
const MAX_BYTES: usize = 4_000_000;

struct FileLogger {
    file: Mutex<Option<(File, usize)>>,
    start: Instant,
}

fn max_level_for(target: &str) -> Level {
    if target.starts_with("liquid_demo") || target.starts_with("liquid_rust") {
        Level::Debug
    } else if target.starts_with("naga") {
        Level::Warn
    } else {
        Level::Info
    }
}

impl Log for FileLogger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= max_level_for(m.target())
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!(
            "{:>9.3} {:<5} {}: {}\n",
            self.start.elapsed().as_secs_f64(),
            r.level(),
            r.target(),
            r.args()
        );
        if let Ok(mut g) = self.file.lock() {
            if let Some((f, written)) = g.as_mut() {
                if *written < MAX_BYTES {
                    let _ = f.write_all(line.as_bytes());
                    *written += line.len();
                }
            }
        }
        eprint!("{line}"); // also visible in logcat (tag RustStdoutStderr)
    }

    fn flush(&self) {
        if let Ok(mut g) = self.file.lock() {
            if let Some((f, _)) = g.as_mut() {
                let _ = f.flush();
                let _ = f.sync_all();
            }
        }
    }
}

pub fn init(dir: Option<PathBuf>) {
    let file = dir.and_then(|d| {
        let cur = d.join(FILE_NAME);
        let prev = d.join(PREV_NAME);
        let _ = fs::remove_file(&prev);
        let _ = fs::rename(&cur, &prev);
        File::create(&cur).ok().map(|f| (f, 0usize))
    });
    let logger = FileLogger { file: Mutex::new(file), start: Instant::now() };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        log::error!("PANIC: {info}\n{bt}");
        log::logger().flush();
    }));
    log::info!("logger ready");
}
