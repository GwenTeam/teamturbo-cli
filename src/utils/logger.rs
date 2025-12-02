use console::style;
use std::sync::atomic::{AtomicBool, Ordering};

static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Initialize logger with verbose mode
pub fn init(verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
}

/// Check if verbose mode is enabled
pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Print verbose log message
pub fn verbose(message: &str) {
    if is_verbose() {
        eprintln!("{} {}", style("[VERBOSE]").dim(), style(message).dim());
    }
}

/// Print verbose log with formatted arguments
#[macro_export]
macro_rules! verbose {
    ($($arg:tt)*) => {
        if $crate::utils::logger::is_verbose() {
            eprintln!("{} {}", console::style("[VERBOSE]").dim(), console::style(format!($($arg)*)).dim());
        }
    };
}

/// Print HTTP request details
pub fn http_request(method: &str, url: &str) {
    if is_verbose() {
        eprintln!(
            "{} {} {}",
            style("[HTTP]").cyan().dim(),
            style(method).bold().dim(),
            style(url).dim()
        );
    }
}

/// Print HTTP response details
pub fn http_response(status: u16, url: &str) {
    if is_verbose() {
        let status_str = if status >= 200 && status < 300 {
            style(status).green().dim()
        } else if status >= 400 {
            style(status).red().dim()
        } else {
            style(status).yellow().dim()
        };

        eprintln!(
            "{} {} {}",
            style("[HTTP]").cyan().dim(),
            status_str,
            style(url).dim()
        );
    }
}

/// Print debug information about operation
pub fn debug(context: &str, message: &str) {
    if is_verbose() {
        eprintln!(
            "{} [{}] {}",
            style("[DEBUG]").blue().dim(),
            style(context).yellow().dim(),
            style(message).dim()
        );
    }
}
