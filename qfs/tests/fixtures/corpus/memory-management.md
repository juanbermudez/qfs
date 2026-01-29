# Memory Management in Systems Programming

Understanding memory management is crucial for systems programming.

## Stack vs Heap

### Stack Memory

- Fast allocation and deallocation
- LIFO (Last In, First Out) structure
- Fixed size at compile time
- Automatically managed

### Heap Memory

- Dynamic allocation
- Slower than stack
- Must be manually managed (or use GC)
- Can grow as needed

## Ownership in Rust

Rust's ownership system prevents memory issues at compile time:

```rust
fn main() {
    // Stack allocation
    let x = 5;

    // Heap allocation
    let s = String::from("hello");

    // Move semantics
    let s2 = s;  // s is no longer valid

    // Clone for deep copy
    let s3 = s2.clone();
}
```

## Smart Pointers

### Box<T>

Heap allocation with single ownership:

```rust
let b = Box::new(5);
```

### Rc<T>

Reference counting for shared ownership:

```rust
use std::rc::Rc;
let a = Rc::new(5);
let b = Rc::clone(&a);
```

### Arc<T>

Atomic reference counting for thread safety:

```rust
use std::sync::Arc;
let a = Arc::new(5);
let b = Arc::clone(&a);
```

## Garbage Collection

Languages like Python and JavaScript use garbage collection:

- Mark and sweep
- Reference counting
- Generational GC

## Memory Leaks

Common causes of memory leaks:
- Circular references
- Forgotten cleanup
- Global state accumulation
- Event listener buildup
