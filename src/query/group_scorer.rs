use std::marker::PhantomData;
use Score;
use DocId;
use postings::{DocSet, DocSetGroup, SkipResult};
use query::Scorer;
use query::boolean_query::ScoreCombiner;

/// Represents a `Scorer` for a collection of `Scorer`s
pub struct GroupScorer<TDocSetGroup, TScorer>
where
    TDocSetGroup: DocSetGroup<TScorer>,
    TScorer: Scorer,
{
    inner: TDocSetGroup,
    score_combiner: ScoreCombiner,
    phantom: PhantomData<TScorer>,
}

impl<TDocSetGroup, TScorer> From<Vec<TScorer>> for GroupScorer<TDocSetGroup, TScorer>
    where TDocSetGroup: DocSetGroup<TScorer> + From<Vec<TScorer>>,
          TScorer: Scorer
{
    fn from(scorers: Vec<TScorer>) -> GroupScorer<TDocSetGroup, TScorer> {
        let num_scorers = scorers.len();
        GroupScorer {
            inner: TDocSetGroup::from(scorers),
            score_combiner: ScoreCombiner::default_for_num_scorers(num_scorers),
            phantom: PhantomData,
        }
    }
}

impl<TDocSetGroup, TScorer> DocSet for GroupScorer<TDocSetGroup, TScorer>
    where TDocSetGroup: DocSetGroup<TScorer>,
          TScorer: Scorer
{
    fn advance(&mut self) -> bool {
        if !self.inner.advance() {
            return false;
        }

        self.score_combiner.clear();
        for scorer in self.inner.docsets() {
            self.score_combiner.update(scorer.score());
        }

        true
    }

    fn skip_next(&mut self, target: DocId) -> SkipResult {
        let res = self.inner.skip_next(target);
        if res == SkipResult::Reached {
            self.score_combiner.clear();
            for scorer in self.inner.docsets() {
                self.score_combiner.update(scorer.score());
            }
        }
        res
    }

    fn doc(&self) -> DocId {
        self.inner.doc()
    }

    fn size_hint(&self) -> usize {
        self.inner.size_hint()
    }
}

impl<TDocSetGroup, TScorer> Scorer for GroupScorer<TDocSetGroup, TScorer>
    where TDocSetGroup: DocSetGroup<TScorer>,
          TScorer: Scorer
{
    fn score(&self) -> Score {
        self.score_combiner.score()
    }
}
