use serde_json::Value;
use std::fmt::Write;

pub fn render_alert_menu(alerts: &[Value], unread_count: usize) -> String {
    let mut html = String::new();
    writeln!(
        html,
        "<div id=\"alerts_menu\"><button class=\"alert_button\">Alerts <span class=\"badge\">{}</span></button><div class=\"alerts_unread\">",
        unread_count
    )
    .ok();

    if alerts.is_empty() {
        html.push_str("<div class=\"no_unread\">No new alerts</div>");
    } else {
        for alert in alerts {
            let text = alert.get("text").and_then(Value::as_str).unwrap_or("");
            let time = alert.get("time").and_then(Value::as_str).unwrap_or("");
            writeln!(
                html,
                "<div class=\"alert\"><span class=\"alert_text\">{}</span> <span class=\"alert_time\">{}</span></div>",
                text,
                time
            )
            .ok();
        }
    }

    html.push_str("</div></div>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_alerts() {
        let html = render_alert_menu(&[json!({"text": "Mentioned you", "time": "Now"})], 1);
        assert!(html.contains("Mentioned you"));
    }
}
