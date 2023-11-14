use std::collections::VecDeque;

/// Manages associated data of request-response protocols whilst they are in-flight.
///
/// The [`InflightProtocolDataQueue`] ensures that for each in-flight protocol, there is a corresponding piece of associated data.
/// We process the associated data in a FIFO order based on the incoming responses.
/// In other words, we assume that requests and their responses are either temporally ordered or it doesn't matter, which piece of data is paired with a particular response.
pub struct InflightProtocolDataQueue<D, Req, Res> {
    data_of_inflight_requests: VecDeque<D>,
    pending_requests: VecDeque<Req>,
    received_responses: VecDeque<Res>,
}

impl<D, Req, Res> Default for InflightProtocolDataQueue<D, Req, Res> {
    fn default() -> Self {
        Self {
            pending_requests: Default::default(),
            received_responses: Default::default(),
            data_of_inflight_requests: Default::default(),
        }
    }
}

impl<D, Req, Res> InflightProtocolDataQueue<D, Req, Res> {
    /// Enqueues a new request along-side with the associated data.
    ///
    /// The request will be returned again from [`InflightProtocolDataQueue::next_request`].
    pub fn enqueue_request(&mut self, request: Req, data: D) {
        self.pending_requests.push_back(request);
        self.data_of_inflight_requests.push_back(data);
    }

    /// Submits a response to the queue.
    ///
    /// A pair of response and data will be returned from [`InflightProtocolDataQueue::next_completed`].
    pub fn submit_response(&mut self, res: Res) {
        debug_assert!(
            self.data_of_inflight_requests.len() > self.received_responses.len(),
            "Expect to not provide more responses than requests were started"
        );
        self.received_responses.push_back(res);
    }

    /// How many protocols are currently in-flight.
    pub fn num_inflight(&self) -> usize {
        self.data_of_inflight_requests.len() - self.received_responses.len()
    }

    pub fn next_completed(&mut self) -> Option<(Res, D)> {
        let res = self.received_responses.pop_front()?;
        let data = self.data_of_inflight_requests.pop_front()?;

        Some((res, data))
    }

    pub fn next_request(&mut self) -> Option<Req> {
        let req = self.pending_requests.pop_front()?;

        Some(req)
    }
}
