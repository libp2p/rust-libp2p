// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

#![cfg(test)]

use super::*;

use std::io;

use futures::future;
use crate::tests::dummy_handler::{Handler, InEvent, OutEvent};
use crate::tests::dummy_muxer::DummyMuxer;
use void::Void;
use crate::PeerId;

type TestHandledNodesTasks = HandledNodesTasks<InEvent, OutEvent, Handler, io::Error, io::Error, ()>;

struct HandledNodeTaskTestBuilder {
    muxer: DummyMuxer,
    handler: Handler,
    task_count: usize,
}

impl HandledNodeTaskTestBuilder {
    fn new() -> Self {
        HandledNodeTaskTestBuilder {
            muxer: DummyMuxer::new(),
            handler: Handler::default(),
            task_count: 0,
        }
    }

    fn with_tasks(&mut self, amt: usize) -> &mut Self {
        self.task_count = amt;
        self
    }
    fn handled_nodes_tasks(&mut self) -> (TestHandledNodesTasks, Vec<TaskId>) {
        let mut handled_nodes = HandledNodesTasks::new();
        let peer_id = PeerId::random();
        let mut task_ids = Vec::new();
        for _i in 0..self.task_count {
            let fut = future::ok((peer_id.clone(), self.muxer.clone()));
            task_ids.push(
                handled_nodes.add_reach_attempt(fut, (), self.handler.clone())
            );
        }
        (handled_nodes, task_ids)
    }
}


// Tests for HandledNodeTasks

#[test]
fn query_for_tasks() {
    let (mut handled_nodes, task_ids) = HandledNodeTaskTestBuilder::new()
        .with_tasks(3)
        .handled_nodes_tasks();

    assert_eq!(task_ids.len(), 3);
    assert_eq!(handled_nodes.task(TaskId(2)).unwrap().id(), task_ids[2]);
    assert!(handled_nodes.task(TaskId(545534)).is_none());
}

#[test]
fn iterate_over_all_tasks() {
    let (handled_nodes, task_ids) = HandledNodeTaskTestBuilder::new()
        .with_tasks(3)
        .handled_nodes_tasks();

    let mut tasks: Vec<TaskId> = handled_nodes.tasks().collect();
    assert!(tasks.len() == 3);
    tasks.sort_by_key(|t| t.0 );
    assert_eq!(tasks, task_ids);
}

#[test]
fn add_reach_attempt_prepares_a_new_task() {
    let mut handled_nodes: HandledNodesTasks<_, _, _, _, _, _> = HandledNodesTasks::new();
    assert_eq!(handled_nodes.tasks().count(), 0);
    assert_eq!(handled_nodes.to_spawn.len(), 0);

    handled_nodes.add_reach_attempt(future::empty::<_, Void>(), (), Handler::default());

    assert_eq!(handled_nodes.tasks().count(), 1);
    assert_eq!(handled_nodes.to_spawn.len(), 1);
}
