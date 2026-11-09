//! Directly indexed interval columns in lazily allocated row-major pages.
//! A round blank uses almost every column; tree searches are unnecessary here.
//! Unused pages cost one pointer, and checkpoints share unchanged pages.
use super::RemovedSpan;
use std::sync::Arc;

const PAGE_BITS: usize = 8;
const PAGE_SIZE: usize = 1 << PAGE_BITS;

#[derive(Clone, Debug)]
struct Page {
    columns: [Option<Vec<RemovedSpan>>; PAGE_SIZE],
}

#[derive(Clone, Debug)]
pub(super) struct IntervalColumns {
    pages: Vec<Option<Arc<Page>>>,
    used_pages: usize,
    // Keep the old storage available only to differential simulation tests.
    #[cfg(test)]
    reference: Option<std::collections::BTreeMap<usize, Vec<RemovedSpan>>>,
}

impl IntervalColumns {
    pub(super) fn new(columns: usize) -> Self {
        Self {
            pages: vec![None; columns.div_ceil(PAGE_SIZE)],
            used_pages: 0,
            #[cfg(test)]
            reference: None,
        }
    }

    #[inline]
    pub(super) fn get(&self, index: &usize) -> Option<&Vec<RemovedSpan>> {
        #[cfg(test)]
        if let Some(reference) = &self.reference {
            return reference.get(index);
        }
        self.pages.get(*index >> PAGE_BITS)?.as_ref()?.columns[*index & (PAGE_SIZE - 1)].as_ref()
    }

    pub(super) fn contains_key(&self, index: &usize) -> bool {
        self.get(index).is_some()
    }

    pub(super) fn is_empty(&self) -> bool {
        #[cfg(test)]
        if let Some(reference) = &self.reference {
            return reference.is_empty();
        }
        self.used_pages == 0
    }

    pub(super) fn get_or_default(&mut self, index: usize) -> &mut Vec<RemovedSpan> {
        #[cfg(test)]
        if let Some(reference) = &mut self.reference {
            return reference.entry(index).or_default();
        }
        let page = self.pages[index >> PAGE_BITS].get_or_insert_with(|| {
            self.used_pages += 1;
            Arc::new(Page {
                columns: std::array::from_fn(|_| None),
            })
        });
        Arc::make_mut(page).columns[index & (PAGE_SIZE - 1)].get_or_insert_default()
    }

    pub(super) fn insert(&mut self, index: usize, spans: Vec<RemovedSpan>) {
        *self.get_or_default(index) = spans;
    }

    // Ascending column order preserves the original volume summation order.
    fn paged_iter(&self) -> impl Iterator<Item = (usize, &Vec<RemovedSpan>)> {
        self.pages.iter().enumerate().flat_map(|(page, columns)| {
            columns.iter().flat_map(move |columns| {
                columns
                    .columns
                    .iter()
                    .enumerate()
                    .filter_map(move |(column, spans)| {
                        spans
                            .as_ref()
                            .map(|spans| (page * PAGE_SIZE + column, spans))
                    })
            })
        })
    }

    #[cfg(not(test))]
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &Vec<RemovedSpan>)> {
        self.paged_iter()
    }

    #[cfg(test)]
    pub(super) fn iter(&self) -> Box<dyn Iterator<Item = (usize, &Vec<RemovedSpan>)> + '_> {
        if let Some(reference) = &self.reference {
            Box::new(reference.iter().map(|(&index, spans)| (index, spans)))
        } else {
            Box::new(self.paged_iter())
        }
    }

    pub(super) fn storage_bytes(&self) -> usize {
        // Shared pages are counted in full: checkpoint budgeting remains conservative.
        self.pages.capacity() * std::mem::size_of::<Option<Arc<Page>>>()
            + self.used_pages * std::mem::size_of::<Page>()
            + self
                .iter()
                .map(|(_, spans)| spans.capacity() * std::mem::size_of::<RemovedSpan>())
                .sum::<usize>()
    }

    #[cfg(test)]
    pub(super) fn use_reference(&mut self) {
        self.reference = Some(
            self.paged_iter()
                .map(|(i, spans)| (i, spans.clone()))
                .collect(),
        );
        self.pages.clear();
        self.used_pages = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_boundaries_and_checkpoint_writes_preserve_column_identity() {
        let mut columns = IntervalColumns::new(3 * PAGE_SIZE + 1);
        let span = RemovedSpan {
            lower: -2.,
            upper: 3.,
            lower_op: Some(7),
            upper_op: Some(8),
            lower_source: 9,
            upper_source: 10,
        };
        let indices = [
            0,
            PAGE_SIZE - 1,
            PAGE_SIZE,
            2 * PAGE_SIZE - 1,
            3 * PAGE_SIZE,
        ];
        for index in indices {
            columns.insert(index, vec![span]);
        }
        let mut checkpoint = columns.clone();
        checkpoint.get_or_default(PAGE_SIZE)[0].upper = 4.;
        checkpoint.insert(PAGE_SIZE + 1, vec![span]);
        assert_eq!(columns.get(&PAGE_SIZE).unwrap()[0].upper, 3.);
        assert!(!columns.contains_key(&(PAGE_SIZE + 1)));
        assert_eq!(checkpoint.get(&PAGE_SIZE).unwrap()[0].upper, 4.);
        assert_eq!(columns.iter().map(|(i, _)| i).collect::<Vec<_>>(), indices);
        assert_eq!(columns.used_pages, 3);
        assert!(columns.get(&usize::MAX).is_none());
        assert!(Arc::ptr_eq(
            columns.pages[0].as_ref().unwrap(),
            checkpoint.pages[0].as_ref().unwrap()
        ));
        assert!(!Arc::ptr_eq(
            columns.pages[1].as_ref().unwrap(),
            checkpoint.pages[1].as_ref().unwrap()
        ));
    }
}
