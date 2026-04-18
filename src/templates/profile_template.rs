use std::fmt::Write;

#[derive(Clone, Debug, Default)]
pub struct AlertPreferencePage {
    pub description: String,
    pub show_notify_once: bool,
    pub notify_once_enabled: bool,
    pub alert_timeout: Option<u32>,
    pub groups: Vec<AlertPreferenceGroup>,
}

#[derive(Clone, Debug, Default)]
pub struct AlertPreferenceGroup {
    pub id: String,
    pub label: String,
    pub options: Vec<GroupToggleOption>,
    pub alerts: Vec<AlertPreference>,
}

#[derive(Clone, Debug, Default)]
pub struct GroupToggleOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct AlertPreference {
    pub id: String,
    pub label: String,
    pub help_link: Option<String>,
    pub channels: Vec<AlertChannelPreference>,
}

#[derive(Clone, Debug, Default)]
pub struct AlertChannelPreference {
    pub channel: AlertChannel,
    pub enabled: bool,
    pub allowed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlertChannel {
    #[default]
    Alert,
    Email,
    Push,
}

impl AlertChannel {
    fn label(&self) -> &'static str {
        match self {
            AlertChannel::Alert => "alert",
            AlertChannel::Email => "email",
            AlertChannel::Push => "push",
        }
    }
}

pub fn render_alert_preferences(page: &AlertPreferencePage) -> String {
    let mut html = String::new();
    writeln!(
        html,
        "<section class=\"alert-preferences\"><h2>Alert Preferences</h2><p>{}</p>",
        page.description
    )
    .ok();

    if page.show_notify_once {
        writeln!(
            html,
            "<label><input type=\"checkbox\" name=\"notify_once\" {}> ",
            if page.notify_once_enabled {
                "checked"
            } else {
                ""
            }
        )
        .ok();
    }

    if let Some(timeout) = page.alert_timeout {
        writeln!(
            html,
            "<div class=\"alert-timeout\"><label>Alert timeout <input type=\"number\" name=\"alert_timeout\" value=\"{}\" min=\"0\" max=\"127\"></label></div>",
            timeout
        )
        .ok();
    }

    for group in &page.groups {
        writeln!(
            html,
            "<div class=\"alert-group\" id=\"group-{}\"><h3>{}</h3>",
            group.id, group.label
        )
        .ok();

        if !group.options.is_empty() {
            html.push_str("<ul class=\"group-options\">");
            for option in &group.options {
                writeln!(
                    html,
                    "<li><label><input type=\"checkbox\" name=\"{}\" {}> {}{}</label></li>",
                    option.id,
                    if option.enabled { "checked" } else { "" },
                    option.label,
                    option
                        .description
                        .as_ref()
                        .map(|desc| format!("<span class=\"help\">{}</span>", desc))
                        .unwrap_or_default(),
                )
                .ok();
            }
            html.push_str("</ul>");
        }

        html.push_str("<table class=\"alert-grid\"><thead><tr><th>Alert</th><th>On Site</th><th>Email</th><th>Push</th></tr></thead><tbody>");
        for alert in &group.alerts {
            writeln!(
                html,
                "<tr><td>{}{}",
                alert.label,
                alert
                    .help_link
                    .as_ref()
                    .map(|link| format!(" <a href=\"{}\" class=\"help\">?</a>", link))
                    .unwrap_or_default()
            )
            .ok();
            for channel in &alert.channels {
                let disabled = if channel.allowed { "" } else { "disabled" };
                let checked = if channel.enabled { "checked" } else { "" };
                writeln!(
                    html,
                    "<td><input type=\"checkbox\" name=\"{}_{}\" {} {}/></td>",
                    alert.id,
                    channel.channel.label(),
                    checked,
                    disabled
                )
                .ok();
            }
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table></div>");
    }

    html.push_str("</section>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_group_and_options() {
        let page = AlertPreferencePage {
            description: "Control how alerts are delivered.".into(),
            show_notify_once: true,
            notify_once_enabled: true,
            alert_timeout: Some(30),
            groups: vec![AlertPreferenceGroup {
                id: "mentions".into(),
                label: "Mentions".into(),
                options: vec![GroupToggleOption {
                    id: "mentions_notify".into(),
                    label: "Notify for mentions".into(),
                    description: None,
                    enabled: true,
                }],
                alerts: vec![AlertPreference {
                    id: "mention".into(),
                    label: "When someone mentions me".into(),
                    help_link: None,
                    channels: vec![
                        AlertChannelPreference {
                            channel: AlertChannel::Alert,
                            enabled: true,
                            allowed: true,
                        },
                        AlertChannelPreference {
                            channel: AlertChannel::Email,
                            enabled: false,
                            allowed: true,
                        },
                        AlertChannelPreference {
                            channel: AlertChannel::Push,
                            enabled: false,
                            allowed: false,
                        },
                    ],
                }],
            }],
        };
        let html = render_alert_preferences(&page);
        assert!(html.contains("Mentions"));
        assert!(html.contains("mention_email"));
    }
}
