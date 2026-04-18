use crate::services::ForumContext;

pub fn prepare_action_context(ctx: &mut ForumContext, action: &str) {
    ctx.context.set("current_action", action);

    let simple_actions = ["findmember", "helpadmin", "printpage"];
    let mut simple = simple_actions.contains(&action);

    if !simple {
        simple |= action == "profile"
            && matches!(ctx.request.string("area"), Some(area) if matches!(area.as_str(), "popup" | "alerts_popup"));
    }

    if !simple {
        if action == "pm" {
            if let Some(sa) = ctx.request.string("sa") {
                if sa == "popup" {
                    simple = true;
                }
            }
        } else if action == "signup" {
            if let Some(sa) = ctx.request.string("sa") {
                if sa == "usernamecheck" {
                    simple = true;
                }
            }
        }
    }

    let extra_params = ["preview", "splitjs"];
    let requires_xml = extra_params.iter().any(|param| ctx.request.contains(param));
    let xml_actions = [
        "quotefast",
        "jsmodify",
        "xmlhttp",
        "post2",
        "suggest",
        "stats",
        "notifytopic",
        "notifyboard",
    ];
    let wants_xml = ctx.request.contains("xml") && (xml_actions.contains(&action) || requires_xml);

    if wants_xml {
        ctx.context.set("simple_action", true);
        ctx.context.set("template_layers", Vec::<String>::new());
        ctx.context.set("xml_output", true);
        return;
    }

    if simple {
        ctx.context.set("simple_action", true);
        ctx.context.set("template_layers", Vec::<String>::new());
    } else if !ctx.context.contains("template_layers") {
        ctx.context
            .set("template_layers", vec!["index".to_string()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ForumContext;

    #[test]
    fn notify_actions_keep_default_layers() {
        let mut ctx = ForumContext::default();
        prepare_action_context(&mut ctx, "notifyboard");
        assert!(!ctx.context.bool("simple_action"));
        let layers = ctx
            .context
            .get("template_layers")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(layers.len(), 1);
    }

    #[test]
    fn xml_requests_flagged() {
        let mut ctx = ForumContext::default();
        ctx.request.set("xml", true);
        prepare_action_context(&mut ctx, "notifytopic");
        assert!(ctx.context.bool("xml_output"));
    }
}
