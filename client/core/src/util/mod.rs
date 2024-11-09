pub mod observed;
pub mod ring_buffer;

pub use observed::Observed;
pub use ring_buffer::{RingBuffer, RingBufferIter};

#[derive(Debug, Clone)]
pub struct FuzzyResult<T: Clone> {
    pub score: i64,
    pub hits: Vec<usize>,
    pub entry: T,
}
