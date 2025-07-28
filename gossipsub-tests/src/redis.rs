use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, Client, RedisError};
use std::time::Duration;

/// Redis client wrapper for test coordination between multiple nodes.
pub(crate) struct RedisClient {
    inner: MultiplexedConnection,
}

impl RedisClient {
    /// Creates a new Redis client connection.
    pub(crate) async fn new(host: String, port: u16) -> Result<Self, RedisError> {
        let client = Client::open(format!("redis://{host}:{port}/"))?;
        let connection = client.get_multiplexed_async_connection().await?;

        Ok(RedisClient { inner: connection })
    }

    /// Increments a counter and waits until it reaches the target value.
    ///
    /// Used for synchronizing multiple test nodes at specific checkpoints.
    pub(crate) async fn signal_and_wait(&mut self, key: &str, target: u64) {
        let mut count: u64 = self.inner.incr(key, 1_u64).await.unwrap();

        loop {
            if count >= target {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            count = self.inner.get(key).await.unwrap();
        }
    }

    /// Pushes a string to the end of a Redis list.
    pub(crate) async fn push(&mut self, key: &str, value: String) {
        let _: () = self.inner.rpush(key, value).await.unwrap();
    }

    /// Retrieves all elements from a Redis list.
    pub(crate) async fn list(&mut self, key: &str) -> Vec<String> {
        self.inner.lrange(key, 0, -1).await.unwrap()
    }
}
