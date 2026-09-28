use sailfish::TemplateOnce;

/// How many pages either side of the current one get their own link.
const WINDOW: u32 = 2;

/// Numbered pagination for a page whose results are addressed by a `page`
/// query parameter: previous and next arrows, the first and last pages, the
/// pages near the current one, and an ellipsis over each gap.
#[derive(TemplateOnce)]
#[template(path = "component/pagination.stpl.html")]
pub struct NumberedPagination {
    previous: Option<String>,
    next: Option<String>,
    items: Vec<PageItem>,
}

enum PageItem {
    Page {
        number: u32,
        href: String,
        current: bool,
    },
    Gap,
}

impl NumberedPagination {
    /// Pagination for page `current` of `total`, linking each page to
    /// `href(page)`. `None` when there is only one page, so there is nothing to
    /// navigate.
    pub fn new(current: u32, total: u32, href: impl Fn(u32) -> String) -> Option<Self> {
        if total <= 1 {
            return None;
        }

        let mut items = Vec::new();
        let mut last_shown = 0;
        for number in page_numbers(current, total) {
            if last_shown > 0 && number > last_shown + 1 {
                items.push(PageItem::Gap);
            }
            items.push(PageItem::Page {
                number,
                href: href(number),
                current: number == current,
            });
            last_shown = number;
        }

        Some(NumberedPagination {
            previous: (current > 1).then(|| href(current - 1)),
            next: (current < total).then(|| href(current + 1)),
            items,
        })
    }
}

/// The page numbers that get a link: the first, the last, and those within
/// [`WINDOW`] of the current page, in order. `total` is at least 2.
fn page_numbers(current: u32, total: u32) -> Vec<u32> {
    let start = if current > WINDOW + 1 {
        current - WINDOW
    } else {
        2
    };
    let end = if current + WINDOW < total {
        current + WINDOW
    } else {
        total - 1
    };

    let mut numbers = vec![1];
    numbers.extend((start..=end).filter(|&p| p > 1 && p < total));
    numbers.push(total);
    numbers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_runs_show_every_page() {
        assert_eq!(page_numbers(1, 2), vec![1, 2]);
        assert_eq!(page_numbers(3, 5), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn long_runs_window_the_current_page() {
        assert_eq!(page_numbers(1, 10), vec![1, 2, 3, 10]);
        assert_eq!(page_numbers(6, 10), vec![1, 4, 5, 6, 7, 8, 10]);
        assert_eq!(page_numbers(10, 10), vec![1, 8, 9, 10]);
    }

    #[test]
    fn gaps_fall_between_distant_pages() {
        let pagination = NumberedPagination::new(6, 10, |p| format!("?page={p}")).unwrap();
        let shape: Vec<Option<u32>> = pagination
            .items
            .iter()
            .map(|item| match item {
                PageItem::Page { number, .. } => Some(*number),
                PageItem::Gap => None,
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                Some(1),
                None,
                Some(4),
                Some(5),
                Some(6),
                Some(7),
                Some(8),
                None,
                Some(10)
            ]
        );
        assert_eq!(pagination.previous.as_deref(), Some("?page=5"));
        assert_eq!(pagination.next.as_deref(), Some("?page=7"));
    }

    #[test]
    fn a_single_page_needs_no_pagination() {
        assert!(NumberedPagination::new(1, 1, |p| p.to_string()).is_none());
        assert!(NumberedPagination::new(1, 0, |p| p.to_string()).is_none());
    }
}
