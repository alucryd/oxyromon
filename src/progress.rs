use indicatif::{ProgressBar, ProgressStyle};

const PROGRESS_CHARS: &str = "#-";
const NONE_TEMPLATE: &str = "{spinner} {wide_msg}";
const COUNT_TEMPLATE: &str =
    "{spinner} {wide_msg} {pos}/{len} {per_sec} {elapsed_precise} ({eta_precise}) [{bar:80}]";
const BYTES_TEMPLATE: &str =
    "{spinner} {wide_msg} {bytes}/{total_bytes} {bytes_per_sec} {elapsed_precise} ({eta_precise}) [{bar:80}]";

pub fn get_progress_bar(length: u64, style: ProgressStyle) -> ProgressBar {
    let progress_bar = ProgressBar::new(length).with_style(style);
    progress_bar.enable_steady_tick(100);
    progress_bar
}

pub fn get_none_progress_style() -> ProgressStyle {
    ProgressStyle::default_bar().template(NONE_TEMPLATE)
}

pub fn get_count_progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template(COUNT_TEMPLATE)
        .progress_chars(PROGRESS_CHARS)
}

pub fn get_bytes_progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template(BYTES_TEMPLATE)
        .progress_chars(PROGRESS_CHARS)
}
