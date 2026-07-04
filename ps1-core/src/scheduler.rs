use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    VBlank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub fire_time: u64,
    pub kind: EventKind,
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .fire_time
            .cmp(&self.fire_time)
            .then_with(|| (self.kind as u8).cmp(&(other.kind as u8)))
    }
}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scheduler {
    timestamp: u64,
    queue: BinaryHeap<Event>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            timestamp: 0,
            queue: BinaryHeap::new(),
        }
    }

    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn add_cycles(&mut self, cycles: u64) {
        self.timestamp = self.timestamp.saturating_add(cycles);
    }

    pub fn schedule(&mut self, event: Event) {
        self.queue.push(event);
    }

    pub fn pop_if_ready(&mut self) -> Option<Event> {
        if self
            .queue
            .peek()
            .map(|event| event.fire_time <= self.timestamp)
            .unwrap_or(false)
        {
            self.queue.pop()
        } else {
            None
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
