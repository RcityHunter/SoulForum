use crate::editor::RichEditControl;

pub fn render_editor(control: &RichEditControl) -> String {
    let mut html = String::new();
    html.push_str(&format!(
        "<textarea id=\"{}\" style=\"width:{};height:{}\">",
        control.id,
        control.width.as_deref().unwrap_or("100%"),
        control.height.as_deref().unwrap_or("250px")
    ));
    html.push_str(&control.value);
    html.push_str("</textarea>\n<div class=\"toolbar\">");
    for button in &control.buttons {
        html.push_str(&format!(
            "<button data-bbc=\"{}\">{}</button>",
            button, button
        ));
    }
    html.push_str("</div>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::RichEditControl;

    #[test]
    fn renders_buttons() {
        let html = render_editor(&RichEditControl {
            id: "editor".into(),
            bbc_enabled: true,
            smileys_enabled: true,
            value: "text".into(),
            buttons: vec!["bold".into()],
            width: Some("100%".into()),
            height: Some("200px".into()),
        });
        assert!(html.contains("bold"));
    }
}
