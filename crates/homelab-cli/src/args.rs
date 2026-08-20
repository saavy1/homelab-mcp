use clap::{Parser, Subcommand, ValueEnum, builder::NonEmptyStringValueParser};
use homelab_api_model::MediaType;
use reqwest::header::HeaderValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Json,
    Table,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum MediaTypeArg {
    Movie,
    Tv,
}

impl From<MediaTypeArg> for MediaType {
    fn from(value: MediaTypeArg) -> Self {
        match value {
            MediaTypeArg::Movie => Self::Movie,
            MediaTypeArg::Tv => Self::Tv,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "homelab",
    about = "Curated homelab operations",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[arg(long, value_enum, default_value_t = OutputFormat::Json, global = true)]
    pub output: OutputFormat,

    #[arg(long, value_parser = parse_correlation_id)]
    pub request_id: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

fn parse_correlation_id(value: &str) -> Result<String, String> {
    if value.is_empty() || value.parse::<HeaderValue>().is_err() {
        return Err("request ID must be a non-empty HTTP header value".to_owned());
    }
    Ok(value.to_owned())
}

fn parse_catalog_id(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("item ID must be a non-empty numeric catalog identifier".to_owned());
    }
    Ok(value.to_owned())
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Capabilities,
    Media {
        #[command(subcommand)]
        command: MediaCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum MediaCommand {
    Health,
    Search {
        #[arg(long, value_parser = NonEmptyStringValueParser::new())]
        query: String,
    },
    Item {
        #[command(subcommand)]
        command: ItemCommand,
    },
    Request {
        #[command(subcommand)]
        command: RequestCommand,
    },
    Requests {
        #[command(subcommand)]
        command: RequestsCommand,
    },
    Downloads {
        #[command(subcommand)]
        command: DownloadsCommand,
    },
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    Sessions {
        #[command(subcommand)]
        command: SessionsCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ItemCommand {
    Show {
        #[arg(long, value_parser = parse_catalog_id)]
        item_id: String,
        #[arg(long, value_enum)]
        media_type: MediaTypeArg,
    },
}

#[derive(Debug, Subcommand)]
pub enum RequestCommand {
    Create {
        #[arg(long)]
        media_id: i64,
        #[arg(long, value_enum)]
        media_type: MediaTypeArg,
    },
}

#[derive(Debug, Subcommand)]
pub enum RequestsCommand {
    List {
        #[arg(long, value_parser = NonEmptyStringValueParser::new())]
        status: Option<String>,
    },
    Approve {
        #[arg(long, value_parser = NonEmptyStringValueParser::new())]
        request_id: String,
    },
    Decline {
        #[arg(long, value_parser = NonEmptyStringValueParser::new())]
        request_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum DownloadsCommand {
    List {
        #[arg(long, value_parser = NonEmptyStringValueParser::new())]
        status: Option<String>,
    },
    Pause {
        #[arg(long, value_parser = NonEmptyStringValueParser::new())]
        download_id: String,
    },
    Resume {
        #[arg(long, value_parser = NonEmptyStringValueParser::new())]
        download_id: String,
    },
    Delete {
        #[arg(long, value_parser = NonEmptyStringValueParser::new())]
        download_id: String,
        #[arg(long)]
        delete_files: bool,
    },
    Retry {
        #[arg(long, value_parser = NonEmptyStringValueParser::new())]
        download_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum LibraryCommand {
    Status,
    Refresh,
}

#[derive(Debug, Subcommand)]
pub enum SessionsCommand {
    List,
}

impl Command {
    pub fn operation_and_risk(&self) -> (&'static str, &'static str) {
        match self {
            Self::Capabilities => ("capabilities.show", "read"),
            Self::Media { command } => command.operation_and_risk(),
        }
    }
}

impl MediaCommand {
    fn operation_and_risk(&self) -> (&'static str, &'static str) {
        match self {
            Self::Health => ("media.health", "read"),
            Self::Search { .. } => ("media.search", "read"),
            Self::Item { .. } => ("media.items.show", "read"),
            Self::Request { .. } => ("media.requests.create", "write"),
            Self::Requests { command } => match command {
                RequestsCommand::List { .. } => ("media.requests.list", "read"),
                RequestsCommand::Approve { .. } => ("media.requests.approve", "write"),
                RequestsCommand::Decline { .. } => ("media.requests.decline", "write"),
            },
            Self::Downloads { command } => match command {
                DownloadsCommand::List { .. } => ("media.downloads.list", "read"),
                DownloadsCommand::Pause { .. } => ("media.downloads.pause", "write"),
                DownloadsCommand::Resume { .. } => ("media.downloads.resume", "write"),
                DownloadsCommand::Delete { delete_files, .. } => {
                    if *delete_files {
                        ("media.downloads.delete", "destructive")
                    } else {
                        ("media.downloads.delete", "write")
                    }
                }
                DownloadsCommand::Retry { .. } => ("media.downloads.retry", "write"),
            },
            Self::Library { command } => match command {
                LibraryCommand::Status => ("media.library.status", "read"),
                LibraryCommand::Refresh => ("media.library.refresh", "write"),
            },
            Self::Sessions { .. } => ("media.sessions.list", "read"),
        }
    }
}
