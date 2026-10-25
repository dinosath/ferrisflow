pub use super::class_merge::merge_classes;

pub fn classes<I, S>(parts: I) -> String
where
    I: IntoIterator<Item = Option<S>>,
    S: AsRef<str>,
{
    let mut output = String::new();

    for part in parts.into_iter().flatten() {
        for token in part.as_ref().split_whitespace() {
            if !output.is_empty() {
                output.push(' ');
            }

            output.push_str(token);
        }
    }

    output
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UiDensity {
    Compact,
    #[default]
    Comfortable,
    Touch,
}
