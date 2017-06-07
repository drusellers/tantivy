use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::mem;
use postings::{DocSet, SkipResult};
use DocId;

/// Each `HeapItem` represents the head of
/// one of scorer being merged.
///
/// * `doc` - is the current doc id for the given segment postings
/// * `ord` - is the ordinal used to identify to which segment postings
/// this heap item belong to.
#[derive(Debug, Eq, PartialEq)]
struct HeapItem {
    doc: DocId,
    ord: u32,
}

/// `HeapItem`s are ordered by their `DocId`
impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
        (other.doc).cmp(&self.doc)
    }
}

/// Creates a `DocSet` that iterates through the union of two or more `DocSet`s.
/// Note that the duplicate elements are kept.
pub struct UnionAllDocSet<TDocSet: DocSet> {
    docsets: Vec<TDocSet>,
    queue: BinaryHeap<HeapItem>,
    finished: bool,
    doc: DocId,
}

impl<TDocSet: DocSet> From<Vec<TDocSet>> for UnionAllDocSet<TDocSet> {
    fn from(mut docsets: Vec<TDocSet>) -> UnionAllDocSet<TDocSet> {
        assert!(docsets.len() >= 2);
        let mut heap_items = Vec::with_capacity(docsets.len());
        for (ord, mut docset) in docsets.iter_mut().enumerate() {
            if docset.advance() {
                heap_items.push(HeapItem {
                                    doc: docset.doc(),
                                    ord: ord as u32,
                                });
            }
        }
        UnionAllDocSet {
            docsets: docsets,
            queue: BinaryHeap::from(heap_items),
            finished: false,
            doc: 0u32,
        }
    }
}

impl<TDocSet: DocSet> UnionAllDocSet<TDocSet> {
    /// Updates the current `DocId` and advances the head of the heap (the docset
    /// with the lowest doc)
    ///
    /// After advancing, the docset is removed from the heap if it has been entirely consumed.
    ///
    /// # Panics
    /// This method will panic if the heap is not empty.
    fn advance_head(&mut self) {
        {
            let mut mutable_head = self.queue.peek_mut().unwrap();
            self.doc = mutable_head.doc;
            let docset = &mut self.docsets[mutable_head.ord as usize];
            if docset.advance() {
                mutable_head.doc = docset.doc();
                return;
            }
        }

        // TODO: replace with PeekMut::pop one day
        self.queue.pop();
    }

    /// Returns an array to the underlying `DocSet`s of the union.
    /// These `DocSet` are in the same position as the `UnionAllDocSet`,
    /// so that user can access their `docfreq` and `positions`.
    pub fn docsets(&self) -> &[TDocSet] {
        &self.docsets[..]
    }
}

impl<TDocSet: DocSet> DocSet for UnionAllDocSet<TDocSet> {
    fn size_hint(&self) -> usize {
        self.docsets.iter().map(|docset| docset.size_hint()).sum()
    }

    #[allow(never_loop)]
    fn advance(&mut self) -> bool {
        if self.finished {
            return false;
        }

        if self.queue.peek().is_none() {
            self.finished = true;
            return false;
        }

        // If the heap was empty we would have returned from the function in the the `match` above
        // so this is safe
        self.advance_head();

        true
    }

    fn skip_next(&mut self, target: DocId) -> SkipResult {
        if self.finished {
            return SkipResult::End;
        }

        let queue = mem::replace(&mut self.queue, BinaryHeap::new());
        let mut heap_items = queue.into_vec();
        let mut found = false;
        for item in &mut heap_items {
            if item.doc == target {
                self.doc = target;
                found = true;
                continue;
            }
            if item.doc > target {
                continue;
            }

            let mut docset = &mut self.docsets[item.ord as usize];
            match docset.skip_next(target) {
                SkipResult::Reached => {
                    item.doc = target;
                    self.doc = target;
                    found = true;
                }
                SkipResult::OverStep => {
                    item.doc = docset.doc();
                }
                SkipResult::End => {
                    // Mark the item as finished so we can remove it later
                    item.ord = u32::max_value();
                }
            }
        }
        heap_items.retain(|item| item.ord < u32::max_value());

        if heap_items.is_empty() {
            self.finished = true;
            return SkipResult::End;
        }

        self.queue = BinaryHeap::from(heap_items);

        if found {
            SkipResult::Reached
        } else {
            // It's safe since we know that the heap is not empty
            self.advance_head();

            SkipResult::OverStep
        }
    }

    fn doc(&self) -> DocId {
        self.doc
    }
}


#[cfg(test)]
mod tests {

    use postings::{DocSet, SkipResult, VecPostings, UnionAllDocSet};

    #[test]
    fn test_union_all() {
        let left = VecPostings::from(vec![1, 3, 9]);
        let right = VecPostings::from(vec![3, 4, 9, 18]);
        let mut union = UnionAllDocSet::from(vec![left, right]);
        assert!(union.advance());
        assert_eq!(union.doc(), 1);
        assert!(union.advance());
        assert_eq!(union.doc(), 3);
        assert!(union.advance());
        assert_eq!(union.doc(), 3);
        assert!(union.advance());
        assert_eq!(union.doc(), 4);
        assert!(union.advance());
        assert_eq!(union.doc(), 9);
        assert!(union.advance());
        assert_eq!(union.doc(), 9);
        assert!(union.advance());
        assert_eq!(union.doc(), 18);
        assert!(!union.advance());
    }

    #[test]
    fn test_union_all_empty() {
        let a = VecPostings::from(vec![]);
        let b = VecPostings::from(vec![]);
        let c = VecPostings::from(vec![]);
        let mut union = UnionAllDocSet::from(vec![a, b, c]);
        assert!(!union.advance());
    }

    #[test]
    fn test_union_all_skip_next() {
        let a = VecPostings::from(vec![1, 3, 7]);
        let b = VecPostings::from(vec![1, 4]);
        let c = VecPostings::from(vec![1, 9]);
        let mut union = UnionAllDocSet::from(vec![a, b, c]);
        assert_eq!(union.skip_next(1), SkipResult::Reached);
        assert_eq!(union.doc(), 1);
        assert!(union.advance());
        assert_eq!(union.doc(), 1);
        assert_eq!(union.skip_next(2), SkipResult::OverStep);
        assert_eq!(union.doc(), 3);
        assert!(union.advance());
        assert_eq!(union.doc(), 4);
        assert_eq!(union.skip_next(7), SkipResult::Reached);
        assert_eq!(union.doc(), 7);
        assert_eq!(union.skip_next(10), SkipResult::End);
        assert!(!union.advance());
        assert_eq!(union.skip_next(11), SkipResult::End);
    }
}
