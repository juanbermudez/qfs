# Rust Programming Guide

Rust is a systems programming language focused on safety, speed, and concurrency.

## Memory Safety

Rust guarantees memory safety without garbage collection through its ownership system.
The borrow checker validates all references at compile time, preventing data races
and null pointer dereferences.

### Ownership Rules

1. Each value has an owner
2. Only one owner at a time
3. Value is dropped when owner goes out of scope

```rust
fn main() {
    let s1 = String::from("hello");
    let s2 = s1; // s1 is moved to s2
    println!("{}", s2);
}
```

## Async/Await

Rust supports asynchronous programming with async/await syntax:

```rust
async fn fetch_data() -> Result<String, Error> {
    let response = client.get(url).await?;
    Ok(response.text().await?)
}
```

## Error Handling

Rust uses Result and Option types for error handling:

```rust
fn divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        Err("Division by zero".to_string())
    } else {
        Ok(a / b)
    }
}
```

## Concurrency

Rust enables fearless concurrency through its type system:

```rust
use std::thread;
use std::sync::mpsc;

fn main() {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        tx.send("Hello from thread").unwrap();
    });

    println!("{}", rx.recv().unwrap());
}
```
