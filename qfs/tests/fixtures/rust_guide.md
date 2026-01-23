# Rust Programming Guide

Rust is a systems programming language that runs blazingly fast, prevents segfaults, and guarantees thread safety.

## Getting Started

To install Rust, use rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Key Features

- **Zero-cost abstractions**: Pay only for what you use
- **Move semantics**: Ownership model prevents data races
- **Pattern matching**: Powerful and expressive
- **Type inference**: Less boilerplate

## Memory Safety

Rust's ownership system ensures memory safety without garbage collection. The borrow checker validates all references at compile time.

```rust
fn main() {
    let s1 = String::from("hello");
    let s2 = s1; // s1 is moved to s2
    // println!("{}", s1); // This would fail to compile
    println!("{}", s2);
}
```

## Concurrency

Rust prevents data races at compile time through its ownership and type system.

```rust
use std::thread;

fn main() {
    let v = vec![1, 2, 3];

    let handle = thread::spawn(move || {
        println!("Vector: {:?}", v);
    });

    handle.join().unwrap();
}
```
