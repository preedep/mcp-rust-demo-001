use chrono::Utc;
use chrono_tz::Tz;
use serde_json::{json, Value};

use crate::domain::{DomainError, Tool, ToolAnnotations, ToolDescriptor, ToolOutput};

pub struct ServerTime;

impl Tool for ServerTime {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "get_server_time",
            description: "Get the current date and time from the server as an RFC 3339 \
                          timestamp. Always call this when asked what time it is — only \
                          the server knows its own clock, so do not answer from memory. \
                          Optionally convert to a named IANA time zone such as \
                          'Asia/Bangkok' or 'UTC'. Safe, instant and read-only.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "timezone": {
                        "type": "string",
                        "description": "IANA time zone name. Defaults to UTC."
                    }
                },
                "additionalProperties": false
            }),
            // Not idempotent: the answer changes between calls.
            annotations: ToolAnnotations {
                idempotent: false,
                ..ToolAnnotations::read_only()
            },
        }
    }

    fn invoke(&self, args: &Value) -> Result<ToolOutput, DomainError> {
        let now = Utc::now();
        match args.get("timezone").and_then(Value::as_str) {
            None => Ok(ToolOutput::ok(now.to_rfc3339())),
            Some(name) => match name.parse::<Tz>() {
                Ok(tz) => Ok(ToolOutput::ok(now.with_timezone(&tz).to_rfc3339())),
                Err(_) => Ok(ToolOutput::failed(format!(
                    "unknown time zone '{name}'; expected an IANA name such as 'UTC'"
                ))),
            },
        }
    }
}
