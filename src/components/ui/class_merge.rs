//! Merges a user class into a component's classes, as the last class wins
//! (RFC 0076). Each utility is classified into the longhand slots it sets
//! by data generated from Tailwind; a component utility is dropped when a
//! user utility under the same variants and importance sets all of them. A
//! token the data cannot classify, such as an app's own class or theme name,
//! removes nothing and is never removed, and debug builds say so.
//!
//! Tailwind scans this file in apps that copy it, so it names no utility
//! but `static`, the keyword the debug report needs.

use dioxus::logger::tracing;

use super::class_merge_table::{GROUPS, HINTS, Kind, PROPERTIES, ROOTS, SETS, STATIC, Value};

struct Utility<'a> {
    variants: &'a str,
    important: bool,
    slots: &'a [u16],
}

fn classify(token: &str) -> Option<Utility<'_>> {
    let (variants, utility) = match last_top_level_colon(token) {
        Some(index) => (&token[..index], &token[index + 1..]),
        None => ("", token),
    };
    let (utility, important) = match utility.strip_suffix('!').or_else(|| utility.strip_prefix('!'))
    {
        Some(rest) => (rest, true),
        None => (utility, false),
    };
    Some(Utility { variants, important, slots: lookup(utility)? })
}

/// Appends the user class to the component classes and drops each component
/// utility that a user utility replaces: one that sets every longhand the
/// component utility sets, under the same variants and importance. A user
/// utility that sets only some of them, such as horizontal padding over all
/// padding, leaves both in place.
pub fn merge_classes(component: String, user: &str) -> String {
    if user.split_whitespace().next().is_none() {
        return component;
    }
    let users = user
    .split_whitespace()
    .filter_map(|token| {
      let utility = classify(token);
      if utility.is_none() {
        report(format!("`{token}` is not a Tailwind utility the class merge knows, so it replaces no component class"));
      }
      utility.map(|utility| (token, utility))
    })
    .collect::<Vec<_>>();
    let mut merged = String::with_capacity(component.len() + user.len() + 1);
    let kept = component.split_whitespace().filter(|token| {
        let Some(own) = classify(token) else {
            return true;
        };
        let replacement = users.iter().find(|(_, other)| {
            other.variants == own.variants
                && other.important == own.important
                && is_subset(own.slots, other.slots)
        });
        if let Some((replacement, _)) = replacement {
            report(format!("`{replacement}` replaces the component class `{token}`"));
        }
        replacement.is_none()
    });
    for token in kept.chain(user.split_whitespace()) {
        if !merged.is_empty() {
            merged.push(' ');
        }
        merged.push_str(token);
    }
    merged
}

/// Logs each distinct merge decision once, in debug builds, so an override
/// that did not apply can be traced without reading the stylesheet.
fn report(message: String) {
    #[cfg(debug_assertions)]
    {
        use std::collections::BTreeSet;
        use std::sync::Mutex;

        static REPORTED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
        if let Ok(mut reported) = REPORTED.lock()
            && reported.insert(message.clone())
        {
            tracing::debug!("dioxus-shadcn class merge (RFC 0076): {message}");
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = message;
}

/// Compares a stored name, which the generated data keeps reversed so
/// Tailwind's source scan finds no class names in it, with a name read
/// forward.
fn reversed_cmp(stored: &str, name: &str) -> std::cmp::Ordering {
    stored.bytes().cmp(name.bytes().rev())
}

fn is_subset(inner: &[u16], outer: &[u16]) -> bool {
    let mut outer = outer.iter();
    inner.iter().all(|slot| outer.any(|other| other == slot))
}

fn last_top_level_colon(token: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut last = None;
    for (index, byte) in token.bytes().enumerate() {
        match byte {
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth -= 1,
            b':' if depth == 0 => last = Some(index),
            _ => {}
        }
    }
    last
}

fn lookup(utility: &str) -> Option<&[u16]> {
    if let Ok(index) = STATIC.binary_search_by(|(name, _)| reversed_cmp(name, utility)) {
        return Some(SETS[STATIC[index].1 as usize]);
    }
    if let Some(inner) = utility.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        let (property, _) = inner.split_once(':')?;
        let index = PROPERTIES.binary_search_by(|(name, _)| reversed_cmp(name, property)).ok()?;
        return Some(SETS[PROPERTIES[index].1 as usize]);
    }

    // Roots from the longest, so a root with a dash in it wins over the root
    // before that dash. Dashes inside an arbitrary value do not split.
    let named_end = utility.find(['[', '(']).unwrap_or(utility.len());
    let splits = std::iter::once(utility.len()).chain(
        utility[..named_end].rmatch_indices('-').map(|(index, _)| index).filter(|index| *index > 0),
    );
    for split in splits {
        let root = &utility[..split];
        let Ok(index) = ROOTS.binary_search_by(|(name, _)| reversed_cmp(name, root)) else {
            continue;
        };
        let value = utility.get(split + 1..).unwrap_or("");
        if let Some(set) = match_value(ROOTS[index].1, value) {
            return Some(SETS[set as usize]);
        }
    }
    None
}

fn match_value(rules: &[(Value, u16)], value: &str) -> Option<u16> {
    let form = value_form(value)?;
    rules.iter().find_map(|(rule, set)| {
        let matches = match (rule, form) {
            (Value::Keyword(group), Form::Named(name)) => GROUPS[*group as usize]
                .binary_search_by(|stored| reversed_cmp(stored, name))
                .is_ok(),
            (Value::Arbitrary, Form::Arbitrary(_)) => true,
            (rule, Form::Arbitrary(Some(value))) => *rule == value,
            (rule, Form::Plain(value)) => *rule == value,
            _ => false,
        };
        matches.then_some(*set)
    })
}

#[derive(Clone, Copy)]
enum Form<'a> {
    Plain(Value),
    Named(&'a str),
    /// An arbitrary value and its form, or `None` for one of no inferable kind.
    Arbitrary(Option<Value>),
}

fn value_form(value: &str) -> Option<Form<'_>> {
    if value.is_empty() {
        return Some(Form::Plain(Value::Bare));
    }
    if let Some(inner) = value.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return Some(Form::Arbitrary(arbitrary_form(inner, false)));
    }
    if let Some(inner) = value.strip_prefix('(').and_then(|rest| rest.strip_suffix(')')) {
        return Some(Form::Arbitrary(arbitrary_form(inner, true)));
    }
    let is_digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if is_digits(value) {
        return Some(Form::Plain(Value::Integer));
    }
    if let Some((whole, quarter)) = value.split_once('.') {
        return (is_digits(whole) && matches!(quarter, "25" | "5" | "75"))
            .then_some(Form::Plain(Value::Decimal));
    }
    if let Some((numerator, denominator)) = value.split_once('/')
        && is_digits(numerator)
        && is_digits(denominator)
    {
        return Some(Form::Plain(Value::Fraction));
    }
    // An opacity modifier after a slash does not change the slots.
    let name = value.split_once('/').map_or(value, |(name, _)| name);
    Some(Form::Named(name))
}

/// `var(--x)` or a `(--x)` shorthand, a `kind:` hint, or a literal whose kind
/// the value shows. A value of no inferable kind is `None`.
fn arbitrary_form(inner: &str, shorthand: bool) -> Option<Value> {
    if let Some((hint, rest)) = inner.split_once(':')
        && !hint.is_empty()
        && hint.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'-')
    {
        let &(_, kind) = HINTS.iter().find(|(name, _)| reversed_cmp(name, hint).is_eq())?;
        let is_var = if shorthand { rest.starts_with("--") } else { rest.starts_with("var(") };
        return is_var.then_some(Value::Hinted(kind));
    }
    if shorthand {
        return inner.starts_with("--").then_some(Value::Var);
    }
    if inner.starts_with("var(") {
        return Some(Value::Var);
    }
    literal_kind(inner).map(Value::Literal)
}

fn literal_kind(value: &str) -> Option<Kind> {
    const COLOR_FUNCTIONS: &[&str] = &[
        "rgb(",
        "rgba(",
        "hsl(",
        "hsla(",
        "hwb(",
        "lab(",
        "lch(",
        "oklab(",
        "oklch(",
        "color(",
        "color-mix(",
    ];
    const LENGTH_UNITS: &[&str] = &[
        "px", "rem", "em", "ex", "ch", "lh", "rlh", "vw", "vh", "vmin", "vmax", "svw", "svh",
        "lvw", "lvh", "dvw", "dvh", "cqw", "cqh", "cqi", "cqb", "cqmin", "cqmax", "cm", "mm", "in",
        "pt", "pc", "q",
    ];
    let is_number = |text: &str| {
        let text = text.strip_prefix('-').unwrap_or(text);
        !text.is_empty()
            && text.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
            && text.bytes().any(|b| b.is_ascii_digit())
    };
    if let Some(hex) = value.strip_prefix('#') {
        return (matches!(hex.len(), 3 | 4 | 6 | 8)
            && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(Kind::Color);
    }
    if COLOR_FUNCTIONS.iter().any(|function| value.starts_with(function)) {
        return Some(Kind::Color);
    }
    if value.starts_with("url(") {
        return Some(Kind::Url);
    }
    if ["linear-gradient(", "radial-gradient(", "conic-gradient(", "repeating-"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
    {
        return Some(Kind::Image);
    }
    if value.strip_suffix('%').is_some_and(is_number) {
        return Some(Kind::Percentage);
    }
    if is_number(value) {
        return Some(Kind::Number);
    }
    let is_length = |text: &str| {
        let digits_end = text
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .unwrap_or(text.len());
        is_number(&text[..digits_end])
            && (LENGTH_UNITS.contains(&&text[digits_end..]) || text[digits_end..].is_empty())
    };
    if is_length(value) {
        return (!is_number(value)).then_some(Kind::Length);
    }
    // `Kind::Shadow`: each comma-separated part has an x and a y offset.
    let is_shadow = value
        .split(',')
        .all(|shadow| shadow.split('_').filter(|part| is_length(part)).count() >= 2);
    is_shadow.then_some(Kind::Shadow)
}
