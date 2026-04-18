use btc_forum_shared::PointsBalance;
use dioxus::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointsSnapshot {
    pub karma: Option<i32>,
    pub merit: Option<i32>,
    pub trust_level: Option<String>,
    pub backend_ready: bool,
}

impl PointsSnapshot {
    pub fn pending() -> Self {
        Self {
            karma: None,
            merit: None,
            trust_level: None,
            backend_ready: false,
        }
    }

    pub fn seeded(seed: &str) -> Self {
        let normalized = seed.trim().to_lowercase();
        if normalized.is_empty() {
            return Self::pending();
        }
        let hash = normalized.bytes().fold(0u32, |acc, byte| {
            acc.wrapping_mul(33).wrapping_add(byte as u32)
        });
        Self {
            karma: Some((hash % 240) as i32 + 12),
            merit: Some((hash % 18) as i32),
            trust_level: Some(
                match hash % 4 {
                    0 => "Newcomer",
                    1 => "Builder",
                    2 => "Regular",
                    _ => "OG",
                }
                .to_string(),
            ),
            backend_ready: false,
        }
    }

    pub fn from_balance(balance: &PointsBalance) -> Self {
        Self {
            karma: i32::try_from(balance.karma).ok(),
            merit: i32::try_from(balance.merit).ok(),
            trust_level: Some(rank_label(balance.karma, balance.merit).to_string()),
            backend_ready: true,
        }
    }
}

fn rank_label(karma: i64, merit: i64) -> &'static str {
    if karma >= 300 && merit >= 3 {
        "OG"
    } else if karma >= 100 && merit >= 1 {
        "Regular"
    } else if karma >= 20 {
        "Builder"
    } else {
        "Newcomer"
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct PointsBadgeProps {
    pub snapshot: PointsSnapshot,
    #[props(optional)]
    pub compact: Option<bool>,
}

pub fn PointsBadge(props: PointsBadgeProps) -> Element {
    let compact = props.compact.unwrap_or(false);
    let snapshot = props.snapshot;
    let karma = snapshot
        .karma
        .map(|value| value.to_string())
        .unwrap_or_else(|| "--".to_string());
    let merit = snapshot
        .merit
        .map(|value| value.to_string())
        .unwrap_or_else(|| "--".to_string());
    let trust = snapshot
        .trust_level
        .unwrap_or_else(|| "Pending backend".to_string());

    rsx! {
        div { class: if compact { "points-badge points-badge--compact" } else { "points-badge" },
            div { class: "points-badge__metric",
                span { class: "points-badge__label", "Karma" }
                strong { "{karma}" }
            }
            div { class: "points-badge__metric",
                span { class: "points-badge__label", "Merit" }
                strong { "{merit}" }
            }
            div { class: "points-badge__meta",
                span { class: "points-badge__trust", "{trust}" }
                span { class: if snapshot.backend_ready { "points-badge__state is-ready" } else { "points-badge__state is-pending" },
                    if snapshot.backend_ready { "live" } else { "preview" }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct PointsEntryProps {
    pub title: String,
    pub snapshot: PointsSnapshot,
    pub hint: String,
    pub action_label: String,
    pub on_action: EventHandler<()>,
    #[props(optional)]
    pub compact: Option<bool>,
}

pub fn PointsEntry(props: PointsEntryProps) -> Element {
    rsx! {
        div { class: if props.compact.unwrap_or(false) { "points-entry points-entry--compact" } else { "points-entry" },
            div { class: "points-entry__header",
                strong { "{props.title}" }
                PointsBadge { snapshot: props.snapshot.clone(), compact: Some(props.compact.unwrap_or(false)) }
            }
            p { class: "points-entry__hint", "{props.hint}" }
            button { class: "ghost-btn", onclick: move |_| props.on_action.call(()), "{props.action_label}" }
        }
    }
}
