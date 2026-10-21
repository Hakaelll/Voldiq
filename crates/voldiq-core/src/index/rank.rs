use std::cmp::Ordering;

pub(super) fn select<T>(
    items: &mut [T],
    nth: usize,
    compare: &impl Fn(&T, &T) -> Ordering,
    cancelled: &mut impl FnMut() -> bool,
) -> bool {
    debug_assert!(nth < items.len());
    let (mut low, mut high) = (0, items.len());
    let mut depth = 2 * items.len().ilog2();
    while high - low > 1 {
        if cancelled() {
            return false;
        }
        if depth == 0 {
            return heap_sort(&mut items[low..high], compare, cancelled);
        }
        depth -= 1;
        let middle = low + (high - low) / 2;
        let last = high - 1;
        let mut candidates = [low, middle, last];
        candidates.sort_unstable_by(|a, b| compare(&items[*a], &items[*b]));
        items.swap(candidates[1], last);
        let mut boundary = low;
        for position in low..last {
            if (position - low) % 256 == 0 && cancelled() {
                return false;
            }
            if compare(&items[position], &items[last]).is_lt() {
                items.swap(position, boundary);
                boundary += 1;
            }
        }
        items.swap(boundary, last);
        match nth.cmp(&boundary) {
            Ordering::Equal => return !cancelled(),
            Ordering::Less => high = boundary,
            Ordering::Greater => low = boundary + 1,
        }
    }
    !cancelled()
}

fn heap_sort<T>(
    items: &mut [T],
    compare: &impl Fn(&T, &T) -> Ordering,
    cancelled: &mut impl FnMut() -> bool,
) -> bool {
    for root in (0..items.len() / 2).rev() {
        if root % 256 == 0 && cancelled() {
            return false;
        }
        sift(items, root, compare);
    }
    for end in (1..items.len()).rev() {
        if end % 256 == 0 && cancelled() {
            return false;
        }
        items.swap(0, end);
        sift(&mut items[..end], 0, compare);
    }
    !cancelled()
}

fn sift<T>(items: &mut [T], mut root: usize, compare: &impl Fn(&T, &T) -> Ordering) {
    while root < items.len() / 2 {
        let mut child = root * 2 + 1;
        if child + 1 < items.len() && compare(&items[child], &items[child + 1]).is_lt() {
            child += 1;
        }
        if !compare(&items[root], &items[child]).is_lt() {
            break;
        }
        items.swap(root, child);
        root = child;
    }
}
