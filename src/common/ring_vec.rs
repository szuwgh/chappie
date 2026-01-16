pub(crate) struct RingVec<T> {
    cache: Vec<T>,
    start: usize,
    size: usize,
}

impl<T> RingVec<T> {
    pub(crate) fn with_capacity(size: usize) -> Self {
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

    pub(crate) fn push_front(&mut self, item: T) -> Option<T> {
        if self.cache.len() < self.size {
            self.cache.insert(self.start, item);
            None
        } else {
            if self.start == 0 {
                self.start = self.size - 1;
            } else {
                self.start = (self.start - 1) % self.size;
            }
            // 替换并返回旧值
            let old = std::mem::replace(&mut self.cache[self.start], item);
            Some(old)
        }
    }

    pub(crate) fn push(&mut self, item: T) -> Option<T> {
        if self.cache.len() < self.size {
            if self.start == 0 {
                self.cache.push(item);
            } else {
                let end = (self.start + self.cache.len()) % self.cache.len();
                self.cache.insert(end, item);
                self.start = (self.start + 1) % self.cache.len();
            }
            None
        } else {
            let old = std::mem::replace(&mut self.cache[self.start], item);
            self.start = (self.start + 1) % self.size;
            Some(old)
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

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let start = self.start;
        // let len = self.cache.len();
        // 使用 split_at_mut 安全分割切片
        let (second, first) = self.cache.split_at_mut(start);
        // 总是返回 Chain 迭代器，保证类型一致
        first.iter_mut().chain(second.iter_mut())
    }
}
