pub(crate) struct RingVec<T> {
    cache: Vec<T>,
    start: usize,
    size: usize,
}

pub(crate) struct Iter<'a, T> {
    ring_vec: &'a RingVec<T>,
    index: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.ring_vec.cache.len() {
            return None;
        }
        let idx = (self.ring_vec.start + self.index) % self.ring_vec.cache.len();
        self.index += 1;
        Some(&self.ring_vec.cache[idx])
    }
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

    pub(crate) fn iter(&self) -> Iter<'_, T> {
        Iter {
            ring_vec: self,
            index: 0,
        }
    }
    //     self.cache
    //         .iter()
    //         .cycle()
    //         .skip(self.start)
    //         .take(self.cache.len())
    // }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let start = self.start;
        // let len = self.cache.len();
        // 使用 split_at_mut 安全分割切片
        let (second, first) = self.cache.split_at_mut(start);
        // 总是返回 Chain 迭代器，保证类型一致
        first.iter_mut().chain(second.iter_mut())
    }

    /// 按 key 排序，先旋转到 start=0 再 sort_by_key，保证物理顺序 = 逻辑顺序
    pub(crate) fn sort_by_key<K: Ord>(&mut self, key: impl Fn(&T) -> K) {
        if self.cache.is_empty() {
            return;
        }
        if self.start > 0 {
            self.cache.rotate_left(self.start);
            self.start = 0;
        }
        self.cache.sort_by_key(|t| key(t));
    }
}

#[cfg(test)]
mod tests {
    use super::RingVec;

    fn collect_vec(rv: &RingVec<i32>) -> Vec<i32> {
        rv.iter().copied().collect::<Vec<_>>()
    }

    #[test]
    fn push_keeps_insertion_order_before_wrap() {
        let mut rv = RingVec::with_capacity(4);
        for v in 1..=4 {
            assert!(rv.push(v).is_none());
        }
        assert_eq!(rv.len(), 4);
        assert_eq!(rv.last(), Some(&4));
        assert_eq!(collect_vec(&rv), vec![1, 2, 3, 4]);
    }

    #[test]
    fn push_overwrites_oldest_when_full() {
        let mut rv = RingVec::with_capacity(3);
        assert!(rv.push(1).is_none());
        assert!(rv.push(2).is_none());
        assert!(rv.push(3).is_none());
        assert_eq!(rv.push(4), Some(1));
        assert_eq!(collect_vec(&rv), vec![2, 3, 4]);
        assert_eq!(rv.get(0), Some(&2));
        assert_eq!(rv.get(2), Some(&4));
    }

    #[test]
    fn push_front_replaces_tail_after_fill() {
        let mut rv = RingVec::with_capacity(3);
        for v in [100, 200, 300] {
            assert!(rv.push(v).is_none());
        }
        assert_eq!(rv.push_front(50), Some(300));
        assert_eq!(collect_vec(&rv), vec![50, 100, 200]);
    }

    #[test]
    fn remove_handles_wrapped_start_positions() {
        let mut rv = RingVec::with_capacity(5);
        for v in 1..=5 {
            assert!(rv.push(v).is_none());
        }
        assert_eq!(rv.push(6), Some(1));
        assert_eq!(collect_vec(&rv), vec![2, 3, 4, 5, 6]);

        assert_eq!(rv.remove(1), Some(3));
        assert_eq!(collect_vec(&rv), vec![2, 4, 5, 6]);

        assert_eq!(rv.remove_last(), Some(6));
        assert_eq!(collect_vec(&rv), vec![2, 4, 5]);
    }

    #[test]
    fn iter_mut_allows_in_place_updates_and_clear() {
        let mut rv = RingVec::with_capacity(3);
        for v in 1..=3 {
            assert!(rv.push(v).is_none());
        }
        for val in rv.iter_mut() {
            *val += 10;
        }
        assert_eq!(collect_vec(&rv), vec![11, 12, 13]);

        rv.clear();
        assert!(rv.is_empty());
        assert_eq!(rv.len(), 0);
        assert_eq!(rv.iter().count(), 0);
    }
}
