use crate::args::OutputFormat;
use homelab_api_model::OperationEnvelope;
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};

const MAX_ROWS: usize = 20;
const MAX_COLUMNS: usize = 6;
const MAX_CELL_CHARS: usize = 40;

pub fn envelope<T: Serialize>(value: &OperationEnvelope<T>, output: OutputFormat) -> io::Result<()> {
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
    metadata_row(writer, "STATUS", Some(&Value::String(status(envelope).to_owned())))?;
    metadata_row(writer, "SUMMARY", envelope.pointer("/summary/text"))?;

    if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
        metadata_row(writer, "ERROR", envelope.pointer("/error/code"))?;
        return Ok(());
    }

    match envelope.get("data") {
        Some(Value::Array(rows)) => write_array(writer, rows),
        Some(Value::Object(fields)) => {
            writer.write_all(b"\nFIELD | VALUE\n------|------\n")?;
            for (name, value) in fields.iter().filter(|(name, _)| safe_field(name)).take(MAX_ROWS) {
                writeln!(writer, "{} | {}", cell(&Value::String(name.clone())), cell(value))?;
            }
            Ok(())
        }
        Some(value) => writeln!(writer, "\n{}", cell(value)),
        None => Ok(()),
    }
}

fn metadata_row(writer: &mut impl Write, label: &str, value: Option<&Value>) -> io::Result<()> {
    writeln!(writer, "{label}: {}", value.map(cell).unwrap_or_else(|| "-".to_owned()))
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
    writeln!(writer, "{}", columns.iter().map(|_| "------").collect::<Vec<_>>().join("|"))?;
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
    !["credential", "password", "secret", "token", "api_key", "authorization"]
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
