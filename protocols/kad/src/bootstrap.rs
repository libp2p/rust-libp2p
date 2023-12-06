use futures::FutureExt;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_timer::Delay;

use crate::QueryStats;

#[derive(Debug)]
pub(crate) struct Status {
    interval: Option<Duration>,
    next_periodic_bootstrap: Option<Delay>,
    waker: Option<Waker>,

    bootstrap_asap: bool,

    is_bootstrapping: bool,
    any_bootstrap_succeeded: bool,
}

impl Status {
    pub(crate) fn new(interval: Option<Duration>) -> Self {
        Self {
            interval,
            next_periodic_bootstrap: interval.map(Delay::new),
            waker: None,
            bootstrap_asap: false,
            is_bootstrapping: false,
            any_bootstrap_succeeded: false,
        }
    }

    pub(crate) fn on_new_peer_in_routing_table(&mut self) {
        self.bootstrap_asap = true;

        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }

    pub(crate) fn on_started(&mut self) {
        self.is_bootstrapping = true;
    }

    pub(crate) fn on_result(&mut self, stats: &QueryStats) {
        if stats.num_successes() > 0 {
            self.any_bootstrap_succeeded = true;
        }

        self.is_bootstrapping = false;
    }

    pub(crate) fn poll_next_bootstrap(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.bootstrap_asap {
            self.bootstrap_asap = false;
            if let (Some(interval), Some(delay)) =
                (self.interval, self.next_periodic_bootstrap.as_mut())
            {
                delay.reset(interval);
            }

            return Poll::Ready(());
        }

        if let (Some(interval), Some(delay)) =
            (self.interval, self.next_periodic_bootstrap.as_mut())
        {
            if let Poll::Ready(()) = delay.poll_unpin(cx) {
                delay.reset(interval);
                return Poll::Ready(());
            }
        }

        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    #[cfg(test)]
    async fn next(&mut self) {
        std::future::poll_fn(|cx| self.poll_next_bootstrap(cx)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use instant::Instant;

    const MS_100: Duration = Duration::from_millis(100);

    #[async_std::test]
    async fn given_periodic_bootstrap_when_failed_then_will_try_again_on_next_connection() {
        let mut status = Status::new(Some(Duration::from_secs(1)));

        status.next().await; // Await periodic bootstrap

        status.on_started();
        status.on_result(&QueryStats::empty().with_successes(0)); // Boostrap failed

        status.on_new_peer_in_routing_table(); // Connected to a new peer though!

        assert!(
            async_std::future::timeout(Duration::from_millis(500), status.next())
                .await
                .is_ok(),
            "bootstrap to be triggered in less then the configured delay because we connected to a new peer"
        );
    }

    #[test]
    fn given_no_periodic_bootstrap_when_failed_then_will_try_again_on_next_connection() {
        let mut status = Status::new(None);

        // User manually triggered a bootstrap
        status.on_started();
        status.on_result(&QueryStats::empty().with_successes(0)); // Boostrap failed

        status.on_new_peer_in_routing_table(); // Connected to a new peer though!

        assert!(
            status.next().now_or_never().is_some(),
            "bootstrap to be triggered immediately because we connected to a new peer"
        )
    }

    #[async_std::test]
    async fn given_periodic_bootstrap_when_routing_table_updated_then_wont_bootstrap_until_next_interval(
    ) {
        let mut status = Status::new(Some(MS_100));

        status.on_new_peer_in_routing_table();

        let start = Instant::now();
        status.next().await;
        let elapsed = Instant::now().duration_since(start);

        assert!(elapsed < Duration::from_millis(1));

        let start = Instant::now();
        status.next().await;
        let elapsed = Instant::now().duration_since(start);

        assert!(elapsed > MS_100);
    }

    #[async_std::test]
    async fn given_no_periodic_bootstrap_when_new_entry_then_will_bootstrap() {
        let mut status = Status::new(None);

        status.on_new_peer_in_routing_table();

        status.next().await;
    }

    #[async_std::test]
    async fn given_periodic_bootstrap_triggers_periodically() {
        let mut status = Status::new(Some(MS_100));

        for _ in 0..5 {
            let start = Instant::now();

            status.next().await;

            let elapsed = Instant::now().duration_since(start);

            assert!(elapsed > (MS_100 - Duration::from_millis(10))); // Subtract 10ms to avoid flakes.
        }
    }
}
