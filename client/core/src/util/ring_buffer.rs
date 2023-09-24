#[derive(Debug, Clone)]
pub struct RingBuffer<T> {
    buffer: Vec<Option<T>>,
    capacity: usize,
    head: usize,
    tail: usize,
}

impl<T> Default for RingBuffer<T> {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            capacity: 0,
            head: 0,
            tail: 0,
        }
    }
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        RingBuffer {
            buffer: (0..capacity).map(|_| None).collect(),
            capacity,
            head: 0,
            tail: 0,
        }
    }

    pub fn push(&mut self, value: T) {
        self.buffer[self.tail] = Some(value);
        self.tail = (self.tail + 1) % self.capacity;
        if self.tail == self.head {
            self.head = (self.head + 1) % self.capacity;
        }
    }
}

pub struct RingBufferIter<'a, T> {
    buffer: &'a RingBuffer<T>,
    index: usize,
}

impl<T> RingBuffer<T> {
    pub fn iter(&self) -> RingBufferIter<'_, T> {
        RingBufferIter {
            buffer: self,
            index: self.head,
        }
    }
}

impl<'a, T> Iterator for RingBufferIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index != self.buffer.tail {
            let value = &self.buffer.buffer[self.index];
            self.index = (self.index + 1) % self.buffer.capacity;
            value.as_ref()
        } else {
            None
        }
    }
}
