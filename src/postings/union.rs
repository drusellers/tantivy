use postings::{DocSet, DocSetGroup, SkipResult, UnionAllDocSet};
use DocId;

/// Creates a `DocSet` that iterates through the (distinct) union of two or more `DocSet`s.
/// The duplicate elements are removed.
pub struct UnionDocSet<TDocSet: DocSet> {
    inner: UnionAllDocSet<TDocSet>,
    current: Option<DocId>,
}

impl<TDocSet: DocSet> From<Vec<TDocSet>> for UnionDocSet<TDocSet> {
    fn from(docsets: Vec<TDocSet>) -> UnionDocSet<TDocSet> {
        UnionDocSet {
            inner: UnionAllDocSet::from(docsets),
            current: None,
        }
    }
}

impl<TDocSet: DocSet> DocSetGroup<TDocSet> for UnionDocSet<TDocSet> {
    fn docsets(&self) -> &[TDocSet] {
        self.inner.docsets()
    }
}

impl<TDocSet: DocSet> DocSet for UnionDocSet<TDocSet> {
    fn size_hint(&self) -> usize {
        self.inner.size_hint()
    }

    #[allow(never_loop)]
    fn advance(&mut self) -> bool {
        if !self.inner.advance() {
            return false;
        }

        let doc = self.inner.doc();
        if Some(doc) == self.current && self.inner.skip_next(doc + 1) == SkipResult::End {
            return false;
        }

        self.current = Some(self.inner.doc());
        true
    }

    fn skip_next(&mut self, target: DocId) -> SkipResult {
        let res = self.inner.skip_next(target);
        if res != SkipResult::End {
            self.current = Some(self.inner.doc());
        }
        res
    }

    fn doc(&self) -> DocId {
        self.inner.doc()
    }
}


#[cfg(test)]
mod tests {

    use postings::{DocSet, SkipResult, VecPostings, UnionDocSet};

    #[test]
    fn test_union() {
        let left = VecPostings::from(vec![1, 3, 9]);
        let right = VecPostings::from(vec![3, 4, 9, 18]);
        let mut union = UnionDocSet::from(vec![left, right]);
        assert!(union.advance());
        assert_eq!(union.doc(), 1);
        assert!(union.advance());
        assert_eq!(union.doc(), 3);
        assert!(union.advance());
        assert_eq!(union.doc(), 4);
        assert!(union.advance());
        assert_eq!(union.doc(), 9);
        assert!(union.advance());
        assert_eq!(union.doc(), 18);
        assert!(!union.advance());
    }

    #[test]
    fn test_union_empty() {
        let a = VecPostings::from(vec![]);
        let b = VecPostings::from(vec![]);
        let c = VecPostings::from(vec![]);
        let mut union = UnionDocSet::from(vec![a, b, c]);
        assert!(!union.advance());
    }

    #[test]
    fn test_union_skip_next() {
        let a = VecPostings::from(vec![1, 3, 7]);
        let b = VecPostings::from(vec![1, 4]);
        let c = VecPostings::from(vec![1, 9]);
        let mut union = UnionDocSet::from(vec![a, b, c]);
        assert_eq!(union.skip_next(1), SkipResult::Reached);
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
