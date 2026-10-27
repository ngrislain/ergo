//! Bounded loaded-catalog ordering, shared by the viewer and native tests.
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub(super) struct Assembly {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub piece_count: Option<usize>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub created_by_name: Option<String>,
    #[serde(default)]
    pub created_at: Option<u64>,
}
impl Assembly {
    pub fn author(&self) -> &str {
        self.created_by_name
            .as_deref()
            .or(self.created_by.as_deref())
            .unwrap_or("Author unavailable")
    }
}
pub(super) fn sort(items: &mut [Assembly], key: &str, descending: bool) {
    items.sort_by(|a, b| {
        let order = match key {
            "author" => a.author().to_lowercase().cmp(&b.author().to_lowercase()),
            "date" => match (a.created_at, b.created_at) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => return std::cmp::Ordering::Less,
                (None, Some(_)) => return std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            },
            "size" => match (a.piece_count, b.piece_count) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => return std::cmp::Ordering::Less,
                (None, Some(_)) => return std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            },
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        };
        let order = if descending { order.reverse() } else { order };
        order
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sorting_uses_real_metadata_and_keeps_unknown_dates_and_sizes_last() {
        let make = |id: &str, name: &str, author: &str, date, count| Assembly {
            id: id.into(),
            name: name.into(),
            created_by: Some(author.into()),
            created_by_name: Some(author.into()),
            created_at: date,
            piece_count: count,
        };
        let mut items = vec![
            make("c", "Zoo", "Alice", None, None),
            make("b", "Beta", "Bob", Some(20), Some(2)),
            make("a", "Alpha", "Alice", Some(10), Some(50)),
        ];
        for (key, descending, expected) in [
            ("name", false, ["a", "b", "c"]),
            ("author", false, ["a", "c", "b"]),
            ("date", true, ["b", "a", "c"]),
            ("size", true, ["a", "b", "c"]),
        ] {
            sort(&mut items, key, descending);
            assert_eq!(
                items.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
                expected
            );
        }
    }
}
