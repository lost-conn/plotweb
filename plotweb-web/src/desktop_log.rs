//! Desktop logging: stderr **and** a log file, set up by the app itself.
//!
//! rinch's shell logs to stderr only. That is nowhere when the app was started
//! from a launcher, and on Windows a release build has no console at all
//! (`windows_subsystem = "windows"`), so a crash or a sync warning would leave no
//! trace. `scripts/plotweb-desktop.sh` used to paper over this on Linux with a
//! `tee`; a downloaded build has no script, so the binary writes the file.
//!
//! Installed before `App::run`, so rinch finds a subscriber already in place and
//! adds none of its own (`rinch::logging::install_shell_subscriber`).

use std::fs::{self, OpenOptions};
use std::sync::Mutex;

use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::EnvFilter;

/// Rotate the file past this size, keeping one previous run as `plotweb.log.1`.
/// A writing session is long and rinch warns per dropped frame, so it does grow.
const ROTATE_BYTES: u64 = 5 * 1024 * 1024;

/// rinch's own chatter at `warn`, PlotWeb's `log::info!` let through.
/// `RUST_LOG` overrides it.
const DEFAULT_FILTER: &str = "warn,plotweb_web=info";

pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(DEFAULT_FILTER))
        .unwrap_or_else(|_| EnvFilter::new("warn"));

    let file = crate::platform::native_log_dir().and_then(|dir| {
        fs::create_dir_all(&dir).ok()?;
        let path = dir.join("plotweb.log");
        if fs::metadata(&path).map(|m| m.len() > ROTATE_BYTES).unwrap_or(false) {
            let _ = fs::rename(&path, dir.join("plotweb.log.1"));
        }
        OpenOptions::new().create(true).append(true).open(&path).ok()
    });

    let installed = match file {
        Some(file) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(std::io::stderr.and(Mutex::new(file)))
            .try_init()
            .is_ok(),
        None => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init()
            .is_ok(),
    };

    // A panic aborts (non-unwinding), and its message goes to stderr — so on
    // Windows it would vanish. Route it through the log first.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));

    if installed {
        log::info!(
            "PlotWeb {} starting; log dir {:?}",
            env!("CARGO_PKG_VERSION"),
            crate::platform::native_log_dir()
        );
    }
}
