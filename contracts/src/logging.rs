//! Structured logging (M0.6).
//!
//! Every line is one JSON object with a fixed schema. Fields that do not apply
//! to a given event are present as `null`, never missing, so downstream tooling
//! can rely on the shape. `request_id` is minted at the outermost boundary,
//! carried in a `tracing` span, and echoed over the wire as `X-Request-Id`.
//!
//! Redaction is enforced by the type system ([`crate::redaction::Secret`]); this
//! module only has to avoid calling `.expose()`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

/// The keys every log line carries, in order. Anything not set for an event is
/// emitted as JSON `null`.
pub const REQUIRED_FIELDS: &[&str] = &[
    "ts",
    "level",
    "component",
    "request_id",
    "operation_id",
    "file_id",
    "version_id",
    "chunk_id",
    "operation",
    "duration_ms",
    "result",
    "error_code",
    "msg",
];

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

static GUARD: OnceLock<Option<tracing_appender::non_blocking::WorkerGuard>> = OnceLock::new();

/// Where log lines go.
pub enum LogSink<'a> {
    /// `runtime/logs/<component>-YYYY-MM-DD.jsonl`, daily rotation.
    Directory(&'a Path),
    /// Standard error, one JSON object per line. Used by tests.
    Stderr,
    /// An in-memory buffer, for assertions in tests.
    Buffer(SharedWriter),
}

/// Initialise the global subscriber. Idempotent within a process; a second call
/// is a no-op so tests that each call it do not panic.
pub fn init(component: &'static str, sink: LogSink<'_>) {
    let (writer, guard): (SharedWriter, Option<_>) = match sink {
        LogSink::Directory(dir) => {
            let _ = std::fs::create_dir_all(dir);
            let file_appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix(component)
                .filename_suffix("jsonl")
                .build(dir)
                .expect("log directory is writable");
            let (nb, g) = tracing_appender::non_blocking(file_appender);
            (Arc::new(Mutex::new(Box::new(nb))), Some(g))
        }
        LogSink::Stderr => (Arc::new(Mutex::new(Box::new(std::io::stderr()))), None),
        LogSink::Buffer(w) => (w, None),
    };

    let layer = JsonLineLayer { component, writer };
    let _ = GUARD.set(guard);
    let _ = tracing_subscriber::registry().with(layer).try_init();
}

/// A shared in-memory buffer usable as a [`LogSink::Buffer`].
pub fn memory_sink() -> (SharedWriter, Arc<Mutex<Vec<u8>>>) {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let w: SharedWriter = Arc::new(Mutex::new(Box::new(BufHandle(buf.clone()))));
    (w, buf)
}

struct BufHandle(Arc<Mutex<Vec<u8>>>);
impl Write for BufHandle {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct JsonLineLayer {
    component: &'static str,
    writer: SharedWriter,
}

#[derive(Default)]
struct FieldBag(BTreeMap<String, Value>);

impl Visit for FieldBag {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(
            field.name().to_string(),
            Value::String(format!("{value:?}")),
        );
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0
            .insert(field.name().to_string(), Value::String(value.to_string()));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().to_string(), Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_string(), Value::from(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().to_string(), Value::from(value));
    }
}

impl<S> Layer<S> for JsonLineLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let mut bag = FieldBag::default();
        attrs.record(&mut bag);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(bag);
        }
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        if let Some(span) = ctx.span(id) {
            let mut ext = span.extensions_mut();
            if let Some(bag) = ext.get_mut::<FieldBag>() {
                values.record(bag);
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut merged: BTreeMap<String, Value> = BTreeMap::new();

        // span stack, outermost first
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(bag) = span.extensions().get::<FieldBag>() {
                    for (k, v) in &bag.0 {
                        merged.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        // event fields (win over span fields)
        let mut bag = FieldBag::default();
        event.record(&mut bag);
        for (k, v) in bag.0 {
            merged.insert(k, v);
        }

        // `msg` comes from a format string (`message`) or an explicit `msg` field.
        let msg = merged
            .remove("message")
            .or_else(|| merged.remove("msg"))
            .unwrap_or(Value::Null);

        let mut line = serde_json::Map::new();
        for &key in REQUIRED_FIELDS {
            let value = match key {
                "ts" => Value::String(chrono::Utc::now().to_rfc3339()),
                "level" => Value::String(event.metadata().level().to_string()),
                "component" => Value::String(self.component.to_string()),
                "msg" => msg.clone(),
                other => merged.remove(other).unwrap_or(Value::Null),
            };
            line.insert(key.to_string(), value);
        }
        // any extra structured fields are kept, but never displace the schema
        for (k, v) in merged {
            line.entry(k).or_insert(v);
        }

        if let Ok(mut w) = self.writer.lock() {
            let _ = serde_json::to_writer(&mut *w, &Value::Object(line));
            let _ = w.write_all(b"\n");
            let _ = w.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_field_list_is_the_documented_thirteen() {
        assert_eq!(REQUIRED_FIELDS.len(), 13);
        assert!(REQUIRED_FIELDS.contains(&"request_id"));
        assert!(REQUIRED_FIELDS.contains(&"error_code"));
    }
}
