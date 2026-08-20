use crate::args::OutputFormat;
use homelab_api_model::{CompletenessStatus, OperationEnvelope, SeasonAvailability};
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};

const MAX_ROWS: usize = 20;
const MAX_COLUMNS: usize = 6;
const MAX_CELL_CHARS: usize = 40;

pub fn envelope<T: Serialize>(
    value: &OperationEnvelope<T>,
    output: OutputFormat,
) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    match output {
        OutputFormat::Json => write_json(&mut writer, value),
        OutputFormat::Table => {
            let value = serde_json::to_value(value).map_err(io::Error::other)?;
            write_table(&mut writer, &value)
        }
    }
}

pub fn availability_envelope(
    value: &OperationEnvelope<SeasonAvailability>,
    output: OutputFormat,
) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    match output {
        OutputFormat::Json => write_json(&mut writer, value),
        OutputFormat::Table => write_availability_table(&mut writer, value),
    }
}

pub fn structured(value: &Value, output: OutputFormat) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    match output {
        OutputFormat::Json => write_json(&mut writer, value),
        OutputFormat::Table => write_table(&mut writer, value),
    }
}

fn write_json(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, value).map_err(io::Error::other)?;
    writer.write_all(b"\n")
}

fn write_table(writer: &mut impl Write, envelope: &Value) -> io::Result<()> {
    metadata_row(writer, "OPERATION", envelope.get("operation"))?;
    metadata_row(writer, "REQUEST ID", envelope.get("request_id"))?;
    metadata_row(
        writer,
        "STATUS",
        Some(&Value::String(status(envelope).to_owned())),
    )?;
    metadata_row(writer, "SUMMARY", envelope.pointer("/summary/text"))?;

    if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
        metadata_row(writer, "ERROR", envelope.pointer("/error/code"))?;
        return Ok(());
    }


    match envelope.get("data") {
        Some(Value::Array(rows)) => write_array(writer, rows),
        Some(Value::Object(fields)) => {
            writer.write_all(b"\nFIELD | VALUE\n------|------\n")?;
            for (name, value) in fields
                .iter()
                .filter(|(name, _)| safe_field(name))
                .take(MAX_ROWS)
            {
                writeln!(
                    writer,
                    "{} | {}",
                    cell(&Value::String(name.clone())),
                    cell(value)
                )?;
            }
            Ok(())
        }
        Some(value) => writeln!(writer, "\n{}", cell(value)),
        None => Ok(()),
    }
}

fn write_availability_table(
    writer: &mut impl Write,
    envelope: &OperationEnvelope<SeasonAvailability>,
) -> io::Result<()> {
    let operation = Value::String(envelope.operation.clone());
    let request_id = Value::String(envelope.request_id.clone());
    let status = Value::String(if envelope.ok { "ok" } else { "error" }.to_owned());
    let summary = Value::String(envelope.summary.text.clone());
    metadata_row(writer, "OPERATION", Some(&operation))?;
    metadata_row(writer, "REQUEST ID", Some(&request_id))?;
    metadata_row(writer, "STATUS", Some(&status))?;
    metadata_row(writer, "SUMMARY", Some(&summary))?;

    if !envelope.ok {
        let error = envelope
            .error
            .as_ref()
            .map(|error| serde_json::to_value(&error.code).map_err(io::Error::other))
            .transpose()?;
        metadata_row(writer, "ERROR", error.as_ref())?;
        return Ok(());
    }

    let Some(data) = envelope.data.as_ref() else {
        return Ok(());
    };
    let next_airing = data
        .next_airing
        .as_ref()
        .and_then(|episode| {
            episode.air_date.as_ref().map(|air_date| {
                Value::String(format!("E{} {air_date}", episode.episode_number))
            })
        })
        .unwrap_or(Value::Null);
    let values = [
        cell(&Value::String(data.series.title.clone())),
        cell(&Value::from(data.season)),
        cell(&Value::Bool(data.in_library)),
        cell(&Value::String(
            completeness_status(data.aired.status).to_owned(),
        )),
        cell(&Value::String(
            completeness_status(data.announced.status).to_owned(),
        )),
        cell(&Value::from(data.announced.available_count)),
        cell(&Value::from(data.announced.expected_count)),
        cell(&next_airing),
    ];

    writer.write_all(
        b"\nTITLE | SEASON | IN_LIBRARY | AIRED | ANNOUNCED | AVAILABLE | EXPECTED | NEXT_AIRING\n",
    )?;
    writer.write_all(b"------|------|------|------|------|------|------|------\n")?;
    writeln!(writer, "{}", values.join(" | "))
}

fn completeness_status(status: CompletenessStatus) -> &'static str {
    match status {
        CompletenessStatus::Complete => "complete",
        CompletenessStatus::Incomplete => "incomplete",
        CompletenessStatus::Unknown => "unknown",
    }
}

fn metadata_row(writer: &mut impl Write, label: &str, value: Option<&Value>) -> io::Result<()> {
    writeln!(
        writer,
        "{label}: {}",
        value.map(cell).unwrap_or_else(|| "-".to_owned())
    )
}

fn status(envelope: &Value) -> &'static str {
    if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
        "error"
    } else {
        match envelope.pointer("/data/status").and_then(Value::as_str) {
            Some("degraded") => "partial",
            Some("unavailable") => "unavailable",
            _ => "ok",
        }
    }
}

fn write_array(writer: &mut impl Write, rows: &[Value]) -> io::Result<()> {
    let columns = rows
        .iter()
        .take(MAX_ROWS)
        .filter_map(Value::as_object)
        .flat_map(|row| row.keys())
        .filter(|name| safe_field(name))
        .fold(Vec::<String>::new(), |mut columns, name| {
            if columns.len() < MAX_COLUMNS && !columns.iter().any(|column| column == name) {
                columns.push(name.clone());
            }
            columns
        });

    if columns.is_empty() {
        for row in rows.iter().take(MAX_ROWS) {
            writeln!(writer, "{}", cell(row))?;
        }
        return Ok(());
    }

    writer.write_all(b"\n")?;
    writeln!(writer, "{}", columns.join(" | "))?;
    writeln!(
        writer,
        "{}",
        columns
            .iter()
            .map(|_| "------")
            .collect::<Vec<_>>()
            .join("|")
    )?;
    for row in rows.iter().take(MAX_ROWS) {
        let Some(row) = row.as_object() else {
            continue;
        };
        let values = columns
            .iter()
            .map(|column| row.get(column).map(cell).unwrap_or_else(|| "-".to_owned()))
            .collect::<Vec<_>>();
        writeln!(writer, "{}", values.join(" | "))?;
    }
    Ok(())
}

fn safe_field(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ![
        "credential",
        "password",
        "secret",
        "token",
        "api_key",
        "authorization",
    ]
    .iter()
    .any(|forbidden| name.contains(forbidden))
}

fn cell(value: &Value) -> String {
    let raw = match value {
        Value::Null => "-".to_owned(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    };
    let normalized = raw.replace(['\n', '\r', '\t'], " ");
    let mut characters = normalized.chars();
    let mut bounded = characters.by_ref().take(MAX_CELL_CHARS).collect::<String>();
    if characters.next().is_some() {
        bounded.push_str("...");
    }
    bounded
}
