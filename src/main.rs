use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use console::Style;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use serde::Deserialize;
use url::Url;

const FILE_MARKER: &str = "__INSTADOWN_FILE__";

#[derive(Debug, Parser)]
#[command(
    name = "instadown",
    version,
    about = "Download an Instagram post or reel",
    after_help = "Examples:\n  instadown https://www.instagram.com/reel/ABC123/\n  instadown --no-compress https://instagram.com/p/ABC123/\n  instadown --audio https://instagram.com/reel/ABC123/"
)]
struct Cli {
    /// Extract audio only and save it as an MP3
    #[arg(long)]
    audio: bool,

    /// Keep the original media quality without the default light compression
    #[arg(long)]
    no_compress: bool,

    /// Load login cookies from a browser (for example: firefox or chrome)
    #[arg(long, value_name = "BROWSER[:PROFILE]")]
    cookies_from_browser: Option<String>,

    /// Public Instagram post or reel URL
    url: String,
}

#[derive(Debug, Default, Deserialize)]
struct Metadata {
    id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    uploader: Option<String>,
    uploader_id: Option<String>,
    duration: Option<f64>,
    like_count: Option<u64>,
    comment_count: Option<u64>,
    entries: Option<Vec<serde_json::Value>>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!(
            "{} {}",
            Style::new().red().bold().apply_to("[error]"),
            error
        );
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let url = validate_instagram_url(&cli.url)?;
    require_command(
        "yt-dlp",
        "--version",
        "Install it with: pipx install yt-dlp",
    )?;
    require_command(
        "ffmpeg",
        "-version",
        "Install ffmpeg with your system package manager",
    )?;

    let spinner = spinner("Fetching post metadata...");
    let metadata = match fetch_metadata(url.as_str(), cli.cookies_from_browser.as_deref()) {
        Ok(metadata) => {
            spinner.finish_and_clear();
            metadata
        }
        Err(error) => {
            spinner.finish_and_clear();
            return Err(error);
        }
    };

    print_metadata(&metadata, cli.audio, !cli.no_compress);
    download(
        url.as_str(),
        cli.audio,
        cli.no_compress,
        cli.cookies_from_browser.as_deref(),
    )?;
    Ok(())
}

fn validate_instagram_url(input: &str) -> Result<Url> {
    let url = Url::parse(input).context("the supplied value is not a valid URL")?;
    if url.scheme() != "http" && url.scheme() != "https" {
        bail!("URL must use http or https");
    }

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host != "instagram.com" && host != "www.instagram.com" && host != "m.instagram.com" {
        bail!("expected an instagram.com URL");
    }

    let mut segments = url.path_segments().into_iter().flatten();
    let kind = segments.next().unwrap_or_default();
    let shortcode = segments.next().unwrap_or_default();
    if !matches!(kind, "p" | "reel" | "reels" | "tv") || shortcode.is_empty() {
        bail!("expected an Instagram post or reel URL (.../p/ID or .../reel/ID)");
    }
    Ok(url)
}

fn require_command(command: &str, version_arg: &str, hint: &str) -> Result<()> {
    let available = Command::new(command)
        .arg(version_arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !available {
        bail!("required command `{command}` was not found. {hint}");
    }
    Ok(())
}

fn fetch_metadata(url: &str, cookies_from_browser: Option<&str>) -> Result<Metadata> {
    let mut command = Command::new("yt-dlp");
    command.args(["--dump-single-json", "--skip-download", "--no-warnings"]);
    add_cookie_args(&mut command, cookies_from_browser);
    let output = command
        .arg(url)
        .output()
        .context("failed to start yt-dlp")?;

    if !output.status.success() {
        bail!("{}", useful_error(&output.stderr));
    }
    serde_json::from_slice(&output.stdout).context("Instagram returned unreadable metadata")
}

fn print_metadata(metadata: &Metadata, audio: bool, compress: bool) {
    let heading = Style::new().cyan().bright().bold();
    let label = Style::new().cyan();
    let muted = Style::new().dim();
    let accent = Style::new().yellow();
    println!(
        "{} {}",
        heading.apply_to("INSTADOWN"),
        muted.apply_to("/ Instagram media")
    );

    if let Some(author) = metadata.uploader.as_ref().or(metadata.uploader_id.as_ref()) {
        println!(
            "  {}  @{}",
            label.apply_to("creator "),
            author.trim_start_matches('@')
        );
    }
    if let Some(title) = metadata
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        println!("  {}  {}", label.apply_to("title   "), one_line(title, 76));
    } else if let Some(description) = metadata
        .description
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        println!(
            "  {}  {}",
            label.apply_to("caption "),
            one_line(description, 76)
        );
    }
    if let Some(id) = metadata.id.as_deref() {
        println!("  {}  {}", label.apply_to("post id "), muted.apply_to(id));
    }

    let mut details = Vec::new();
    if let Some(duration) = metadata.duration {
        details.push(format_duration(duration));
    }
    if let Some(likes) = metadata.like_count {
        details.push(format!("{} likes", compact_count(likes)));
    }
    if let Some(comments) = metadata.comment_count {
        details.push(format!("{} comments", compact_count(comments)));
    }
    if let Some(count) = metadata
        .entries
        .as_ref()
        .map(Vec::len)
        .filter(|count| *count > 1)
    {
        details.push(format!("{count} items"));
    }
    if !details.is_empty() {
        println!(
            "  {}  {}",
            label.apply_to("details "),
            details.join("  |  ")
        );
    }
    let mode = if audio && compress {
        "audio (MP3, lightly compressed)"
    } else if audio {
        "audio (MP3, highest quality)"
    } else if compress {
        "video / image (light compression)"
    } else {
        "video / image (original quality)"
    };
    println!(
        "  {}  {}\n",
        label.apply_to("mode    "),
        accent.apply_to(mode)
    );
}

fn download(
    url: &str,
    audio: bool,
    no_compress: bool,
    cookies_from_browser: Option<&str>,
) -> Result<()> {
    let template = "%(uploader|instagram)s_%(id)s_%(title).80B.%(ext)s";
    let mut command = Command::new("yt-dlp");
    command.args([
        "--newline",
        "--progress",
        "--no-colors",
        "--no-warnings",
        "--restrict-filenames",
        "--output",
        template,
        "--print",
        &format!("after_move:{FILE_MARKER}%(filepath)s"),
    ]);
    if audio {
        command
            .args([
                "--extract-audio",
                "--audio-format",
                "mp3",
                "--audio-quality",
            ])
            .arg(if no_compress { "0" } else { "4" });
    } else {
        let format = if no_compress {
            "bestvideo+bestaudio/best"
        } else {
            "bestvideo[width<=1280][height<=1280]+bestaudio/best[width<=1280][height<=1280]/best"
        };
        command.args(["--format", format, "--merge-output-format", "mp4"]);
    }
    add_cookie_args(&mut command, cookies_from_browser);
    command
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().context("failed to start yt-dlp")?;
    let stdout = child
        .stdout
        .take()
        .context("could not read yt-dlp output")?;
    let stderr = child
        .stderr
        .take()
        .context("could not read yt-dlp errors")?;
    // Drain stderr concurrently: a verbose extractor error should never fill the OS pipe and
    // leave the progress reader waiting forever.
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut BufReader::new(stderr), &mut bytes).map(|_| bytes)
    });
    let progress = download_bar();
    let percent = Regex::new(r"\[download\]\s+([0-9]+(?:\.[0-9]+)?)%").unwrap();
    let mut files = Vec::<PathBuf>::new();

    for line in BufReader::new(stdout).lines() {
        let line = line.context("failed while reading download progress")?;
        if let Some(value) = percent
            .captures(&line)
            .and_then(|captures| captures.get(1))
            .and_then(|value| value.as_str().parse::<f64>().ok())
        {
            progress.set_position(value.round().clamp(0.0, 100.0) as u64);
        }
        if let Some(path) = line.strip_prefix(FILE_MARKER) {
            files.push(PathBuf::from(path));
        }
    }

    let status = child.wait().context("failed while waiting for yt-dlp")?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("failed to join the yt-dlp error reader"))?
        .context("failed to read yt-dlp errors")?;
    if !status.success() {
        progress.abandon_with_message("Download failed");
        bail!("{}", useful_error(&stderr));
    }

    progress.set_position(100);
    progress.finish_and_clear();
    if files.is_empty() {
        println!(
            "{} Download complete",
            Style::new().green().bold().apply_to("[done]")
        );
    } else {
        for path in files {
            println!(
                "{} Saved {}",
                Style::new().green().bold().apply_to("[done]"),
                Style::new().cyan().bold().apply_to(path.display())
            );
        }
    }
    Ok(())
}

fn add_cookie_args(command: &mut Command, cookies_from_browser: Option<&str>) {
    if let Some(browser) = cookies_from_browser {
        command.args(["--cookies-from-browser", browser]);
    }
}

fn spinner(message: &'static str) -> ProgressBar {
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::with_template("{spinner:.cyan.bright} {msg:.cyan.bright}")
            .expect("valid spinner style")
            .tick_strings(&["-", "\\", "|", "/"]),
    );
    bar.set_message(message);
    bar.enable_steady_tick(Duration::from_millis(80));
    bar
}

fn download_bar() -> ProgressBar {
    let bar = ProgressBar::new(100);
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.cyan.bright} {msg:.cyan.bright.bold} [{bar:32.cyan.bright/black.bright}] {pos:>3}%",
        )
        .expect("valid progress style")
        .progress_chars("=>-"),
    );
    bar.set_message("Downloading");
    bar.enable_steady_tick(Duration::from_millis(100));
    bar
}

fn useful_error(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr);
    let cleaned = message
        .lines()
        .map(|line| line.trim_start_matches("ERROR: "))
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if cleaned.is_empty() {
        "Instagram extraction failed. The post may be private or unavailable.".to_owned()
    } else if cleaned.contains("rate-limit reached or login required") {
        format!(
            "{cleaned}\n\nIf this is a public post, update yt-dlp first:\n  python3 -m pip install --user --upgrade --break-system-packages yt-dlp\nFor a private post, retry with:\n  instadown --cookies-from-browser firefox <url>"
        )
    } else {
        cleaned
    }
}

fn one_line(value: &str, max_chars: usize) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() <= max_chars {
        return value;
    }
    let shortened = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    format!("{shortened}...")
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.round().max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn compact_count(value: u64) -> String {
    match value {
        0..=999 => value.to_string(),
        1_000..=999_999 => format!("{:.1}K", value as f64 / 1_000.0),
        _ => format!("{:.1}M", value as f64 / 1_000_000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_instagram_urls() {
        assert!(validate_instagram_url("https://www.instagram.com/reel/ABC_123/").is_ok());
        assert!(validate_instagram_url("https://instagram.com/p/xyz/?igsh=123").is_ok());
    }

    #[test]
    fn rejects_unrelated_or_profile_urls() {
        assert!(validate_instagram_url("https://example.com/reel/ABC/").is_err());
        assert!(validate_instagram_url("https://instagram.com/some_user/").is_err());
    }

    #[test]
    fn truncates_metadata_cleanly() {
        assert_eq!(
            one_line("hello\n  friendly world", 20),
            "hello friendly world"
        );
        assert_eq!(one_line("123456789", 5), "12...");
    }

    #[test]
    fn formats_counts_and_duration() {
        assert_eq!(compact_count(999), "999");
        assert_eq!(compact_count(12_450), "12.4K");
        assert_eq!(format_duration(65.2), "1:05");
    }
}
