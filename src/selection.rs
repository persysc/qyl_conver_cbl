use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct Selection {
    pub(crate) items: HashSet<usize>,
    anchor: Option<usize>,
    pub(crate) cursor: Option<usize>,
}

impl Selection {
    pub(crate) fn select(&mut self, index: usize, visible: &[usize], toggle: bool, range: bool) {
        let Some(end) = visible.iter().position(|item| *item == index) else { return };
        self.cursor = Some(index);
        let start = self.anchor.filter(|_| range).and_then(|anchor| visible.iter().position(|item| *item == anchor));
        if let Some(start) = start {
            if !toggle { self.items.clear(); }
            self.items.extend(&visible[start.min(end)..=start.max(end)]);
        } else {
            if !toggle { self.items.clear(); }
            if !self.items.insert(index) { self.items.remove(&index); }
            self.anchor = Some(index);
        }
    }

    pub(crate) fn targets(&self, visible: &[usize]) -> Vec<usize> {
        visible.iter().copied().filter(|index| self.items.contains(index)).collect()
    }

    pub(crate) fn closed(&mut self, index: usize) {
        fn shift(item: usize, index: usize) -> Option<usize> {
            if item == index { None } else if item > index { Some(item - 1) } else { Some(item) }
        }
        self.items = self.items.iter().copied().filter_map(|item| shift(item, index)).collect();
        self.anchor = self.anchor.and_then(|item| shift(item, index));
        self.cursor = self.cursor.and_then(|item| shift(item, index));
    }
}
