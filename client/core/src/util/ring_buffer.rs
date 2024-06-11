use std::sync::Arc;

use im::Vector;

#[derive(Debug, Clone)]
pub struct RingBuffer<T: std::fmt::Debug> {
    buffer: Vector<Option<Arc<T>>>,
    capacity: usize,
    head: usize,
    len: usize,
}

impl<T: Clone + std::fmt::Debug> Default for RingBuffer<T> {
    fn default() -> Self {
        Self {
            buffer: Vector::new(),
            capacity: 0,
            head: 0,
            len: 0,
        }
    }
}

impl<T: std::fmt::Debug> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        RingBuffer {
            buffer: (0..capacity).map(|_| None).collect(),
            capacity,
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, value: T) {
        let index = (self.head + self.len) % self.capacity;
        self.buffer[index] = Some(Arc::new(value));
        if self.len == self.capacity {
            self.head = (self.head + 1) % self.capacity;
        } else {
            self.len += 1;
        }
    }

    pub fn pop(&mut self) -> Option<Arc<T>> {
        if self.len == 0 {
            return None;
        };
        self.len -= 1;
        self.head = (self.head + self.capacity - 1) % self.capacity;
        self.buffer[self.head].take()
    }

    pub fn clear(&mut self) {
        *self = Self::new(self.capacity);
    }

    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<&Arc<T>> {
        let index = (self.head + index) % self.capacity;
        self.buffer.get(index).and_then(|v| v.as_ref())
    }
}

pub struct RingBufferIter<'a, T: Clone + std::fmt::Debug> {
    buffer: &'a RingBuffer<T>,
    index: usize,
    pos: usize,
}

impl<T: Clone + std::fmt::Debug> RingBuffer<T> {
    pub fn iter(&self) -> RingBufferIter<'_, T> {
        RingBufferIter {
            buffer: self,
            index: self.head,
            pos: 0,
        }
    }
}

impl<'a, T: Clone + std::fmt::Debug> Iterator for RingBufferIter<'a, T> {
    type Item = &'a Arc<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos != self.buffer.len {
            let value = &self.buffer.buffer[self.index];
            self.index = (self.index + 1) % self.buffer.capacity;
            self.pos += 1;
            value.as_ref()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_iter() {
        let mut buffer = RingBuffer::new(3);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        let mut iter = buffer.iter();

        assert_eq!(iter.next().map(AsRef::as_ref), Some(&1));
        assert_eq!(iter.next().map(AsRef::as_ref), Some(&2));
        assert_eq!(iter.next().map(AsRef::as_ref), Some(&3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_push_overflow() {
        let mut buffer = RingBuffer::new(3);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);

        let mut iter = buffer.iter();

        assert_eq!(iter.next().map(AsRef::as_ref), Some(&2));
        assert_eq!(iter.next().map(AsRef::as_ref), Some(&3));
        assert_eq!(iter.next().map(AsRef::as_ref), Some(&4));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_push_and_iter_wrap_around() {
        let mut buffer = RingBuffer::new(3);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);

        let mut iter = buffer.iter();

        assert_eq!(iter.next().map(AsRef::as_ref), Some(&2));
        assert_eq!(iter.next().map(AsRef::as_ref), Some(&3));
        assert_eq!(iter.next().map(AsRef::as_ref), Some(&4));
        assert_eq!(iter.next(), None);

        buffer.push(5);

        let mut iter = buffer.iter();

        assert_eq!(iter.next().map(AsRef::as_ref), Some(&3));
        assert_eq!(iter.next().map(AsRef::as_ref), Some(&4));
        assert_eq!(iter.next().map(AsRef::as_ref), Some(&5));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_default() {
        let ring_buffer: RingBuffer<i32> = Default::default();

        // Ensure that the default capacity is set correctly
        assert_eq!(ring_buffer.capacity, 0);

        // Ensure that the head and length are initialized to 0
        assert_eq!(ring_buffer.head, 0);
        assert_eq!(ring_buffer.len, 0);

        // Ensure that the buffer is empty
        assert_eq!(ring_buffer.buffer.len(), 0);
    }

    #[test]
    fn test_clear() {
        let mut buffer = RingBuffer::new(3);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        assert_eq!(buffer.len(), 3);

        buffer.clear();

        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.iter().count(), 0);
    }

    #[test]
    fn test_pop() {
        let mut buffer = RingBuffer::new(3);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        let popped_value = buffer.pop();
        assert_eq!(popped_value.map(Arc::unwrap_or_clone), Some(3));
        assert_eq!(buffer.len(), 2);

        let popped_value = buffer.pop();
        assert_eq!(popped_value.map(Arc::unwrap_or_clone), Some(2));
        assert_eq!(buffer.len(), 1);

        let popped_value = buffer.pop();
        assert_eq!(popped_value.map(Arc::unwrap_or_clone), Some(1));
        assert_eq!(buffer.len(), 0);

        let popped_value = buffer.pop();
        assert_eq!(popped_value, None);
        assert_eq!(buffer.len(), 0);
    }
}
