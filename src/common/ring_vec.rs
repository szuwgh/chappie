pub(crate) struct RingVec<T> {
    cache: Vec<T>,
    start: usize,
    size: usize,
}

impl<T> RingVec<T> {
    pub(crate) fn new(size: usize) -> Self {
        RingVec {
            cache: Vec::with_capacity(size),
            start: 0,
            size: size,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    //删除第最后元素 并返回这个元素
    pub(crate) fn remove_last(&mut self) -> Option<T> {
        self.remove(self.cache.len() - 1)
    }

    //删除第n个元素 并返回这个元素
    pub(crate) fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.cache.len() {
            return None;
        }
        let idx = (self.start + index) % self.cache.len();
        let item = self.cache.remove(idx);
        if self.start > idx {
            self.start -= 1;
        }

        Some(item)
    }

    pub(crate) fn push_front(&mut self, item: T) {
        if self.cache.len() < self.size {
            self.cache.insert(self.start, item);
        } else {
            if self.start == 0 {
                self.start = self.size - 1;
            } else {
                self.start = (self.start - 1) % self.size;
            }
            self.cache[self.start] = item;
        }
    }

    pub(crate) fn push(&mut self, item: T) {
        if self.cache.len() < self.size {
            if self.start == 0 {
                self.cache.push(item);
            } else {
                let end = (self.start + self.cache.len()) % self.cache.len();
                self.cache.insert(end, item);
                self.start = (self.start + 1) % self.cache.len();
            }
        } else {
            self.cache[self.start] = item;
            self.start = (self.start + 1) % self.size;
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.cache.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        if index >= self.cache.len() {
            return None;
        }
        let idx = (self.start + index) % self.cache.len();
        Some(&self.cache[idx])
    }

    pub(crate) fn last(&self) -> Option<&T> {
        if self.cache.is_empty() {
            return None;
        }
        let idx = (self.start + self.cache.len() - 1) % self.cache.len();
        Some(&self.cache[idx])
    }

    pub(crate) fn clear(&mut self) {
        self.cache.clear();
        self.start = 0;
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.cache
            .iter()
            .cycle()
            .skip(self.start)
            .take(self.cache.len())
    }
}
