# Async/Await Patterns

Understanding asynchronous programming across languages.

## JavaScript Async

JavaScript uses Promises and async/await:

```javascript
async function fetchData(url) {
  try {
    const response = await fetch(url);
    const data = await response.json();
    return data;
  } catch (error) {
    console.error('Fetch failed:', error);
    throw error;
  }
}

// Parallel execution
const [users, posts] = await Promise.all([
  fetchData('/api/users'),
  fetchData('/api/posts')
]);
```

## Rust Async

Rust async is zero-cost and uses futures:

```rust
use tokio;

#[tokio::main]
async fn main() {
    let result = fetch_data("https://api.example.com").await;
    println!("{:?}", result);
}

async fn fetch_data(url: &str) -> Result<String, reqwest::Error> {
    reqwest::get(url).await?.text().await
}
```

## Python Async

Python uses asyncio for async programming:

```python
import asyncio
import aiohttp

async def fetch_data(url):
    async with aiohttp.ClientSession() as session:
        async with session.get(url) as response:
            return await response.text()

async def main():
    data = await fetch_data('https://api.example.com')
    print(data)

asyncio.run(main())
```

## Common Patterns

### Error Handling

Always handle async errors properly:
- Use try/catch in JavaScript
- Use Result types in Rust
- Use try/except in Python

### Cancellation

Support cancellation tokens for long-running operations.

### Timeouts

Always set timeouts on async operations to prevent hangs.
