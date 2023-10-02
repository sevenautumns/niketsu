#[derive(Debug, Clone)]
pub struct RingBuffer<T> {
    buffer: Vec<Option<T>>,
    capacity: usize,
    head: usize,
    len: usize,
}

impl<T> Default for RingBuffer<T> {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            capacity: 0,
            head: 0,
            len: 0,
        }
    }
}

impl<T> RingBuffer<T> {
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
        self.buffer[index] = Some(value);
        if self.len == self.capacity {
            self.head = (self.head + 1) % self.capacity;
        } else {
            self.len += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        let index = (self.head + index) % self.capacity;
        self.buffer.get(index).and_then(|v| v.as_ref())
    }
}

pub struct RingBufferIter<'a, T> {
    buffer: &'a RingBuffer<T>,
    index: usize,
    pos: usize,
}

impl<T> RingBuffer<T> {
    pub fn iter(&self) -> RingBufferIter<'_, T> {
        RingBufferIter {
            buffer: self,
            index: self.head,
            pos: 0,
        }
    }
}

impl<'a, T> Iterator for RingBufferIter<'a, T> {
    type Item = &'a T;

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

        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
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

        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), Some(&4));
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

        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), None);

        buffer.push(5);

        let mut iter = buffer.iter();

        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&5));
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
}
