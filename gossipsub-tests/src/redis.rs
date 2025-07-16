use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, Client, RedisError};
use std::time::Duration;

pub(crate) struct RedisClient {
    inner: MultiplexedConnection,
}

impl RedisClient {
    pub(crate) async fn new(host: String, port: u16) -> Result<Self, RedisError> {
        let client = Client::open(format!("redis://{host}:{port}/"))?;
        let connection = client.get_multiplexed_async_connection().await?;

        Ok(RedisClient { inner: connection })
    }

    /// Signal an entry on the given key and then wait until the specified value has been reached.
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

    /// Push a string at the tail of the list
    pub(crate) async fn push(&mut self, key: &str, value: String) {
        let _: () = self.inner.rpush(key, value).await.unwrap();
    }

    /// Returns all elements from a Redis list.
    pub(crate) async fn list(&mut self, key: &str) -> Vec<String> {
        self.inner.lrange(key, 0, -1).await.unwrap()
    }
}
