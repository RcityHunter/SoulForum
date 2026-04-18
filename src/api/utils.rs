pub(crate) fn sanitize_input(input: &str) -> String {
    ammonia::Builder::default()
        .url_schemes(["http", "https"].into())
        .clean(input)
        .to_string()
}

pub(crate) fn sanitize_filename(name: &str) -> String {
    let cleaned = name.replace(['/', '\\'], "_");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "upload".to_string()
    } else {
        cleaned.to_string()
    }
}
