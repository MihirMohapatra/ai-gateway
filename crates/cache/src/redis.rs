use redis::{AsyncCommands, Client, RedisResult};

pub struct RedisCache {
    client: Client,
}

impl RedisCache {
    pub fn new(url: &str) -> RedisResult<Self> {
        let client = Client::open(url)?;
        Ok(Self { client })
    }

    pub async fn get(&self, key: &str) -> RedisResult<Option<Vec<u8>>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        conn.get(key).await
    }

    pub async fn set(&self, key: &str, value: Vec<u8>, ttl_secs: usize) -> RedisResult<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let _: () = conn.set_ex(key, value, ttl_secs as u64).await?;
        Ok(())
    }
}
