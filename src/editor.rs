use crate::services::{ForumContext, ForumError, ServiceResult};
use serde_json::json;

#[derive(Clone, Debug)]
pub struct RichEditOptions {
    pub id: String,
    pub value: String,
    pub width: Option<String>,
    pub height: Option<String>,
    pub allow_bbc: bool,
    pub allow_smileys: bool,
    pub preview: bool,
}

impl Default for RichEditOptions {
    fn default() -> Self {
        Self {
            id: "post_box".into(),
            value: String::new(),
            width: None,
            height: None,
            allow_bbc: true,
            allow_smileys: true,
            preview: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RichEditControl {
    pub id: String,
    pub bbc_enabled: bool,
    pub smileys_enabled: bool,
    pub value: String,
    pub buttons: Vec<String>,
    pub width: Option<String>,
    pub height: Option<String>,
}

pub fn create_control_richedit(
    ctx: &mut ForumContext,
    options: RichEditOptions,
) -> ServiceResult<RichEditControl> {
    let control = RichEditControl {
        id: options.id.clone(),
        bbc_enabled: options.allow_bbc,
        smileys_enabled: options.allow_smileys,
        value: options.value,
        buttons: default_buttons(options.allow_bbc),
        width: options.width,
        height: options.height,
    };

    ctx.context.set(
        "richedit",
        json!({
            "id": control.id,
            "bbc": control.bbc_enabled,
            "smileys": control.smileys_enabled,
            "value": control.value,
            "buttons": control.buttons,
            "width": control.width,
            "height": control.height,
        }),
    );

    if options.preview {
        ctx.context.set("show_preview", true);
    }

    Ok(control)
}

fn default_buttons(allow_bbc: bool) -> Vec<String> {
    if allow_bbc {
        vec![
            "bold".into(),
            "italic".into(),
            "underline".into(),
            "url".into(),
            "img".into(),
            "quote".into(),
            "code".into(),
        ]
    } else {
        vec!["plain".into()]
    }
}

#[derive(Clone, Debug)]
pub struct VerificationOptions {
    pub id: String,
    pub require_captcha: bool,
}

impl Default for VerificationOptions {
    fn default() -> Self {
        Self {
            id: "post".into(),
            require_captcha: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VerificationState {
    pub id: String,
    pub passed: bool,
}

pub fn create_control_verification(
    ctx: &mut ForumContext,
    options: VerificationOptions,
    do_test: bool,
) -> ServiceResult<VerificationState> {
    if !options.require_captcha {
        let key = format!("verification_{}", options.id);
        ctx.context.set(&key, json!({"passed": true}));
        return Ok(VerificationState {
            id: options.id,
            passed: true,
        });
    }

    if do_test && ctx.session.bool("captcha_failed") {
        return Err(ForumError::Validation("captcha_failed".into()));
    }

    let key = format!("verification_{}", options.id);
    ctx.context.set(&key, json!({"passed": !do_test }));
    Ok(VerificationState {
        id: options.id,
        passed: !do_test,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ForumContext;

    #[test]
    fn rich_edit_defaults() {
        let mut ctx = ForumContext::default();
        let control = create_control_richedit(&mut ctx, RichEditOptions::default()).unwrap();
        assert_eq!(control.id, "post_box");
        assert_eq!(control.buttons.len(), 7);
    }

    #[test]
    fn verification_passes_without_captcha() {
        let mut ctx = ForumContext::default();
        let result =
            create_control_verification(&mut ctx, VerificationOptions::default(), false).unwrap();
        assert!(result.passed);
    }
}
