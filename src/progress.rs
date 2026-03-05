use console::Style;
use lazy_static::lazy_static;

// Re-export indicatif types so consumers of `progress::*` have them available
pub use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

const TICK_CHARS: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
const PROGRESS_CHARS: &str = "━╸━";

const NONE_TEMPLATE: &str = "  {spinner:.cyan} {wide_msg}";
const COUNT_TEMPLATE: &str = "  {spinner:.cyan} {wide_msg}\n        {pos}/{len} [{bar:40.cyan/dim}] {per_sec} {elapsed_precise} (ETA {eta_precise})";
const BYTES_TEMPLATE: &str = "  {spinner:.cyan} {wide_msg}\n        {bytes}/{total_bytes} [{bar:40.cyan/dim}] {bytes_per_sec} {elapsed_precise} (ETA {eta_precise})";

lazy_static! {
    static ref MULTI_PROGRESS: MultiProgress = MultiProgress::new();

    // Prefixed styles
    static ref STYLE_HEADER: Style = Style::new().bold().cyan();
    static ref STYLE_SUBHEADER: Style = Style::new().bold();
    static ref STYLE_SUCCESS: Style = Style::new().green();
    static ref STYLE_WARNING: Style = Style::new().yellow();
    static ref STYLE_ERROR: Style = Style::new().red().bold();
    static ref STYLE_SKIP: Style = Style::new().dim();
    static ref STYLE_ACTION: Style = Style::new().bold();
    static ref STYLE_DIM: Style = Style::new().dim();
}

/// Returns the global MultiProgress instance.
/// All progress bars should be added to this instance.
pub fn get_multi_progress() -> &'static MultiProgress {
    &MULTI_PROGRESS
}

pub fn get_progress_bar(length: u64, style: ProgressStyle) -> ProgressBar {
    let pb = MULTI_PROGRESS.add(ProgressBar::new(length).with_style(style));
    pb
}

pub fn get_none_progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .tick_chars(TICK_CHARS)
        .template(NONE_TEMPLATE)
        .expect("Failed to create progress bar")
}

pub fn get_count_progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .tick_chars(TICK_CHARS)
        .template(COUNT_TEMPLATE)
        .expect("Failed to create progress bar")
        .progress_chars(PROGRESS_CHARS)
}

pub fn get_bytes_progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .tick_chars(TICK_CHARS)
        .template(BYTES_TEMPLATE)
        .expect("Failed to create progress bar")
        .progress_chars(PROGRESS_CHARS)
}

// ── Categorized output helpers ──────────────────────────────────────────────

/// Print a bold cyan header, e.g. system names or section titles.
/// Example: "◆ Processing \"Nintendo - Game Boy\""
pub fn print_header(progress_bar: &ProgressBar, message: &str) {
    progress_bar.println(format!(
        "  {} {}",
        STYLE_HEADER.apply_to("◆"),
        STYLE_HEADER.apply_to(message),
    ));
}

/// Print a subheader for steps within a section.
/// Example: "  ▸ Processing games"
pub fn print_subheader(progress_bar: &ProgressBar, message: &str) {
    progress_bar.println(format!(
        "    {} {}",
        STYLE_SUBHEADER.apply_to("▸"),
        STYLE_SUBHEADER.apply_to(message),
    ));
}

/// Print an informational message.
/// Example: "  ℹ System: Test System"
pub fn print_info(progress_bar: &ProgressBar, message: &str) {
    progress_bar.println(format!("    {} {}", STYLE_DIM.apply_to("ℹ"), message,));
}

/// Print a success/completion message.
/// Example: "  ✔ Imported Test Game (USA)"
pub fn print_success(progress_bar: &ProgressBar, message: &str) {
    progress_bar.println(format!("    {} {}", STYLE_SUCCESS.apply_to("✔"), message,));
}

/// Print a warning message.
/// Example: "  ⚠ Converted file doesn't match the original"
pub fn print_warning(progress_bar: &ProgressBar, message: &str) {
    progress_bar.println(format!(
        "    {} {}",
        STYLE_WARNING.apply_to("⚠"),
        STYLE_WARNING.apply_to(message),
    ));
}

/// Print an error message.
/// Example: "  ✖ Failed to parse DAT file"
pub fn print_error(progress_bar: &ProgressBar, message: &str) {
    progress_bar.println(format!(
        "    {} {}",
        STYLE_ERROR.apply_to("✖"),
        STYLE_ERROR.apply_to(message),
    ));
}

/// Print a skip/dim message.
/// Example: "  ↪ Already at version \"20200721\""
pub fn print_skip(progress_bar: &ProgressBar, message: &str) {
    progress_bar.println(format!(
        "    {} {}",
        STYLE_SKIP.apply_to("↪"),
        STYLE_SKIP.apply_to(message),
    ));
}

/// Print an action message for file operations (create, copy, move, delete, etc.)
/// Example: "  → Moving to \"/path/to/file\""
pub fn print_action(progress_bar: &ProgressBar, message: &str) {
    progress_bar.println(format!("    {} {}", STYLE_DIM.apply_to("→"), message,));
}

/// Print a blank separator line.
pub fn print_separator(progress_bar: &ProgressBar) {
    progress_bar.println("");
}
