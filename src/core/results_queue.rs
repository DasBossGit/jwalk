use crossbeam::channel::{self, Receiver, SendError, Sender, TryRecvError};
use std::collections::BinaryHeap;
use std::marker::PhantomData;
use std::thread;

use super::Delegate;
use super::DirList;
use super::IndexPath;

#[derive(Clone)]
pub struct ResultsQueue<D>
where
  D: Delegate,
{
  sender: Sender<DirList<D>>,
}

pub struct ResultsQueueIter<D>
where
  D: Delegate,
{
  next_matcher: NextResultMatcher<D>,
  receiver: Receiver<DirList<D>>,
  receive_buffer: BinaryHeap<DirList<D>>,
}

struct NextResultMatcher<D>
where
  D: Delegate,
{
  looking_for_index_path: IndexPath,
  remaining_read_dirs: Vec<usize>,
  phantom: PhantomData<D>,
}

pub fn new_results_queue<D>() -> (ResultsQueue<D>, ResultsQueueIter<D>)
where
  D: Delegate,
{
  //let (sender, receiver) = channel::bounded(100);
  let (sender, receiver) = channel::unbounded();
  (
    ResultsQueue { sender },
    ResultsQueueIter {
      receiver,
      next_matcher: NextResultMatcher::default(),
      receive_buffer: BinaryHeap::new(),
    },
  )
}

impl<D> ResultsQueue<D>
where
  D: Delegate,
{
  pub fn push(&self, dent: DirList<D>) -> std::result::Result<(), SendError<DirList<D>>> {
    self.sender.send(dent)
  }
}

impl<D> Iterator for ResultsQueueIter<D>
where
  D: Delegate,
{
  type Item = DirList<D>;
  fn next(&mut self) -> Option<DirList<D>> {
    let looking_for = &self.next_matcher.looking_for_index_path;
    loop {
      let top_dir_list = self.receive_buffer.peek();
      if let Some(top_dir_list) = top_dir_list {
        if top_dir_list.index_path.eq(looking_for) {
          break;
        }
      }

      if self.next_matcher.is_none() {
        return None;
      }

      match self.receiver.try_recv() {
        Ok(dir_list) => {
          self.receive_buffer.push(dir_list);
        }
        Err(err) => match err {
          TryRecvError::Empty => thread::yield_now(),
          TryRecvError::Disconnected => break,
        },
      }
    }

    if let Some(dir_list) = self.receive_buffer.pop() {
      self.next_matcher.increment_past(&dir_list);
      Some(dir_list)
    } else {
      None
    }
  }
}

impl<D> NextResultMatcher<D>
where
  D: Delegate,
{
  fn is_none(&self) -> bool {
    self.looking_for_index_path.is_empty()
  }

  fn decrement_remaining_read_dirs_at_this_level(&mut self) {
    *self.remaining_read_dirs.last_mut().unwrap() -= 1;
  }

  fn increment_past(&mut self, dir_list: &DirList<D>) {
    self.decrement_remaining_read_dirs_at_this_level();

    if dir_list.scheduled_read_dirs > 0 {
      // If visited item has children then push 0 index path, since we are now
      // looking for the first child.
      self.looking_for_index_path.push(0);
      self.remaining_read_dirs.push(dir_list.scheduled_read_dirs);
    } else {
      // Incrememnt sibling index
      self.looking_for_index_path.increment_last();

      // If no siblings remain at this level unwind stacks
      while !self.remaining_read_dirs.is_empty() && *self.remaining_read_dirs.last().unwrap() == 0 {
        self.looking_for_index_path.pop();
        self.remaining_read_dirs.pop();
        // Finished processing level, so increment sibling index
        if !self.looking_for_index_path.is_empty() {
          self.looking_for_index_path.increment_last();
        }
      }
    }
  }
}

impl<D> Default for NextResultMatcher<D>
where
  D: Delegate,
{
  fn default() -> NextResultMatcher<D> {
    NextResultMatcher {
      looking_for_index_path: IndexPath::with_vec(vec![0]),
      remaining_read_dirs: vec![1],
      phantom: PhantomData,
    }
  }
}
