use anyhow::Result;
use nu_ansi_term::Color;
use std::fmt;
use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;

#[cfg(debug_assertions)]
const FIELD_INDENT: &str = "    ";

#[cfg(debug_assertions)]
const CONTINUATION_INDENT: &str = "    ";

#[cfg(debug_assertions)]
pub struct IndentedFormatter;

#[cfg(debug_assertions)]
impl<S, N> FormatEvent<S, N> for IndentedFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        let level = metadata.level();

        let level_color = match *level {
            Level::ERROR => Color::Red,
            Level::WARN => Color::Yellow,
            Level::INFO => Color::Green,
            Level::DEBUG => Color::Cyan,
            Level::TRACE => Color::Purple,
        };

        write!(
            writer,
            "{} ",
            Color::White
                .dimmed()
                .paint(chrono::Utc::now().format("%H:%M:%S%.3fZ").to_string())
        )?;

        let level_str = format!("{level:>5}");
        write!(writer, "{} ", level_color.paint(level_str))?;

        write!(
            writer,
            "{}: ",
            Color::White.dimmed().paint(metadata.target())
        )?;

        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);

        if let Some(message) = visitor.message {
            writeln!(writer, "{message}")?;
        } else {
            writeln!(writer)?;
        }

        if let (Some(file), Some(line)) = (metadata.file(), metadata.line()) {
            let location = format!("at {file}:{line}");
            writeln!(
                writer,
                "{FIELD_INDENT}{}",
                Color::White.dimmed().paint(location)
            )?;
        }

        for (key, value) in visitor.fields {
            if key != "message" {
                write!(
                    writer,
                    "{FIELD_INDENT}{}: ",
                    Color::White.dimmed().paint(&key)
                )?;
                let is_multiline = Self::format_field_value(&mut writer, &value)?;
                if !is_multiline {
                    writeln!(writer)?;
                }
            }
        }

        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                let extensions = span.extensions();
                if let Some(span_visitor) = extensions.get::<FieldVisitor>() {
                    // Format span fields using our custom formatting
                    for (key, value) in &span_visitor.fields {
                        write!(
                            writer,
                            "{FIELD_INDENT}{}: ",
                            Color::White.dimmed().paint(key)
                        )?;
                        let is_multiline = Self::format_field_value(&mut writer, value)?;
                        if !is_multiline {
                            writeln!(writer)?;
                        }
                    }
                }
            }
        }

        writeln!(writer)?;

        Ok(())
    }
}

#[cfg(debug_assertions)]
impl IndentedFormatter {
    fn format_field_value(writer: &mut Writer<'_>, value: &str) -> Result<bool, fmt::Error> {
        let lines: Vec<&str> = value.lines().collect();
        if lines.len() > 1 {
            let first_line = lines[0];
            writeln!(writer, "{first_line}")?;
            for line in &lines[1..] {
                writeln!(writer, "{CONTINUATION_INDENT}{line}")?;
            }
            Ok(true)
        } else {
            write!(writer, "{value}")?;
            Ok(false)
        }
    }
}

#[cfg(debug_assertions)]
struct FieldVisitor {
    fields: Vec<(String, String)>,
    message: Option<String>,
}

#[cfg(debug_assertions)]
struct SpanFieldLayer;

#[cfg(debug_assertions)]
impl<S> tracing_subscriber::Layer<S> for SpanFieldLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        #[allow(clippy::expect_used)]
        let span = ctx.span(id).expect("Span not found, this is a bug");
        let mut visitor = FieldVisitor::new();
        attrs.record(&mut visitor);

        let mut extensions = span.extensions_mut();
        extensions.insert(visitor);
    }
}

#[cfg(debug_assertions)]
impl FieldVisitor {
    fn new() -> Self {
        Self {
            fields: Vec::new(),
            message: None,
        }
    }
}

#[cfg(debug_assertions)]
impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let name = field.name();
        let value_str = format!("{:#?}", value);

        if name == "message" {
            self.message = Some(value_str.trim_matches('"').to_string());
        } else {
            self.fields.push((name.to_string(), value_str));
        }
    }
}

/// Initialize tracing-based logging to stdout
pub fn setup_tracing() -> Result<()> {
    // Set up environment filter
    // Default to INFO level, but allow override with RUST_LOG environment variable
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("room_101=info,iroh=error,iroh_gossip=error"));

    #[cfg(debug_assertions)]
    {
        // Use custom indented formatter with span field capture layer in debug builds
        let registry = tracing_subscriber::registry()
            .with(SpanFieldLayer)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_thread_ids(false)
                    .with_file(false)
                    .with_line_number(false)
                    .event_format(IndentedFormatter),
            )
            .with(env_filter);

        tracing::subscriber::set_global_default(registry)?;
    }

    #[cfg(not(debug_assertions))]
    {
        // Use compact formatter in release builds
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .compact()
            .init();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_setup() {
        let result = setup_tracing();
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_custom_formatter_basic() {
        let _formatter = IndentedFormatter;
    }
}
